# visp-db-access web console

The React 19 + TypeScript console for visp-db-access. It talks to the server's
[v1 API](../docs/API.md) with same-origin session cookies and the
`X-Requested-With: vda` header. `npm run build` writes hashed assets to
`web/dist`, which the Rust server embeds and serves at `/` (the server returns
`index.html` for client routes such as `/clusters/:id`).

## Develop

Node.js 22.12 or newer:

```sh
npm ci
npm run dev        # proxies /api to a server on http://localhost:8080
npm run dev:mock   # in-memory demo API, no backend needed
```

Mock accounts are `admin@visp.dev`, `reviewer@visp.dev` and `member@visp.dev`,
password `demo-password`. The mock resets on a full page reload. Its SQL
classifier is a simplified simulation; the server always re-analyzes queries
and enforces policy, cost, permissions and approval at execution. Mock modules
load only when `VITE_MOCK=1` and are excluded from production builds.

See [Development](../docs/DEVELOPMENT.md) for running the full stack locally.

## Layout

| Path                 | Contents                                                                             |
| -------------------- | ------------------------------------------------------------------------------------ |
| `src/api/`           | Contract types (`types.ts`), HTTP client and errors, and the isolated mock transport |
| `src/features/`      | Overview, clusters, console, approvals, history, discovery, auth and admin pages     |
| `src/components/ui/` | Accessible primitives built on Radix ([guide](src/components/ui/README.md))          |
| `src/lib/`           | Querying, preferences, theming, time formatting, policy and result helpers           |
| `src/styles/`        | Design tokens and shared styles ([design system](../docs/DESIGN.md))                 |
| `tests/`             | Vitest unit tests and Playwright end-to-end tests (`tests/e2e/`)                     |
| `scripts/`           | Bundle budget check, doc sync, screenshot and audit tooling                          |

## Behavior worth knowing

- **Security-sensitive data is never optimistic.** Mutations aren't retried
  automatically, and success invalidates cached data. Run is enabled only by an
  analysis of the exact current SQL; each execution uses a fresh client ID that
  the cancel endpoint also receives. Leaving the console cancels a running query.
- **What stays in the browser.** Query drafts, favorites, open tabs and
  preferences are stored in `localStorage`, scoped to the signed-in user (and
  cluster for drafts). Results and credentials are never stored.
- **Results are a snapshot.** Sorting, filtering, pinning and hiding operate on
  fetched rows and never run more SQL. Copies and exports keep masked values
  masked; CSV exports escape formula-leading values.
- **Sessions.** An expired session prompts for sign-in without unloading the
  editor. When the gateway is unreachable, a banner retries a safe read every
  ten seconds; mutations are never replayed.
- **Keyboard.** ⌘/Ctrl+K opens the command palette, ⌘/Ctrl+Enter runs, Esc
  cancels, ⌘/Ctrl+Alt+T opens a query tab, and `?` lists all shortcuts.

## Checks

```sh
npm run typecheck && npm run lint && npm run format:check
npm test
npx playwright install chromium   # once
npm run test:e2e
npm run build
```

`npm run build` also refreshes the copies of the API and architecture docs
served under `/docs`, writes `dist/bundle-report.json`, and fails if the initial
JavaScript exceeds 250 KiB gzip or if the editor, formatter or grid leak into
non-console routes. The end-to-end suite includes an axe accessibility scan of
every route at desktop and phone sizes in both themes.
