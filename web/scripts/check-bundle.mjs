import { readFile, writeFile } from 'node:fs/promises';
import { gzipSync } from 'node:zlib';
const manifest = JSON.parse(await readFile('dist/.vite/manifest.json', 'utf8'));
const entry = Object.keys(manifest).find((key) => manifest[key].isEntry);
async function graph(key, seen = new Set()) {
  if (seen.has(key)) return seen;
  seen.add(key);
  for (const imported of manifest[key]?.imports ?? [])
    await graph(imported, seen);
  return seen;
}
async function size(keys) {
  let total = 0;
  for (const key of keys) {
    const file = manifest[key]?.file;
    if (file?.endsWith('.js'))
      total += gzipSync(await readFile(`dist/${file}`)).length;
  }
  return total;
}
const shared = await graph(entry),
  bytes = await size(shared),
  routes = [];
const routeEntries = [
  ['/overview', 'overview'],
  ['/clusters', 'list'],
  ['/clusters/:id?tab=schema|health|access|policy|settings', 'detail'],
  ['/console', 'console'],
  ['/approvals', 'approvals'],
  ['/history', 'history'],
  ['/users', 'users'],
  ['/access', 'access'],
  ['/audit', 'audit'],
  ['/settings', 'settings'],
];
for (const [route, name] of routeEntries) {
  const key = Object.keys(manifest).find((key) => manifest[key].name === name);
  if (!key) throw new Error(`Route chunk missing: ${name}`);
  const dependencies = await graph(key);
  const additional = await size(
    [...dependencies].filter((key) => !shared.has(key)),
  );
  if (
    name !== 'console' &&
    [...dependencies].some((key) =>
      /\/(?:editor|formatter|grid)-/.test(manifest[key].file),
    )
  )
    throw new Error(`Console-only dependencies load on ${route}`);
  routes.push({
    route,
    additional_gzip_kb: Number((additional / 1024).toFixed(2)),
    total_gzip_kb: Number(((bytes + additional) / 1024).toFixed(2)),
  });
}
const report = {
  entry_gzip_kb: Number((bytes / 1024).toFixed(2)),
  budget_gzip_kb: 250,
  entry_files: [...shared].map((key) => manifest[key].file),
  routes,
};
await writeFile('dist/bundle-report.json', JSON.stringify(report, null, 2));
console.log(
  `Initial JavaScript: ${report.entry_gzip_kb} KiB gzip / 250 KiB budget`,
);
if (bytes > 250 * 1024)
  throw new Error('Initial bundle exceeds the 250 KiB gzip budget.');
if (
  report.entry_files.some((file) => /\/(?:editor|formatter|grid)-/.test(file))
)
  throw new Error('Console-only dependencies are eagerly loaded.');
