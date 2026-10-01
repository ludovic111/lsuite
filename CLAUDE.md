# lsuite.xyz

The suite's site; see README.md (layout, routes, domains) and STANDARD.md (the contract every
lsuite app meets, with a status table). Each app repo (`../ryolune`, `../kimchi`, `../zenith`)
has an "lsuite" section in its CLAUDE.md with its remaining gaps.

## Next session (2026-10-01)

- [x] ryolune 0.12 page (2026-10-01): New in 0.12, one theme dark/light gallery, 35 plugins,
      199 commands, new agents. Versions on pages are `%VERSION:<app>%`, filled by `server.js`
      from the latest published GitHub release, so the hero shows 0.11.1 until the 0.12.0 release
      is published (its release run failed on Apple notarization: the Apple developer agreement
      must be accepted, then re-run). Retake captures from `../ryolune/site/img/` when they change.
- [x] zenith page (2026-10-01): rewritten for the new zenith, a Mac app for coding with agents
      (Claude Code and Codex threads, Rust server, Tauri app); `zmock` now illustrates a thread.
      The old dashboard, Ask zenith, the agent team and the `zenith_*` MCP tools are gone: don't
      advertise them. `assets/img/og/zenith.png` retaken from the new page (2026-10-01).
- [ ] Keep the agent-readiness table on the home page and `STANDARD.md`'s status table in step
      as kimchi and zenith close their gaps.
- [x] **Design system on the site itself** (2026-10-01): pages load `/design/tokens.css` before
      `assets/styles.css`, whose variables all map onto `--ls-*`; app pages set `data-app` on
      `<html>` (signature color, aurora backdrop); nav, cards, cells, tables, stats, downloads, FAQ,
      bands and footer are glass tier 1, terminals and code tier 2; screenshots stay solid. Light and
      dark follow the system. No hard-coded color is left in `styles.css` or the pages: add a token.
