import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { format, resolveConfig } from 'prettier';

// Publish owned copies without changing the authoritative contract files.
await mkdir('public/docs', { recursive: true });
for (const [source, target] of [
  ['API.md', 'api.md'],
  ['ARCHITECTURE.md', 'architecture.md'],
]) {
  const content = await readFile(`../docs/${source}`, 'utf8');
  const path = `public/docs/${target}`;
  const formatted = await format(content, {
    ...(await resolveConfig(path)),
    filepath: path,
  });
  const existing = await readFile(path, 'utf8').catch(() => null);
  if (existing !== formatted) await writeFile(path, formatted);
}
