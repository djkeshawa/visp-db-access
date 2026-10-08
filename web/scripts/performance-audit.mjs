import { chromium } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';

// Local lab observations, without CPU/network throttling; this is not a Lighthouse score.
const base = process.env.AUDIT_URL ?? 'http://127.0.0.1:5177';
const output = 'screenshots/audit/after/performance.json';
const browser = await chromium.launch(),
  samples = [];
for (const width of [1440, 390])
  for (let run = 1; run <= 3; run++) {
    const context = await browser.newContext({
      viewport: { width, height: width === 1440 ? 900 : 844 },
    });
    const page = await context.newPage();
    await page.addInitScript(() => {
      window.__uiPerformance = { lcp: 0, cls: 0 };
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries())
          window.__uiPerformance.lcp = entry.startTime;
      }).observe({ type: 'largest-contentful-paint', buffered: true });
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries())
          if (!entry.hadRecentInput) window.__uiPerformance.cls += entry.value;
      }).observe({ type: 'layout-shift', buffered: true });
    });
    const files = new Set();
    page.on('response', (response) => {
      if (new URL(response.url()).pathname.endsWith('.js'))
        files.add(new URL(response.url()).pathname);
    });
    await page.goto(`${base}/login`);
    await page.getByRole('button', { name: 'Sign in', exact: true }).waitFor();
    await page.waitForTimeout(1000);
    const navigation = await page.evaluate(() => ({
      fcp_ms: performance.getEntriesByName('first-contentful-paint')[0]
        ?.startTime,
      lcp_ms: window.__uiPerformance.lcp,
      cls: window.__uiPerformance.cls,
    }));
    const start = Date.now();
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByRole('heading', { name: 'Workspace overview' }).waitFor();
    const overviewMs = Date.now() - start;
    await page.waitForTimeout(400);
    const overviewFiles = [...files];
    if (overviewFiles.some((file) => /\/(?:editor|formatter|grid)-/.test(file)))
      throw new Error(
        'A console dependency loaded before visiting the console.',
      );
    const consoleStart = Date.now();
    await page.evaluate(() => {
      history.pushState(
        {},
        '',
        '/console?cluster_id=00000000-0000-4000-8000-000000000100',
      );
      dispatchEvent(new PopStateEvent('popstate'));
    });
    await page.getByText('Safe to run', { exact: true }).waitFor();
    samples.push({
      width,
      run,
      ...navigation,
      login_and_overview_ms: overviewMs,
      console_ready_ms: Date.now() - consoleStart,
      overview_js: overviewFiles,
      console_added_js: [...files].filter(
        (file) => !overviewFiles.includes(file),
      ),
    });
    await context.close();
  }
await browser.close();
await mkdir('screenshots/audit/after', { recursive: true });
await writeFile(
  output,
  JSON.stringify(
    {
      environment:
        'Headless Chromium, local Vite production preview with mock API, cold contexts, no throttling; three samples per viewport. Includes real mock delays; no Lighthouse score.',
      samples,
    },
    null,
    2,
  ),
);
console.log(`Saved ${samples.length} performance samples to ${output}`);
