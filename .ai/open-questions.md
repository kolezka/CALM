# Open questions

Decisions only the user can make. Ask the user once, do not repeat. When one is answered,
record it in `decisions.md` and delete the line here. Last checked 2026-11-01.

## Plugin API v2

Spec: `docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md`, "Open points for review".

- Q1. Include `PanelInfo.joined` (join neighbors)? Recommended: yes, read-only and cheap. Blocks phase 3.
- Q2. Allow `panels.setAppearance` on every panel kind, not only TUI? Recommended: yes, customization first. Blocks phase 4.
- Q3. Restart words: backends say `on-failure`/`never`, TUI apps say `prompt`/`on-exit`/`never`. Keep apart or align? Open until phase 14.
- Q4. Move phase 11 (external control channel) earlier? The Claude Code mods integration waits on it.
- Q5. Go for the CALM-owned Ghostty fork (phase 10, `[patch.crates-io]`)? No worker touches `Cargo.toml` for it without the user's go.
