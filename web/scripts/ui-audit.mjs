import { chromium } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
const phase = process.argv[2] ?? 'after';
const scanEnabled = process.env.AUDIT_SCAN !== '0';
const only = process.argv[3] ? new RegExp(process.argv[3]) : null;
const base = process.env.AUDIT_URL ?? 'http://127.0.0.1:5175';
const output = process.env.AUDIT_OUTPUT ?? `screenshots/audit/${phase}`;
await mkdir(output, { recursive: true });
const browser = await chromium.launch();
const cluster = '00000000-0000-4000-8000-000000000100';
const routes = [
  ['overview', '/overview'],
  ['clusters', '/clusters'],
  ['console-empty', '/console'],
  ['console', `/clusters/${cluster}`],
  ...['schema', 'health', 'access', 'policy', 'settings'].map((tab) => [
    `cluster-${tab}`,
    `/clusters/${cluster}?tab=${tab}`,
  ]),
  ['approvals', '/approvals'],
  ['history', '/history'],
  ['users', '/users'],
  ['access', '/access'],
  ['audit', '/audit'],
  ['settings', '/settings'],
  ...(phase === 'before'
    ? []
    : [
        ['preferences', '/settings?tab=preferences'],
        ['primitives', '/ui?kitchen-sink'],
      ]),
  ['discovery', '/settings?tab=discovery'],
  ['discovered', '/clusters?tab=discovered'],
  ['404', '/missing'],
];
const results = only
  ? JSON.parse(await readFile(`${output}/report.json`, 'utf8'))
  : [];
async function navigate(page, path) {
  await page.evaluate((path) => {
    history.pushState({}, '', path);
    dispatchEvent(new PopStateEvent('popstate'));
  }, path);
  await page.waitForTimeout(700);
  await page
    .locator('[aria-label="Loading"]')
    .waitFor({ state: 'hidden' })
    .catch(() => {});
}
async function capture(page, name, mode, scan = true) {
  await page.waitForTimeout(200);
  await page.screenshot({
    path: `${output}/${mode}-${name}.png`,
    fullPage: true,
    animations: 'disabled',
  });
  if (scan && scanEnabled) {
    const axe = await new AxeBuilder({ page }).analyze();
    const previous = results.findIndex(
      (result) => result.mode === mode && result.name === name,
    );
    if (previous >= 0) results.splice(previous, 1);
    results.push({
      mode,
      name,
      url: page.url(),
      overflow: await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
      contrasts: [...axe.passes, ...axe.violations]
        .filter((rule) => rule.id === 'color-contrast')
        .flatMap((rule) =>
          rule.nodes.flatMap((node) =>
            [...node.any, ...node.all].flatMap((check) =>
              check.data?.fgColor &&
              check.data?.bgColor &&
              check.data?.contrastRatio
                ? [
                    {
                      foreground: check.data.fgColor,
                      background: check.data.bgColor,
                      ratio: check.data.contrastRatio,
                    },
                  ]
                : [],
            ),
          ),
        ),
      violations: axe.violations.map((v) => ({
        id: v.id,
        impact: v.impact,
        description: v.description,
        nodes: v.nodes.map((n) => ({
          target: n.target,
          summary: n.failureSummary,
        })),
      })),
    });
  }
  const main = page.locator('main.page');
  if ((await main.count()) && !(await page.getByRole('dialog').count())) {
    const scrollable = await main.evaluate(
      (el) => el.scrollHeight > el.clientHeight + 10,
    );
    if (scrollable && !name.endsWith('-bottom')) {
      await main.evaluate((el) => {
        el.scrollTop = el.scrollHeight;
      });
      await page.waitForTimeout(200);
      await capture(page, `${name}-bottom`, mode, scan);
      await main.evaluate((el) => {
        el.scrollTop = 0;
      });
    }
  }
}
for (const width of [1440, 390])
  for (const theme of ['light', 'dark']) {
    const mode = `${width}-${theme}`;
    const context = await browser.newContext({
      viewport: { width, height: width === 1440 ? 900 : 844 },
      colorScheme: theme,
    });
    const page = await context.newPage();
    await page.goto(`${base}/login`);
    await capture(page, 'login', mode);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByRole('heading', { name: 'Workspace overview' }).waitFor();
    for (const [name, path] of routes) {
      if (only && !only.test(name)) continue;
      await navigate(page, path);
      await capture(page, name, mode);
    }
    if (only) {
      await context.close();
      await writeFile(
        `${output}/report.json`,
        JSON.stringify(results, null, 2),
      );
      continue;
    }
    await navigate(page, `/clusters/${cluster}`);
    await page.getByText('Safe to run', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Run', exact: false }).click();
    await page.getByRole('grid').waitFor();
    await capture(page, 'results', mode);
    await page
      .locator('.cell-value')
      .filter({ hasText: '"plan"' })
      .first()
      .click();
    await capture(page, 'cell-inspector', mode);
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Close dialog' })
      .click();
    await page
      .getByRole('textbox', { name: 'SQL editor' })
      .fill('DELETE FROM users');
    await page.getByText('Blocked', { exact: true }).waitFor();
    await capture(page, 'blocked', mode);
    await page
      .getByRole('textbox', { name: 'SQL editor' })
      .fill("UPDATE users SET name = 'Maya' WHERE id = 42");
    await page.getByText('Needs approval').waitFor();
    await page
      .getByRole('button', { name: 'Request approval…', exact: true })
      .click();
    await capture(page, 'approval-request', mode);
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Close dialog' })
      .click();
    await navigate(page, '/approvals');
    await page
      .getByRole('button', { name: 'Review', exact: true })
      .first()
      .click();
    await page.waitForTimeout(500);
    await capture(page, 'approval-detail', mode);
    if (await page.getByRole('dialog').count())
      await page
        .getByRole('dialog')
        .getByRole('button', { name: 'Close dialog' })
        .click();
    else await page.getByRole('button', { name: 'Close approval' }).click();
    await navigate(page, '/clusters');
    await page
      .getByRole('button', { name: 'Add cluster', exact: true })
      .click();
    await capture(page, 'add-cluster', mode);
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Close dialog' })
      .click();
    await page.keyboard.press('Control+k');
    await capture(page, 'palette', mode);
    await page.keyboard.press('Escape');
    await navigate(page, '/settings?tab=discovery');
    await page.getByRole('button', { name: 'Add source', exact: true }).click();
    await capture(page, 'discovery-source-dialog', mode);
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Close dialog' })
      .click();
    await page
      .getByRole('button', { name: 'History', exact: true })
      .first()
      .click();
    await page.waitForTimeout(500);
    await capture(page, 'discovery-runs-dialog', mode);
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Close dialog' })
      .click();
    await navigate(page, '/clusters?tab=discovered');
    await page
      .getByRole('button', { name: 'Import', exact: true })
      .first()
      .click();
    await capture(page, 'discovery-import-dialog', mode);
    await context.close();
    console.log(`${phase}: ${mode} complete`);
    await writeFile(`${output}/report.json`, JSON.stringify(results, null, 2));
  }
await browser.close();
console.log(
  `Captured ${results.length} screens; serious/critical: ${results.reduce((n, r) => n + r.violations.filter((v) => ['serious', 'critical'].includes(v.impact)).length, 0)}`,
);
