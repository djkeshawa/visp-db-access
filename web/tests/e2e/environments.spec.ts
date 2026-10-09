import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
const production = '00000000-0000-4000-8000-000000000100';
test.beforeEach(async ({ page }) => {
  await page.goto('/login');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.getByRole('heading', { name: 'Workspace overview' }).waitFor();
});
test('clusters default to environment groups and keyboard navigation', async ({
  page,
}) => {
  await page.getByRole('link', { name: 'Clusters', exact: true }).click();
  await expect(
    page.getByRole('table', { name: 'Production clusters' }),
  ).toBeVisible();
  const row = page.getByRole('row', {
    name: 'Open commerce-primary',
    exact: true,
  });
  await row.focus();
  await page.keyboard.press('Enter');
  await expect(
    page.getByRole('button', { name: 'Run on production' }),
  ).toBeVisible();
  await expect(page.locator('main')).toHaveAttribute(
    'data-environment',
    'production',
  );
  await page.getByRole('tab', { name: 'Policy', exact: true }).click();
  await expect(page.locator('main')).toHaveAttribute(
    'data-environment',
    'production',
  );
  await expect(page.locator('.production-label')).toHaveCount(0);
});
for (const width of [1440, 390])
  for (const theme of ['light', 'dark'] as const) {
    test(`risk identity stays accessible at ${width}px in ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: width === 1440 ? 900 : 844 });
      await page.getByRole('button', { name: 'User menu' }).click();
      await page
        .getByRole('menuitem', {
          name: theme === 'light' ? 'Light' : 'Dark',
          exact: false,
        })
        .click();
      await page.evaluate((id) => {
        history.pushState({}, '', `/clusters/${id}`);
        dispatchEvent(new PopStateEvent('popstate'));
      }, production);
      await page.getByText('Safe to run', { exact: true }).waitFor();
      await page.getByRole('button', { name: 'Run on production' }).click();
      await page.getByRole('grid', { name: 'Query results' }).waitFor();
      await expect(
        page.getByText('Query completed', { exact: true }),
      ).toHaveCount(0);
      await page.getByRole('button', { name: 'Export', exact: true }).click();
      await expect(
        page.getByRole('menuitem', { name: 'Download CSV' }),
      ).toBeVisible();
      await page.keyboard.press('Escape');
      const editor = await page.locator('.cm-scroller').boundingBox();
      expect(editor!.height).toBeGreaterThanOrEqual(126);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      await page
        .getByRole('textbox', { name: 'SQL editor' })
        .fill('DELETE FROM users');
      await page.getByText('Blocked', { exact: true }).waitFor();
      await expect(
        page.getByRole('button', { name: 'Run on production' }),
      ).toBeDisabled();
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    });
  }

test('every environment has a distinct frame and target action', async ({
  page,
}) => {
  for (const [suffix, environment, label, rail] of [
    ['100', 'production', 'Run on production', '8px'],
    ['102', 'staging', 'Run on staging', '4px'],
    ['103', 'development', 'Run', '2px'],
  ] as const) {
    await page.evaluate((suffix) => {
      history.pushState(
        {},
        '',
        `/clusters/00000000-0000-4000-8000-000000000${suffix}`,
      );
      dispatchEvent(new PopStateEvent('popstate'));
    }, suffix);
    await page.getByText('Safe to run', { exact: true }).waitFor();
    await expect(page.locator('main')).toHaveAttribute(
      'data-environment',
      environment,
    );
    expect(
      await page
        .locator('main')
        .evaluate((el) => getComputedStyle(el).borderTopWidth),
    ).toBe(rail);
    await expect(
      page.getByRole('button', { name: new RegExp(`^${label}`) }),
    ).toBeEnabled();
  }
});
