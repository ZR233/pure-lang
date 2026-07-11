# Noto Sans Visual Test Font

`NotoSans-Variable.ttf` is the upstream `NotoSans[wdth,wght].ttf` file from
the Google Fonts repository. It is stored in this repository only for
deterministic, readable Flutter visual tests and is loaded at test runtime.
It is not declared in the production `pubspec.yaml` or production theme.

- Upstream: https://github.com/google/fonts
- Revision: `ec0464b978de222073645d6d3366f3fdf03376d8`
- Source file: `ofl/notosans/NotoSans[wdth,wght].ttf`
- Source URL: https://github.com/google/fonts/blob/ec0464b978de222073645d6d3366f3fdf03376d8/ofl/notosans/NotoSans%5Bwdth%2Cwght%5D.ttf
- Font SHA-256: `bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d`
- License: SIL Open Font License 1.1; see `OFL.txt` in this directory.
- License SHA-256: `e2e177a32561584d4fc13aaa3cd8e53758a12910f013fe9ca125419111722029`

The upstream filename was renamed locally to avoid shell escaping in test
commands. The font bytes are otherwise unchanged. This is a test-only asset.
The license text has one upstream trailing space removed for repository
whitespace checks; its wording is unchanged.
