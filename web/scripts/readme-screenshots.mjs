// Regenerates the README screenshots in ../docs/images from the mock UI.
// Usage: npm run dev:mock -- --host 127.0.0.1 --port 5179 --strictPort
//        node scripts/readme-screenshots.mjs
import { chromium } from '@playwright/test';
import { mkdir } from 'node:fs/promises';

const base = process.env.SCREENSHOT_URL ?? 'http://127.0.0.1:5179';
const out = '../docs/images';
const production = '00000000-0000-4000-8000-000000000100';

await mkdir(out, { recursive: true });
const browser = await chromium.launch();

async function session(theme) {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
    colorScheme: theme,
  });
  await page.goto(`${base}/login`);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.waitForURL((url) => url.pathname !== '/login');
  return page;
}

async function go(page, path) {
  await page.evaluate((path) => {
    history.pushState({}, '', path);
    dispatchEvent(new PopStateEvent('popstate'));
  }, path);
}

async function shot(page, name) {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${out}/${name}.png` });
}

const light = await session('light');
await go(light, '/overview');
await light.getByText('Needs you').waitFor();
await shot(light, 'overview');
await go(light, '/clusters');
await light.getByRole('heading', { name: 'Clusters' }).waitFor();
await shot(light, 'clusters');
await go(light, `/clusters/${production}?tab=policy`);
await light.getByText('Read limits').waitFor();
await shot(light, 'policy');

const dark = await session('dark');
await go(dark, `/clusters/${production}`);
await dark.getByText('Safe to run', { exact: true }).waitFor();
await dark.getByRole('button', { name: /^Run on production/ }).click();
await dark.getByRole('grid').waitFor();
await shot(dark, 'console');
await go(dark, '/approvals');
await dark.getByRole('button', { name: 'Review', exact: true }).first().click();
await dark.getByRole('region', { name: 'Query approval' }).waitFor();
await shot(dark, 'approvals');

await browser.close();
console.log(`Screenshots written to ${out}`);
