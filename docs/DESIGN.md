# Design system

The console's visual language is **risk is the only color**. Its audience is
engineers, DBAs and security administrators querying production databases, so
the interface stays near-monochrome and color is reserved for what carries
risk: the environment you're in and the guard's verdict. Primary actions,
navigation, links, selection and focus are ink. Environments and verdicts share
one safer → riskier ladder (teal, amber, red), and words and shapes always carry
the meaning alongside color. Health is neutral when healthy; Degraded (amber
square) and Down (red hollow dot, red word) take risk color because an
unavailable database is a risk.

The styling follows the restraint of native Apple interfaces: system fonts,
space instead of lines, quiet controls, translucent materials only on floating
layers, and motion only in response to the user.

Implementation: tokens in `web/src/styles/tokens.css`, shared rules in
`web/src/styles/refinement.css`, primitives in `web/src/components/ui/`
(previewed at `/ui?kitchen-sink` in development builds).

## Tokens and typography

`web/src/styles/tokens.css` is the single foundation source. Existing `--bg`,
`--text`, `--muted`, `--border` and semantic status names are role aliases, not
independent palettes. There are no indigo or `--accent*` tokens.

| Role                   | Light     | Dark      |
| ---------------------- | --------- | --------- |
| Paper                  | `#F3F4F6` | `#15171B` |
| Surface                | `#FFFFFF` | `#1C1F24` |
| Ink                    | `#1A1D23` | `#E8EAED` |
| Label secondary        | ink 70%   | ink 70%   |
| Rule (hairline)        | ink 10%   | ink 12%   |
| Subtle                 | `#ECEEF1` | `#282D34` |
| Production             | `#C8242F` | `#F47780` |
| Production soft        | `#FCECEE` | `#382328` |
| Staging                | `#B7791F` | `#E6B65C` |
| Staging soft           | `#FCF1DD` | `#352D20` |
| Development            | `#2F7D6D` | `#79C6B2` |
| Development soft       | `#E7F3EF` | `#20332E` |
| Staging small text     | `#855610` | `#E6B65C` |
| Development small text | `#286E60` | `#79C6B2` |
| Development rail       | `#A5C8BE` | `#3B6258` |
| Selection              | `#DCE0E5` | `#414954` |
| On ink                 | `#FFFFFF` | `#15171B` |

Amber is too light for small text on white, and the base teal falls just below
AA on paper, so companion _text_ tokens darken those families while the bands
keep the base colors. In dark mode the production text and band are lifted for
contrast, but the production Run button keeps `#C8242F` with white text in both
themes, since white on the lifted pink would fail.

### Type

Fonts are Apple-native. The UI stack is `-apple-system, BlinkMacSystemFont,
'Inter Variable', 'Inter', 'Segoe UI', system-ui`: Apple devices render SF Pro
from the system, everything else renders bundled Inter (variable, with optical
sizes). Mono is `ui-monospace, 'SF Mono', 'JetBrains Mono Variable', …`. SF is
never bundled or served: Apple's license restricts SF Pro to interfaces of apps
running on Apple platforms, and the system font keywords are the standards-based
way to use it on those platforms only.

Identifiers, hosts and IDs in the UI face use a separate subset, `Inter
Identifiers` (`web/src/assets/fonts/`, SIL OFL), because Fontsource's Latin
subsets omit Inter's `ss02` Disambiguation set and `zero`. With those features
0/O and l/I/1 are unambiguous — misreading a production identifier is a real
risk. The subset keeps `kern`; without it, pairs around hyphens drift apart.
Mono is only for SQL, hosts, IDs and numeric table cells. Numbers that line up
(counts, timers, durations, table numerals) use tabular figures.

Scale: 12 caption · 13 secondary/data · **14 body** · 17 section title ·
22 page title · 28 overview greeting. Two weights, 400 and 600. Negative
tracking only at ≥ 20px (−0.015em); SF applies its own optical tracking. Text
hierarchy uses one ink at four strengths — label primary 100%, secondary 70%,
tertiary 38%, quaternary 18% — instead of separate greys. Headings use
`text-wrap: balance`, paragraphs `pretty`. Sentence case throughout.

### Layout, controls and motion

- **Space over lines.** At most one bordered container per page. The console
  is one surface; schema, editor, results and safety are separated by hairlines.
  Hairlines are ink at 10% (12% dark), 0.5px on 2× displays. Tables have no
  vertical rules, an unboxed 12px header and quaternary-fill row hover.
- **Quiet controls.** Buttons are 28 / 32 / 44px (phone primary); one primary
  per view. Icon buttons are borderless with a hover fill and a tooltip.
  `SegmentedControl` (equal segments as wide as the widest label, sliding
  thumb, arrow-key navigation) replaces view/filter tab rows; `Switch` replaces
  checkboxes for settings; checkboxes remain only for row selection. Inputs
  are filled, borderless until hover/focus. Badges are text on a soft fill.
  Lucide icons at 16px, stroke 1.5.
- **Materials.** Sidebar, top bar, menus, popovers, tooltips, toasts and the
  command palette use a translucent material (`saturate(180%) blur(20px)` over
  80% surface) with an opaque fallback for no support and for
  `prefers-reduced-transparency`. Dialogs and sheets are opaque. Only floating
  layers have shadows (0.5px edge + 2/6px + 12/32px soft).
- **Radii are concentric.** Dialog 14, panel 10, control 7; a nested shape uses
  its parent's radius minus the padding between them.
- **Motion.** 120 / 200 / 320ms; sheets ease `cubic-bezier(0.32, 0.72, 0, 1)`,
  popovers `cubic-bezier(0.2, 0, 0, 1)` and grow from their trigger. Dialogs
  fade and scale from 0.98; segmented thumbs slide. Nothing animates on load or
  loops; reduced motion leaves fades only.
- **Focus.** A soft 3px ring of ink (64% light, 48% dark), visible at ≥ 3:1.
  Dialogs focus themselves on open, not their close button, so the close
  tooltip doesn't appear and the first Escape closes the dialog.
- **Preferences apply and persist immediately** in this browser; there is no
  Save step.

## Environment frame

Everything right of the sidebar and below the top bar, on every cluster tab and
the selected-cluster console, carries one band across its top edge:

- Production: an 8px band of static −45° red/soft-red diagonal stripes (6px on
  phones).
- Staging: a 4px solid amber band.
- Development: a 2px muted teal line.

Width and pattern encode risk independently of hue. Nothing animates. Clusters
inventory group headings repeat the treatment as a small swatch. The compact
header has the cluster name, environment word, health/latency, mono host (hidden
on phones; available in Settings and on hover), access and an inline drift link.

Run actions name their target: `Run on production` (red, white text), `Run on
staging` (amber family) and `Run` (ink) in development. Approval-required writes
keep the explicit Request approval action; blocked SQL disables execution. One
Export menu holds CSV/JSON downloads and clipboard formats. Completed results are
their own confirmation (no toast). Phone editors keep at least six visible lines
and the results status line switches to short forms so the first rows stay in
view.

## Pages

- **Overview** opens with a greeting and one line of counts, then _Needs you_:
  approvals waiting for my review, my pending requests and clusters down or
  degraded ("You're all caught up" when empty). Then recent queries
  (Mine/Everyone). The setup checklist is compact, dismissible and hidden once
  complete.
- **Clusters** default to dense rows grouped Production, Staging, Development:
  name/engine monogram, health, truncated host, project, your access, last
  queried. Rows are keyboard navigable. Card view remains available.
- **Cluster tabs** (schema, health, access, settings) use grouped-list layouts.
- **Policy** starts with a plain summary ("Reads return at most 1,000 rows.
  Writes are blocked. DDL is blocked."), then grouped rows of label, one-line
  explanation and control with units (seconds in the UI, milliseconds to the
  API). Rows that differ from the environment default show _Customized_ and
  _Reset to default_; a save bar appears only with unsaved changes.
- **Approvals** is a two-pane inbox on desktop with segmented filters and counts;
  phones open the request as a full-screen sheet with sticky actions. Counts
  cover loaded requests, and the page says so.
- **History and Audit** group rows under day headings; each row shows the verb,
  tables and a syntax-tinted SQL preview. A denied query shows _Blocked_ once.
  The date-range popover offers Today, Last 7 days, Last 30 days and Custom.
- **Users and Access** use list rows with quiet badges; destructive actions live
  in a row menu with confirmation; _Added by_ shows a person or "—".
- **Settings, Preferences, Login, command palette, dialogs, toasts, empty states,
  skeletons and errors** all use the shared primitives.

## Contrast measurements

Measured from rendered screens (lowest first per theme). Every pair meets WCAG AA.

| Theme | Text      | Background | Ratio   | AA   |
| ----- | --------- | ---------- | ------- | ---- |
| Dark  | `#F47780` | `#382328`  | 5.41:1  | Pass |
| Dark  | `#FFFFFF` | `#C8242F`  | 5.60:1  | Pass |
| Dark  | `#F47780` | `#1C1F24`  | 6.13:1  | Pass |
| Dark  | `#AEB1B6` | `#282D34`  | 6.44:1  | Pass |
| Dark  | `#AEB1B4` | `#282B30`  | 6.59:1  | Pass |
| Dark  | `#F47780` | `#15171B`  | 6.66:1  | Pass |
| Dark  | `#79C6B2` | `#20332E`  | 6.69:1  | Pass |
| Dark  | `#ABADB1` | `#1C1F24`  | 7.35:1  | Pass |
| Dark  | `#ABACB0` | `#1B1D22`  | 7.43:1  | Pass |
| Dark  | `#E8EAED` | `#414954`  | 7.55:1  | Pass |
| Dark  | `#A9ABAE` | `#15171B`  | 7.79:1  | Pass |
| Dark  | `#79C6B2` | `#1C1F24`  | 8.28:1  | Pass |
| Dark  | `#E6B65C` | `#1C1F24`  | 8.82:1  | Pass |
| Dark  | `#79C6B2` | `#15171B`  | 9.00:1  | Pass |
| Dark  | `#E6B65C` | `#15171B`  | 9.57:1  | Pass |
| Dark  | `#E8EAED` | `#2C2F34`  | 11.14:1 | Pass |
| Dark  | `#E8EAED` | `#282D34`  | 11.49:1 | Pass |
| Dark  | `#E8EAED` | `#282B30`  | 11.78:1 | Pass |
| Dark  | `#15171B` | `#D0D4DA`  | 12.05:1 | Pass |
| Dark  | `#E8EAED` | `#1C1F24`  | 13.70:1 | Pass |
| Dark  | `#E8EAED` | `#1B1D22`  | 13.98:1 | Pass |
| Dark  | `#15171B` | `#E8EAED`  | 14.88:1 | Pass |
| Dark  | `#E8EAED` | `#15171B`  | 14.88:1 | Pass |
| Light | `#C8242F` | `#FCECEE`  | 4.90:1  | Pass |
| Light | `#C8242F` | `#F3F4F6`  | 5.08:1  | Pass |
| Light | `#286E60` | `#E7F3EF`  | 5.29:1  | Pass |
| Light | `#286E60` | `#F3F4F6`  | 5.46:1  | Pass |
| Light | `#C8242F` | `#FFFFFF`  | 5.60:1  | Pass |
| Light | `#FFFFFF` | `#C8242F`  | 5.60:1  | Pass |
| Light | `#855610` | `#F3F4F6`  | 5.72:1  | Pass |
| Light | `#595C61` | `#ECEEF1`  | 5.77:1  | Pass |
| Light | `#5B5D61` | `#F1F1F2`  | 5.84:1  | Pass |
| Light | `#5B5D62` | `#F3F4F6`  | 5.98:1  | Pass |
| Light | `#286E60` | `#FFFFFF`  | 6.01:1  | Pass |
| Light | `#5E6064` | `#FDFDFD`  | 6.19:1  | Pass |
| Light | `#5F6165` | `#FFFFFF`  | 6.20:1  | Pass |
| Light | `#855610` | `#FFFFFF`  | 6.29:1  | Pass |
| Light | `#FFFFFF` | `#343A44`  | 11.44:1 | Pass |
| Light | `#1A1D23` | `#DCE0E5`  | 12.73:1 | Pass |
| Light | `#1A1D23` | `#EDEDED`  | 14.41:1 | Pass |
| Light | `#1A1D23` | `#ECEEF1`  | 14.52:1 | Pass |
| Light | `#1A1D23` | `#F1F1F2`  | 14.95:1 | Pass |
| Light | `#1A1D23` | `#F3F4F6`  | 15.33:1 | Pass |
| Light | `#1A1D23` | `#FDFDFD`  | 16.59:1 | Pass |
| Light | `#1A1D23` | `#FFFFFF`  | 16.88:1 | Pass |
| Light | `#FFFFFF` | `#1A1D23`  | 16.88:1 | Pass |

## Known gaps

These need API support before the console can show them exactly:

1. Access shows "—" for _Added by_: `Grant.created_by` is only a UUID. A
   nullable grantor object `{id, name, email}` would let it show a person.
2. _Last queried_ on the clusters list is derived from the caller's 200 most
   recent history records; "—" means "not in that window". A per-cluster
   last-query timestamp would make it exact.
3. Connected clusters report `postgres` or `mysql` only, so MariaDB clusters
   can't show their own engine marker once imported.
4. Approval counts and the History/Audit date range cover loaded records only
   (the UI says so). Server-side counts and `from`/`to` filters would fix that.

Discovery and inventory tables scroll horizontally on phones to keep their
columns rather than collapsing into cards.

## Verification

`npm run test:e2e` scans every route with axe at 1440 × 900 and 390 × 844 in both
themes; the contrast table above was measured from rendered screens. Keep both
clean when changing tokens or primitives.
