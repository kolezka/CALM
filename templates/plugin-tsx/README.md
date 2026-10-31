# CALM plugin template (TSX)

A starting point for a CALM plugin written in TypeScript and TSX, with Preact as the JSX runtime.

1. Copy this folder and rename it.
2. In `calm-plugin.json`, set a unique `id` (lowercase letters, digits, `-`) and a `name`.
3. Build: `bun install && bun run build`. This writes `dist/index.js`, which `index.html` loads.
4. Run CALM with the parent folder on the plugin path, for example
   `CALM_PLUGIN_PATH=/path/to/my-plugins cargo run -p calm-app`.

The `window.calm` types come from `@calm/plugin-sdk`, mapped in `tsconfig.json` `paths` to
`packages/calm-plugin-sdk/calm.d.ts` in the CALM repository. After copying this folder, fix that
path to point at your CALM checkout. Type check with `bunx tsc --noEmit -p .`. The full reference
is `docs/plugins.md` in the CALM repository.

Plugins are trusted code with full access to the host bridge. They are not sandboxed.
