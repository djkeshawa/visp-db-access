import { chromium } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { installAuditTransport } from '../tests/e2e/audit-transport.ts';
const phase = process.argv[2] ?? 'after',
  base = process.env.AUDIT_URL ?? 'http://127.0.0.1:5175',
  output = process.env.AUDIT_OUTPUT ?? `screenshots/audit/${phase}/states`;
await mkdir(output, { recursive: true });
const only = process.argv[3] ? new RegExp(process.argv[3]) : null;
const browser = await chromium.launch(),
  results = only
    ? JSON.parse(await readFile(`${output}/report.json`, 'utf8'))
    : [],
  cluster = '00000000-0000-4000-8000-000000000100';
const lists = [
  ['overview', '/overview', '/'],
  ['clusters', '/clusters', '/clusters'],
  ['schema', `/clusters/${cluster}?tab=schema`, `/clusters/${cluster}/schema`],
  ['approvals', '/approvals', '/approvals'],
  ['history', '/history', '/history'],
  ['users', '/users', '/users'],
  ['access', '/access', '/grants'],
  ['audit', '/audit', '/audit'],
  ['discovery', '/settings?tab=discovery', '/discovery/sources'],
  ['discovered', '/clusters?tab=discovered', '/discovery/resources'],
];
async function capture(page, name, mode) {
  const existing = results.findIndex(
    (result) => result.name === name && result.mode === mode,
  );
  if (existing >= 0) results.splice(existing, 1);
  await page.screenshot({
    timeout: 60000,
    path: `${output}/${mode}-${name}.png`,
    fullPage: true,
    animations: 'disabled',
  });
  const axe = await new AxeBuilder({ page }).analyze();
  results.push({
    mode,
    name,
    overflow: await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    violations: axe.violations.map((v) => ({
      id: v.id,
      impact: v.impact,
      nodes: v.nodes.map((n) => ({
        target: n.target,
        summary: n.failureSummary,
      })),
    })),
  });
}
for (const width of [1440, 390])
  for (const theme of ['light', 'dark']) {
    const mode = `${width}-${theme}`;
    for (const kind of ['empty', 'loading', 'error'])
      for (const [name, path, target] of lists) {
        if (only && !only.test(`${name}-${kind}`)) continue;
        const context = await browser.newContext({
          viewport: { width, height: width === 1440 ? 900 : 844 },
          colorScheme: theme,
        });
        const page = await context.newPage();
        // Authenticate through the original mock transport before applying faults.
        await installAuditTransport(page);
        await page.addInitScript(() => {
          window.__uiAuditAutoLogin = true;
        });
        await page.addInitScript(
          (value) => {
            window.__uiAuditFault = value;
          },
          {
            path: target === '/' && kind !== 'empty' ? '/overview' : target,
            kind,
          },
        );
        await page.goto(`${base}${path}`);
        await page.waitForTimeout(kind === 'loading' ? 600 : 1800);
        if (name === 'users' && kind === 'empty')
          await page
            .getByRole('textbox', { name: 'Search users' })
            .fill('No matching identity');
        await capture(page, `${name}-${kind}`, mode);
        await context.close();
        await writeFile(
          `${output}/report.json`,
          JSON.stringify(results, null, 2),
        );
      }
    if (only) continue;
    const context = await browser.newContext({
      viewport: { width, height: width === 1440 ? 900 : 844 },
      colorScheme: theme,
    });
    const page = await context.newPage();
    await installAuditTransport(page);
    await page.goto(`${base}/login`);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByRole('heading', { name: 'Workspace overview' }).waitFor();
    await page.evaluate((path) => {
      history.pushState({}, '', path);
      dispatchEvent(new PopStateEvent('popstate'));
    }, `/console?cluster_id=${cluster}`);
    await page.getByText('Safe to run', { exact: true }).waitFor();
    await page.evaluate(() => {
      window.__uiAuditFault = { path: '/clusters/', kind: 'long' };
    });
    await page.getByRole('button', { name: 'Run', exact: false }).click();
    await page.getByRole('grid').waitFor();
    await capture(page, 'long-truncated-results', mode);
    await page
      .locator('.cell-value')
      .filter({ hasText: 'Long text' })
      .first()
      .click();
    await capture(page, 'long-cell-inspector', mode);
    if (await page.getByRole('dialog').count())
      await page
        .getByRole('dialog')
        .getByRole('button', { name: 'Close dialog' })
        .click();
    await page.evaluate(() => {
      window.__uiAuditFault = { path: '/clusters/', kind: 'expired' };
    });
    await page
      .getByRole('textbox', { name: 'SQL editor' })
      .fill('SELECT id FROM users LIMIT 73');
    await page.waitForTimeout(800);
    await capture(page, 'session-expired', mode);
    await page.keyboard.press('Escape');
    await context.close();
    await writeFile(`${output}/report.json`, JSON.stringify(results, null, 2));
    console.log(`${phase} state audit: ${mode} complete`);
  }
await browser.close();
console.log(`Captured ${results.length} state screens`);
