# @calm/plugin-sdk

TypeScript types for `window.calm`, the API CALM injects into every plugin page. Types only:
there is no runtime code to install or ship, CALM provides the implementation.

`calm.d.ts` is the contract. Tests in `crates/calm-plugins/src/bridge.rs` fail if it drifts
from the injected prelude or from `BRIDGE_METHODS`. Keep one member per line in its interfaces,
because that test reads the file line by line.

## Use it in a plugin

Map the package name to the file in `tsconfig.json` (path relative to the tsconfig):

```json
{
  "compilerOptions": {
    "paths": { "@calm/plugin-sdk": ["../../packages/calm-plugin-sdk/calm.d.ts"] }
  }
}
```

Then import the types. A type-only import is erased by the bundler:

```ts
import type { Calm, PanelSpec } from "@calm/plugin-sdk";

const calm: Calm | undefined = window.calm;
const off = calm?.events.on("resize", ({ width, height, zoom }) => { /* ... */ });
```

Importing the module also declares `window.calm` globally. Type check with
`bunx tsc --noEmit -p .`.

The app does not serve any call yet: until the plugin host is wired, no `window.calm` call
resolves. See "Host support" in `docs/plugins.md`.

Plugins are trusted code. `calm.process.exec` runs programs with the user's rights.
