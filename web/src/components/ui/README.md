# Visp design system

Use `index.tsx` primitives for actions, fields, selects, tabs, dialogs,
confirmations, badges, tooltips and list feedback. Radix handles focus containment
and keyboard behavior. `toast.tsx` supplies error and background-action feedback;
`brand.tsx` owns the gateway mark. Preview at `/ui?kitchen-sink` as an admin in
development; the production route is unavailable.

`styles/tokens.css` owns all shared tokens. Atkinson Hyperlegible Next is the UI
font; Atkinson Hyperlegible Mono is reserved for SQL, hosts, IDs and numeric table
cells. The scale is 12 / 13 / 14 / 16 / 20 / 26px. Controls use 6px radii, panels
8px and dialogs 12px. Only floating layers have shadows. Focus uses a 2px ink
outline with a 2px offset. Selection is neutral gray.

Primary actions are ink. Color denotes environments and SQL guard verdicts.
`Badge` exposes neutral / env / verdict / status variants; status text is
capitalized and neutral, with explicit words. `EnvBadge` delegates to `Badge`.
See `docs/DESIGN.md` for color pairs, contrast measurements and the environment
frame. Production has a striped 6px rail, staging a solid 4px rail and development
a muted teal 2px rail. On phones the rail becomes a top band.

The app occupies the viewport; main content scrolls independently. Console routes
use a bounded editor/results split and collapsible schema/safety columns. Standard
buttons are 36px tall; mobile approval actions are 40px. Use sentence case and
explicit action names, preserve API error messages and offer retries for failed
reads. Destructive confirmations state their scope.
