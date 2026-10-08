# Identifier font

Body text uses the installed `@fontsource-variable/inter` package's `opsz.css`.
Apple systems use SF through system font aliases only. No Apple font is bundled.

Fontsource Inter 5.3.0's Google Fonts Latin subsets contain `tnum`, but omit
`ss02` and `zero`. This supplementary UI face is a local subset of Inter's
upstream variable font, fetched from https://rsms.me/inter/font-files/InterVariable.woff2
(version string `Version 4.001;git-9221beed3`). FontTools inspection identifies
`ss02` as **Disambiguation**; `zero` and `tnum` also exist. The font retains both
`opsz` (14–32) and `wght` (100–900), with CSS restricting use to weights 400–600.

The subset retains U+0020–017F, U+2000–206F, euro, arrows and minus;
GSUB features `calt`, `locl`, `ss02`, `tnum` and `zero`, plus GPOS `kern` (without kerning, pairs around hyphens visibly drift apart). FontTools 4.61.1's subsetter
was configured with `flavor='woff2'`, `layout_features=['calt','ccmp','locl','ss02','zero','tnum','liga','kern','mark','mkmk']`,
`name_IDs=['*']`, and English name records. No network font requests occur at runtime.

See [LICENSE-Inter.txt](LICENSE-Inter.txt) for the SIL Open Font License 1.1.
Inter identifiers are UI type, not a substitute for the separate JetBrains Mono
SQL/editor face. JetBrains Mono's OFL license ships in its Fontsource package.
