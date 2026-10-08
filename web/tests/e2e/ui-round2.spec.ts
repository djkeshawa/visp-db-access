import { test, expect } from '@playwright/test';
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
}
test('desktop console keeps editor and results inside a fixed viewport', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await go(page, `/clusters/${cluster}`);
  await page.getByText('Safe to run', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Run', exact: false }).click();
  await page.getByRole('grid').waitFor();
  const bounds = await page.locator('.results').boundingBox();
  expect(bounds!.y).toBeLessThan(600);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(900);
  await expect(page.locator('.app-footer')).toHaveCount(0);
  expect(
    await page
      .locator('main.page')
      .evaluate((el) => el.scrollHeight <= el.clientHeight),
  ).toBe(true);
});
test('mobile approval sheet keeps decisions visible while SQL scrolls', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await go(page, '/approvals');
  await page
    .getByRole('button', { name: 'Review', exact: true })
    .first()
    .click();
  await page.getByRole('button', { name: 'Approve', exact: true }).waitFor();
  const dialog = await page.getByRole('dialog').boundingBox(),
    approve = await page
      .getByRole('button', { name: 'Approve', exact: true })
      .boundingBox();
  expect(dialog!.x).toBe(0);
  expect(dialog!.y).toBe(0);
  expect(dialog!.width).toBe(390);
  expect(dialog!.height).toBe(844);
  expect(approve!.y + approve!.height).toBeLessThanOrEqual(844);
  expect((await page.locator('.skip-link').boundingBox())!.x).toBeLessThan(0);
  const before = await page
    .getByRole('button', { name: 'Approve', exact: true })
    .boundingBox();
  await page.locator('.approval-scroll').evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  expect(
    (await page
      .getByRole('button', { name: 'Approve', exact: true })
      .boundingBox())!.y,
  ).toBe(before!.y);
});

test('schema row actions and context menu insert safe qualified SQL', async ({
  page,
}) => {
  await signIn(page);
  await go(page, `/clusters/${cluster}`);
  const row = page
    .locator('.schema-table > summary')
    .filter({ hasText: 'users' });
  await row.click({ button: 'right' });
  await page
    .getByRole('menuitem', { name: 'SELECT top 100', exact: true })
    .click();
  await expect(page.getByRole('textbox', { name: 'SQL editor' })).toContainText(
    '`commerce`.`users` LIMIT 100;',
  );
  await row.focus();
  await page.keyboard.press('Shift+F10');
  await expect(
    page.getByRole('menuitem', { name: 'Insert name' }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await row.hover();
  await page.getByRole('button', { name: 'Insert users', exact: true }).click();
  await expect(page.getByRole('textbox', { name: 'SQL editor' })).toContainText(
    'LIMIT 100; `commerce`.`users`',
  );
});
test('skip link is shown only for keyboard navigation', async ({ page }) => {
  await signIn(page);
  const link = page.getByRole('link', { name: 'Skip to content' });
  expect((await link.boundingBox())!.x).toBeLessThan(0);
  await page.keyboard.press('Tab');
  await expect(link).toBeFocused();
  expect((await link.boundingBox())!.x).toBeGreaterThanOrEqual(0);
  await page.keyboard.press('Enter');
  await expect(page.locator('#main-content')).toBeFocused();
});
test('phone console keeps results visible and opens safety on demand', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await go(page, `/clusters/${cluster}`);
  await page.getByText('Safe to run', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Run', exact: false }).click();
  await page.getByRole('grid').waitFor();
  const result = await page.locator('.results').boundingBox();
  expect(result!.y).toBeLessThan(600);
  expect(result!.y + result!.height).toBeLessThanOrEqual(844);
  expect(
    await page
      .locator('main.page')
      .evaluate((el) => el.scrollHeight <= el.clientHeight),
  ).toBe(true);
  const viewport = await page.locator('.result-scroll').boundingBox();
  const row = await page.locator('.result-row').first().boundingBox();
  expect(row!.y + row!.height).toBeLessThanOrEqual(
    viewport!.y + viewport!.height,
  );
  await page
    .locator('.cell-value')
    .filter({ hasText: '"plan"' })
    .first()
    .click();
  await expect(page.getByRole('dialog', { name: 'JSON value' })).toBeVisible();
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Open safety analysis' }).click();
  await expect(
    page.getByRole('complementary', { name: 'Safety panel', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Close safety panel' }).click();
  await expect(
    page.getByRole('complementary', { name: 'Safety panel', exact: true }),
  ).toHaveCount(0);
});
for (const width of [1440, 390]) {
  test(`long preferences scroll only inside main at ${width}px`, async ({
    page,
  }) => {
    const height = width === 1440 ? 900 : 844;
    await page.setViewportSize({ width, height });
    await signIn(page);
    await go(page, '/settings?tab=preferences');
    await page.getByRole('heading', { name: 'Your settings' }).waitFor();
    expect(
      await page.evaluate(() => document.documentElement.scrollHeight),
    ).toBe(height);
    expect(
      await page
        .locator('main.page')
        .evaluate((el) => el.scrollHeight > el.clientHeight),
    ).toBe(true);
    await page.locator('main.page').evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    expect((await page.locator('.topbar').boundingBox())!.y).toBe(0);
    expect(
      await page.evaluate(() => document.documentElement.scrollHeight),
    ).toBe(height);
  });
}
