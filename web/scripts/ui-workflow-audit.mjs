import { chromium } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { mkdir, writeFile } from 'node:fs/promises';
import { installAuditTransport } from '../tests/e2e/audit-transport.ts';

const base = process.env.AUDIT_URL ?? 'http://127.0.0.1:5175';
const output = process.env.AUDIT_OUTPUT ?? 'screenshots/audit/after/workflows',
  results = [];
await mkdir(output, { recursive: true });
const browser = await chromium.launch();
async function go(page, path) {
  await page.evaluate((path) => {
    history.pushState({}, '', path);
    dispatchEvent(new PopStateEvent('popstate'));
  }, path);
  await page.waitForTimeout(800);
}
async function capture(page, mode, name) {
  await page.waitForTimeout(200);
  await page.screenshot({
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
  await writeFile(`${output}/report.json`, JSON.stringify(results, null, 2));
}
async function close(page) {
  // Escape closes the current popup even when a long drawer has scrolled its header away.
  if (
    !(await page.getByRole('dialog').count()) &&
    (await page.getByRole('button', { name: 'Close approval' }).count())
  ) {
    await page.getByRole('button', { name: 'Close approval' }).click();
    return;
  }
  await page.keyboard.press('Escape');
  await page.getByRole('dialog').waitFor({ state: 'hidden' });
}
for (const width of [1440, 390])
  for (const theme of ['light', 'dark']) {
    const mode = `${width}-${theme}`,
      context = await browser.newContext({
        viewport: { width, height: width === 1440 ? 900 : 844 },
        colorScheme: theme,
      });
    const page = await context.newPage();
    await installAuditTransport(page);
    await page.goto(`${base}/login`);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByRole('heading', { name: 'Workspace overview' }).waitFor();
    if (width === 390) {
      await page.getByRole('button', { name: 'Open navigation' }).click();
      await capture(page, mode, 'mobile-navigation');
      await page.keyboard.press('Escape');
    }
    await page.keyboard.press('?');
    await capture(page, mode, 'shortcut-help');
    await close(page);
    await page.keyboard.press('Control+k');
    await page
      .getByRole('textbox', { name: 'Search pages and clusters' })
      .fill('clstr');
    await capture(page, mode, 'fuzzy-palette');
    await close(page);
    await go(page, '/console?cluster_id=00000000-0000-4000-8000-000000000100');
    await page.getByText('Safe to run', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Run', exact: false }).click();
    await page.getByRole('grid').waitFor();
    await page
      .getByRole('button', { name: 'Column options for name', exact: true })
      .click();
    await capture(page, mode, 'column-menu');
    await page.getByRole('menuitem', { name: 'Sort descending' }).click();
    await page
      .getByRole('textbox', { name: 'Filter fetched rows' })
      .fill('No fetched match');
    await capture(page, mode, 'filtered-results-empty');
    await page.getByRole('textbox', { name: 'Filter fetched rows' }).fill('');
    await page.getByRole('button', { name: 'Export', exact: true }).click();
    await capture(page, mode, 'copy-menu');
    await page.keyboard.press('Escape');
    for (const name of ['Close schema panel', 'Close safety panel']) {
      const control = page.getByRole('button', { name, exact: true });
      if (await control.count()) await control.click();
    }
    await capture(page, mode, 'collapsed-panels');
    await page.getByRole('button', { name: 'Toggle schema panel' }).click();
    await page
      .locator('.schema-table > summary')
      .filter({ hasText: 'users' })
      .click({ button: 'right' });
    await capture(page, mode, 'schema-context-menu');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Close schema panel' }).click();
    await page.getByRole('button', { name: 'Open safety analysis' }).click();
    await capture(page, mode, 'safety-panel');
    await page.getByRole('button', { name: 'Close safety panel' }).click();
    await page.getByRole('button', { name: 'Manage Query 1' }).click();
    await page.getByRole('menuitem', { name: 'Rename tab' }).click();
    await capture(page, mode, 'rename-tab');
    await close(page);
    await page
      .getByRole('button', { name: 'Save favorite', exact: true })
      .click();
    await page
      .getByLabel('Favorite name', { exact: true })
      .fill('Customer lookup');
    await capture(page, mode, 'save-favorite');
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Save favorite', exact: true })
      .click();
    await page.getByRole('button', { name: 'Favorites (1)' }).click();
    await capture(page, mode, 'local-favorites');
    await close(page);
    await page.getByRole('button', { name: 'History', exact: true }).click();
    await page
      .getByRole('region', { name: 'Inline query history' })
      .getByRole('button', { name: 'Restore into tab' })
      .first()
      .waitFor();
    await capture(page, mode, 'inline-history');
    await close(page);
    await page
      .getByRole('button', { name: 'New query tab', exact: true })
      .click();
    await page
      .getByRole('textbox', { name: 'SQL editor' })
      .fill('SELECT id FROM users LIMIT 41');
    await page
      .getByRole('button', { name: 'Close Query 2', exact: true })
      .click();
    await capture(page, mode, 'discard-draft');
    await close(page);
    await go(page, '/history');
    await page.locator('.sql-preview button').first().click();
    await capture(page, mode, 'history-details');
    await go(page, '/audit');
    await page.getByText('JSON details', { exact: true }).first().click();
    await capture(page, mode, 'audit-json');
    await go(page, '/access');
    await page
      .getByRole('checkbox', { name: 'Select all loaded grants' })
      .check();
    await page.getByRole('button', { name: 'Revoke selected' }).click();
    await capture(page, mode, 'bulk-grants-confirm');
    await close(page);
    await page.getByRole('button', { name: 'User menu' }).click();
    await page.getByRole('menuitem', { name: 'Change password' }).click();
    await capture(page, mode, 'password-dialog');
    await close(page);
    await go(page, '/approvals');
    await page
      .getByRole('button', { name: 'Review', exact: true })
      .first()
      .click();
    await page.getByLabel('Review note').fill('Verified WHERE target');
    await page.getByRole('button', { name: 'Approve', exact: true }).click();
    await page.getByRole('button', { name: 'Execute approved query' }).click();
    await page.getByRole('button', { name: 'Confirm', exact: true }).click();
    await page.getByText('1 row affected.').waitFor();
    await capture(page, mode, 'approval-executed');
    await close(page);
    await page.evaluate(() =>
      window.dispatchEvent(new Event('vda:unreachable')),
    );
    await capture(page, mode, 'reconnecting-banner');
    await context.close();
    console.log(`Workflow audit: ${mode} complete`);
  }
await browser.close();
console.log(`Captured ${results.length} workflow screens`);
