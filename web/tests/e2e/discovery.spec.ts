import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/login');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: 'Workspace overview' }),
  ).toBeVisible();
});
test('add source, scan, verify credentials, and import into a cluster', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.getByRole('link', { name: 'Settings', exact: true }).click();
  await page.getByRole('tab', { name: 'Cloud discovery', exact: true }).click();
  await page.getByRole('button', { name: 'Add source', exact: true }).click();
  await page
    .getByLabel('Source name', { exact: true })
    .fill('E2E discovery account');
  await page.getByLabel('Search AWS regions').fill('Sydney');
  await page.getByRole('checkbox', { name: 'Sydney ap-southeast-2' }).check();
  await page.getByRole('button', { name: 'Remove region us-east-1' }).click();
  await page
    .getByRole('combobox', { name: 'Default project', exact: true })
    .click();
  await page.getByRole('option', { name: 'Commerce', exact: true }).click();
  await page.getByText('Required IAM permissions', { exact: true }).click();
  await expect(
    page.getByText('rds:DescribeDBClusters', { exact: false }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Test connection' }).click();
  await expect(
    page.getByText('Connection verified', { exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Save source' }).click();
  const source = page
    .getByRole('row')
    .filter({ hasText: 'E2E discovery account' });
  await source.getByRole('button', { name: 'Test', exact: true }).click();
  await expect(
    page.getByText('Connection verified', { exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Close dialog' }).click();
  await source.getByRole('button', { name: 'Scan now' }).click();
  await expect(source).toContainText('3 found');
  await page.getByRole('link', { name: 'View discovered databases →' }).click();
  await page.getByRole('combobox', { name: 'Discovery source' }).click();
  await page
    .getByRole('option', { name: 'E2E discovery account', exact: true })
    .click();
  const row = page
    .getByRole('row')
    .filter({ has: page.getByText('sandbox-postgres', { exact: true }) });
  await row.getByRole('button', { name: 'Import', exact: true }).click();
  await expect(
    page.getByRole('combobox', { name: 'Project', exact: true }),
  ).toContainText('Commerce');
  await page.getByRole('button', { name: 'Create project inline' }).click();
  await page
    .getByLabel('New project name', { exact: true })
    .fill('Imported AWS services');
  await page
    .getByRole('button', { name: 'Create project', exact: true })
    .click();
  await expect(
    page.getByRole('combobox', { name: 'Project', exact: true }),
  ).toContainText('Imported AWS services');
  await page
    .getByLabel('Cluster name', { exact: true })
    .fill('e2e-imported-db');
  await page.getByLabel('Username', { exact: true }).fill('vda_gateway');
  await page.getByLabel('Password', { exact: true }).fill('database-secret');
  await expect(
    page.getByRole('button', { name: 'Import database' }),
  ).toBeDisabled();
  await page
    .getByRole('button', { name: 'Test connection', exact: true })
    .click();
  await expect(
    page.getByText('Connection verified. Ready to import.'),
  ).toBeVisible();
  await page
    .getByLabel('Password', { exact: true })
    .fill('changed-database-secret');
  await expect(
    page.getByRole('button', { name: 'Import database' }),
  ).toBeDisabled();
  await page
    .getByRole('button', { name: 'Test connection', exact: true })
    .click();
  await expect(
    page.getByText('Connection verified. Ready to import.'),
  ).toBeVisible();

  await page.screenshot({
    path: 'screenshots/discovery-import.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.getByRole('button', { name: 'Import database' }).click();
  await expect(page).toHaveURL(/\/clusters\/[^/?]+$/);
  await expect(
    page.getByRole('heading', { name: 'e2e-imported-db', exact: true }),
  ).toBeVisible();
  expect(errors).toEqual([]);
});
test('discovery screens, themes, history and drift review', async ({
  page,
}) => {
  await page.getByRole('link', { name: 'Settings', exact: true }).click();
  await page.getByRole('tab', { name: 'Cloud discovery', exact: true }).click();
  await expect(page.getByText('APAC sandbox', { exact: true })).toBeVisible();
  await page.screenshot({
    path: 'screenshots/discovery-sources.png',
    fullPage: true,
    animations: 'disabled',
  });
  const source = page
    .getByRole('row')
    .filter({ hasText: 'Production account' });
  await source.getByRole('button', { name: 'Edit', exact: true }).click();
  await page.getByText('Required IAM permissions', { exact: true }).click();
  await page.screenshot({
    path: 'screenshots/discovery-source-dialog.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.getByRole('button', { name: 'Close dialog' }).click();
  await source.getByRole('button', { name: 'History', exact: true }).click();
  await expect(page.getByRole('dialog')).toContainText('Succeeded');
  await page.screenshot({
    path: 'screenshots/discovery-history.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.getByRole('button', { name: 'Close dialog' }).click();
  await page.getByRole('link', { name: 'View discovered databases →' }).click();
  await expect(
    page.getByText('Publicly accessible', { exact: false }),
  ).toBeVisible();
  await expect(page.getByText('Unencrypted', { exact: true })).toBeVisible();
  await page.screenshot({
    path: 'screenshots/discovered-light.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.getByRole('button', { name: 'User menu' }).click();
  await page.getByRole('menuitem', { name: 'Dark', exact: false }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.screenshot({
    path: 'screenshots/discovered-dark.png',
    fullPage: true,
    animations: 'disabled',
  });
  const imported = page
    .getByRole('row')
    .filter({ hasText: 'commerce-primary' });
  await imported.getByRole('link').click();
  await expect(
    page.getByRole('button', { name: 'Review AWS changes' }),
  ).toBeVisible();
  await page.screenshot({
    path: 'screenshots/discovery-cluster-banner.png',
    animations: 'disabled',
  });
  await page.getByRole('button', { name: 'Review AWS changes' }).click();
  await expect(
    page.getByRole('button', { name: 'Apply', exact: true }),
  ).toBeDisabled();
  await page.screenshot({
    path: 'screenshots/discovery-drift.png',
    fullPage: false,
    animations: 'disabled',
  });
  await page
    .getByLabel('Password', { exact: true })
    .fill('new-endpoint-secret');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  await expect(
    imported.getByText('Endpoint changed', { exact: true }),
  ).toHaveCount(0);
  await page.setViewportSize({ width: 768, height: 1024 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});
test('bulk ignore, restore and member discovery visibility', async ({
  page,
}) => {
  await page.getByRole('link', { name: 'Discovery', exact: false }).click();
  await page
    .getByRole('checkbox', { name: 'Select all new resources on this page' })
    .check();
  await page.getByRole('button', { name: 'Ignore selected' }).click();
  await expect(
    page.getByText('6 resources ignored', { exact: true }),
  ).toBeVisible();
  const row = page
    .getByRole('row')
    .filter({ has: page.getByText('sandbox-postgres', { exact: true }) });
  await row.getByRole('button', { name: 'Unignore' }).click();
  await expect(
    row.getByRole('button', { name: 'Import', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'User menu' }).click();
  await page.getByRole('menuitem', { name: 'Log out' }).click();
  await page.getByLabel('Email', { exact: true }).fill('member@visp.dev');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(
    page.getByRole('link', { name: 'Discovery', exact: false }),
  ).toHaveCount(0);
  await page.getByRole('link', { name: 'Clusters', exact: true }).click();
  await expect(
    page.getByRole('tab', { name: 'Discovered', exact: false }),
  ).toHaveCount(0);
});
