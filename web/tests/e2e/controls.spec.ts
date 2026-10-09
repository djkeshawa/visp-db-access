import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
const cluster = '00000000-0000-4000-8000-000000000100';
test.beforeEach(async ({ page }) => {
  await page.goto('/login');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.getByRole('heading', { name: 'Workspace overview' }).waitFor();
});
async function go(page: import('@playwright/test').Page, path: string) {
  await page.evaluate((path) => {
    history.pushState({}, '', path);
    dispatchEvent(new PopStateEvent('popstate'));
  }, path);
}
test('policy converts fractional seconds to milliseconds and resets a customized row', async ({
  page,
}) => {
  await go(page, `/clusters/${cluster}?tab=policy`);
  const timeout = page.getByLabel('Statement timeout', { exact: true });
  await timeout.fill('15.125');
  await page
    .getByRole('button', { name: 'Review changes', exact: true })
    .click();
  await page.getByRole('button', { name: 'Confirm', exact: true }).click();
  await expect(
    page.getByText('Safety policy saved', { exact: true }),
  ).toBeVisible();
  const saved = await page.evaluate(async (path) => {
    const modulePath = '/src/api/client.ts';
    const api = await import(modulePath);
    return api.request(path);
  }, `/clusters/${cluster}/policy`);
  expect(saved.statement_timeout_ms).toBe(15125);
  await expect(timeout).toHaveValue('15.125');
  const rows = page.getByLabel('Maximum result rows', { exact: true });
  await rows.fill('250');
  await page
    .getByRole('button', {
      name: 'Reset Maximum result rows to default',
      exact: true,
    })
    .click();
  await expect(rows).toHaveValue('1000');
  await expect(page.locator('.save-bar')).toHaveCount(0);
  const writes = page.getByRole('switch', {
    name: 'Allow writes',
    exact: true,
  });
  await writes.focus();
  await page.keyboard.press('Space');
  await expect(writes).toBeChecked();
  await expect(page.locator('.save-bar')).toBeVisible();
});
test('desktop approval inbox becomes a phone sheet and retains the review note', async ({
  page,
}) => {
  await go(page, '/approvals');
  await page
    .getByRole('button', { name: 'Review', exact: true })
    .first()
    .click();
  await expect(
    page.getByRole('region', { name: 'Query approval' }),
  ).toBeVisible();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.getByLabel('Review note').fill('Verified target rows');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(
    page.getByRole('dialog', { name: 'Query approval' }),
  ).toBeVisible();
  await expect(page.getByLabel('Review note')).toHaveValue(
    'Verified target rows',
  );
  const action = await page
    .getByRole('button', { name: 'Approve', exact: true })
    .boundingBox();
  expect(action!.y + action!.height).toBeLessThanOrEqual(844);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});
test('date presets update both URL boundaries atomically', async ({ page }) => {
  await go(page, '/history');
  await page.getByRole('button', { name: 'Date range', exact: true }).click();
  await page.getByRole('button', { name: 'Last 7 days', exact: true }).click();
  const params = new URL(page.url()).searchParams;
  expect(params.get('from')).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  expect(params.get('to')).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  expect(Date.parse(params.get('to')!) - Date.parse(params.get('from')!)).toBe(
    6 * 86400000,
  );
  await expect(
    page.getByText('UTC date range', { exact: false }),
  ).toBeVisible();
});
test('segmented preferences respond to arrow keys and persist in this browser', async ({
  page,
}) => {
  await go(page, '/settings?tab=preferences');
  const density = page.getByRole('group', { name: 'Density', exact: true });
  await density
    .getByRole('button', { name: 'Comfortable', exact: true })
    .focus();
  await page.keyboard.press('ArrowRight');
  await expect(
    density.getByRole('button', { name: 'Compact', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  // Mock sessions live in memory, so a reload signs out; the preference must
  // survive in this browser's storage regardless.
  await page.reload();
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.waitForURL((url) => url.pathname !== '/login');
  await go(page, '/settings?tab=preferences');
  await expect(
    page
      .getByRole('group', { name: 'Density', exact: true })
      .getByRole('button', { name: 'Compact', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
});
