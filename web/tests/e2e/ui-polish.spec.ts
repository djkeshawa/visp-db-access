import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { installAuditTransport } from './audit-transport';
import { mkdir, writeFile } from 'node:fs/promises';
const cluster = '00000000-0000-4000-8000-000000000100';
async function signIn(page: import('@playwright/test').Page) {
  await page.goto('/login');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.getByRole('heading', { name: 'Workspace overview' }).waitFor();
}
async function go(page: import('@playwright/test').Page, path: string) {
  await page.evaluate((path) => {
    history.pushState({}, '', path);
    dispatchEvent(new PopStateEvent('popstate'));
  }, path);
  await page.locator('main.page').waitFor();
}
async function console(page: import('@playwright/test').Page) {
  await go(page, `/console?cluster_id=${cluster}`);
  await page.getByText('Safe to run', { exact: true }).waitFor();
}
for (const kind of ['loading', 'error'] as const)
  test(`page headings remain accessible during ${kind}`, async ({ page }) => {
    await installAuditTransport(page);
    await page.addInitScript(() => {
      window.__uiAuditAutoLogin = true;
    });
    for (const [path, target] of [
      ['/overview', '/overview'],
      [`/clusters/${cluster}?tab=schema`, `/clusters/${cluster}`],
    ] as const) {
      await page.addInitScript(
        (fault) => {
          window.__uiAuditFault = fault;
        },
        { path: target, kind },
      );
      await page.goto(path);
      await expect(page.locator('main.page h1')).toBeVisible();
      if (kind === 'loading')
        await expect(
          page.getByRole('status', { name: 'Loading', exact: true }).first(),
        ).toBeVisible();
      else await expect(page.getByRole('alert').first()).toBeVisible();
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    }
  });
test('setup checklist uses API state and deep-links the first-run steps', async ({
  page,
}) => {
  await installAuditTransport(page, { path: '/', kind: 'empty' });
  await signIn(page);
  const setup = page.getByRole('region', { name: 'Workspace setup' });
  await expect(setup).toContainText('0 of 4 complete');
  await setup.getByRole('link', { name: 'Add a cluster', exact: true }).click();
  await expect(
    page.getByRole('dialog', { name: 'Add a cluster' }),
  ).toBeVisible();
});
test('command palette fuzzy actions, keyboard selection and shortcut help', async ({
  page,
}) => {
  await signIn(page);
  await page.keyboard.press('Control+k');
  await page
    .getByRole('textbox', { name: 'Search pages and clusters' })
    .fill('clstr');
  await expect(
    page
      .getByRole('dialog')
      .getByRole('button', { name: 'Clusters', exact: true }),
  ).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page.locator('h1')).toContainText('Clusters');
  await page.keyboard.press('?');
  await expect(
    page.getByRole('dialog', { name: 'Keyboard shortcuts' }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Control+k');
  await page
    .getByRole('textbox', { name: 'Search pages and clusters' })
    .fill('Add cluster');
  await page.keyboard.press('Enter');
  await expect(
    page.getByRole('dialog', { name: 'Add a cluster' }),
  ).toBeVisible();
});
test('tab operations retain identity, browser drafts and local favorites', async ({
  page,
}) => {
  await signIn(page);
  await console(page);
  await page
    .getByRole('button', { name: 'New query tab', exact: true })
    .click();
  await page
    .getByRole('textbox', { name: 'SQL editor' })
    .fill('SELECT id FROM users LIMIT 17');
  await page.getByRole('button', { name: 'Manage Query 2' }).click();
  await page.getByRole('menuitem', { name: 'Rename tab' }).click();
  await page.getByLabel('Tab name', { exact: true }).fill('Customer lookup');
  await page.getByRole('button', { name: 'Rename tab', exact: true }).click();
  await page
    .getByRole('button', { name: 'Save favorite', exact: true })
    .click();
  await page.getByLabel('Favorite name', { exact: true }).fill('Customer IDs');
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Save favorite', exact: true })
    .click();
  await page.getByRole('button', { name: 'Manage Customer lookup' }).click();
  await page.getByRole('menuitem', { name: 'Move left' }).click();
  await expect(page.getByRole('tab').first()).toContainText('Customer lookup');
  await go(page, '/overview');
  await console(page);
  await expect(
    page.getByRole('tab', { name: 'Customer lookup', exact: true }),
  ).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('button', { name: 'Favorites (1)' }).click();
  await page.getByRole('button', { name: 'Restore into tab' }).click();
  await expect(page.getByRole('textbox', { name: 'SQL editor' })).toContainText(
    'LIMIT 17',
  );
});
test('result snapshot filters, sorts, hides, pins, inspects and exports', async ({
  page,
}) => {
  await signIn(page);
  await console(page);
  await page.getByRole('button', { name: 'Run', exact: false }).click();
  await page.getByRole('grid').waitFor();
  await page.getByRole('textbox', { name: 'Filter fetched rows' }).fill('Maya');
  await expect(
    page.getByRole('gridcell').filter({ hasText: 'Jordan' }),
  ).toHaveCount(0);
  await page
    .getByRole('button', { name: 'Column options for name', exact: true })
    .click();
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByRole('menuitem', { name: 'Sort descending' }).click();
  await expect(
    page.getByRole('columnheader', { name: 'name', exact: true }),
  ).toHaveAttribute('aria-sort', 'descending');
  await page
    .getByRole('button', { name: 'Column options for email', exact: true })
    .click();
  await page.getByRole('menuitem', { name: 'Hide column' }).click();
  await expect(
    page.getByRole('columnheader', { name: 'email', exact: true }),
  ).toHaveCount(0);
  await page.getByRole('button', { name: 'Show all columns' }).click();
  await page
    .getByRole('button', { name: 'Column options for name', exact: true })
    .click();
  await page.getByRole('menuitem', { name: 'Pin column', exact: true }).click();
  await expect(
    page.getByRole('columnheader', { name: 'name', exact: true }),
  ).toHaveClass(/pinned-column/);
  await page.locator('[data-cell="0-0"]').focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Cell value' })).toBeVisible();
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Export', exact: true }).click();
  await expect(
    page.getByRole('menuitem', { name: 'Copy as Markdown' }),
  ).toBeVisible();
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.keyboard.press('Escape');
  const resize = page.getByRole('separator', {
    name: 'Resize editor and results',
  });
  const initialHeight = Number(await resize.getAttribute('aria-valuenow'));
  await resize.focus();
  await page.keyboard.press('ArrowDown');
  await expect(resize).toHaveAttribute(
    'aria-valuenow',
    String(initialHeight + 16),
  );
});
test('inline history restores into a fresh tab', async ({ page }) => {
  await signIn(page);
  await console(page);
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await page
    .getByRole('region', { name: 'Inline query history' })
    .getByRole('button', { name: 'Restore into tab' })
    .first()
    .click();
  await expect(page.getByRole('tab')).toHaveCount(2);
});
test('mobile approval review has no horizontal page overflow and respects four-eyes', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await go(page, '/approvals');
  await page
    .getByRole('button', { name: 'Review', exact: true })
    .first()
    .click();
  await page.getByLabel('Review note').fill('Verified target rows');
  await expect(
    page.getByRole('button', { name: 'Approve', exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await expect(page.getByRole('dialog').getByText(/Expires in/)).toBeVisible();
  await page.screenshot({
    path: 'screenshots/audit/after/mobile-approval-review.png',
    fullPage: true,
  });
});
test('approval actions expire promptly in both themes and viewport sizes', async ({
  page,
}) => {
  test.setTimeout(90000);
  await installAuditTransport(page, { path: '/approvals/', kind: 'expiring' });
  const output = 'screenshots/audit/after/expired-approvals',
    results = [];
  await mkdir(output, { recursive: true });
  for (const width of [1440, 390])
    for (const theme of ['light', 'dark'] as const) {
      await page.setViewportSize({ width, height: width === 1440 ? 900 : 844 });
      await page.emulateMedia({ colorScheme: theme });
      await signIn(page);
      await go(page, '/approvals');
      await page
        .getByRole('button', { name: 'Review', exact: true })
        .first()
        .click();
      await expect(
        page.getByRole('button', { name: 'Approve', exact: true }),
      ).toBeVisible();
      await expect(page.locator('.approval-expiry')).toContainText('Expired', {
        timeout: 4500,
      });
      await expect(
        page.getByRole('button', { name: 'Approve', exact: true }),
      ).toHaveCount(0);
      await expect(
        page.getByRole('button', { name: 'Execute approved query' }),
      ).toHaveCount(0);
      const mode = `${width}-${theme}`,
        axe = await new AxeBuilder({ page }).analyze();
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      );
      expect(axe.violations).toEqual([]);
      expect(overflow).toBe(false);
      await page.screenshot({
        path: `${output}/${mode}-expired-approval.png`,
        fullPage: true,
        animations: 'disabled',
      });
      results.push({
        mode,
        name: 'expired-approval',
        overflow,
        violations: axe.violations,
      });
      await writeFile(
        `${output}/report.json`,
        JSON.stringify(results, null, 2),
      );
    }
});
test('date range filters loaded history and CSV export is explicit', async ({
  page,
}) => {
  await signIn(page);
  await go(page, '/history');
  await page.getByRole('button', { name: 'Date range', exact: true }).click();
  await page.getByRole('button', { name: 'Custom', exact: true }).click();
  await page.getByLabel('From date', { exact: true }).fill('2099-01-01');
  await expect(page.getByText('No queries yet', { exact: true })).toBeVisible();
  await expect(
    page.getByRole('button', { name: 'Export loaded rows as CSV' }),
  ).toBeDisabled();
  await page.getByLabel('From date', { exact: true }).fill('');
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export loaded rows as CSV' }).click();
  expect((await download).suggestedFilename()).toBe('query-history.csv');
});
test('expired sessions and unavailable server preserve the editor', async ({
  page,
}) => {
  await installAuditTransport(page);
  await signIn(page);
  await console(page);
  await page.evaluate(() => {
    window.__uiAuditFault = { path: '/clusters/', kind: 'expired' };
  });
  await page
    .getByRole('textbox', { name: 'SQL editor' })
    .fill('SELECT id FROM users LIMIT 73');
  await expect(
    page.getByRole('dialog', { name: 'Your session expired' }),
  ).toBeVisible();
  await page.evaluate(() => {
    delete window.__uiAuditFault;
  });
  await page
    .getByRole('dialog')
    .getByLabel('Password', { exact: true })
    .fill('demo-password');
  await page.getByRole('button', { name: 'Resume session' }).click();
  await expect(
    page.getByRole('dialog', { name: 'Your session expired' }),
  ).toHaveCount(0);
  await expect(page.getByRole('textbox', { name: 'SQL editor' })).toContainText(
    'LIMIT 73',
  );
  await page.evaluate(() => {
    window.__uiAuditFault = { path: '/clusters/', kind: 'offline' };
  });
  await page
    .getByRole('textbox', { name: 'SQL editor' })
    .fill('SELECT id FROM users LIMIT 74');
  await expect(
    page.getByText('Cannot reach the gateway.', { exact: false }),
  ).toBeVisible();
  await page.evaluate(() => {
    delete window.__uiAuditFault;
  });
  await page.getByRole('button', { name: 'Retry now' }).click();
  await expect(
    page.getByText('Cannot reach the gateway.', { exact: false }),
  ).toHaveCount(0);
});
for (const width of [1440, 390])
  for (const theme of ['light', 'dark'] as const)
    test(`axe every route at ${width}px ${theme}`, async ({ page }) => {
      test.setTimeout(180000);
      await page.setViewportSize({ width, height: width === 1440 ? 900 : 844 });
      await page.emulateMedia({ colorScheme: theme });
      await signIn(page);
      const routes = [
        '/overview',
        '/clusters',
        '/console',
        `/console?cluster_id=${cluster}`,
        ...['schema', 'health', 'access', 'policy', 'settings'].map(
          (tab) => `/clusters/${cluster}?tab=${tab}`,
        ),
        '/approvals',
        '/history',
        '/users',
        '/access',
        '/audit',
        '/settings',
        '/settings?tab=preferences',
        '/settings?tab=discovery',
        '/clusters?tab=discovered',
        '/ui?kitchen-sink',
        '/missing',
      ];
      for (const route of routes) {
        await go(page, route);
        await page.waitForTimeout(700);
        if (!route.startsWith('/ui'))
          await expect(page.getByLabel('Loading', { exact: true })).toHaveCount(
            0,
          );
        const results = await new AxeBuilder({ page }).analyze();
        expect(
          results.violations,
          `${route} ${JSON.stringify(results.violations.map((v) => ({ id: v.id, nodes: v.nodes.map((n) => n.target) })))}`,
        ).toEqual([]);
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth <= innerWidth,
          ),
          route,
        ).toBe(true);
      }
    });

test('keyboard path runs SQL, enters results and requests approval', async ({
  page,
}) => {
  await signIn(page);
  await console(page);
  const editor = page.getByRole('textbox', { name: 'SQL editor' });
  await editor.focus();
  await page.keyboard.press('Control+Enter');
  await page.getByRole('grid').waitFor();
  await editor.focus();
  for (let index = 0; index < 60; index++) {
    await page.keyboard.press('Tab');
    if (
      await page.evaluate(
        () =>
          document.activeElement instanceof HTMLElement &&
          !!document.activeElement.dataset.cell,
      )
    )
      break;
  }
  expect(
    await page.evaluate(
      () =>
        document.activeElement instanceof HTMLElement &&
        !!document.activeElement.dataset.cell,
    ),
  ).toBe(true);
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Cell value' })).toBeVisible();
  await page.keyboard.press('Escape');
  await editor.fill("UPDATE users SET name = 'Maya' WHERE id = 42");
  await page.getByText('Needs approval').waitFor();
  await editor.focus();
  await page.keyboard.press('Control+Enter');
  await page.getByRole('dialog', { name: 'Request query approval' }).waitFor();
  await page.getByLabel('Reason', { exact: true }).focus();
  await page.keyboard.type('Verified target rows for support ticket.');
  await page.keyboard.press('Tab');
  await page.keyboard.press('Tab');
  await page.keyboard.press('Enter');
  await expect(
    page.getByText('Approval requested', { exact: true }),
  ).toBeVisible();
});
test('reviewing the current policy completes setup from an audit event', async ({
  page,
}) => {
  await signIn(page);
  await go(page, `/clusters/${cluster}?tab=policy`);
  await page.getByRole('button', { name: 'Save reviewed policy' }).click();
  await page.getByRole('button', { name: 'Confirm', exact: true }).click();
  await expect(
    page.getByText('Safety policy saved', { exact: true }),
  ).toBeVisible();
  await go(page, '/overview');
  await expect(
    page.getByRole('region', { name: 'Workspace setup' }),
  ).toHaveCount(0);
  await expect(
    page.getByRole('region', { name: 'Workspace setup' }),
  ).toHaveCount(0);
  await go(page, '/clusters');
  await go(page, '/overview');
  await expect(
    page.getByRole('region', { name: 'Workspace setup' }),
  ).toHaveCount(0);
});
test('bulk grant revocation confirms scope and removes selected grants', async ({
  page,
}) => {
  await signIn(page);
  await go(page, '/access');
  await page
    .getByRole('checkbox', { name: 'Select all loaded grants' })
    .check();
  await page.getByRole('button', { name: 'Revoke selected' }).click();
  await expect(page.getByRole('dialog')).toContainText(
    'Each revocation is checked separately',
  );
  await page.getByRole('button', { name: 'Confirm', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: 'No explicit grants' }),
  ).toBeVisible();
});
