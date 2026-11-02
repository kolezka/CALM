/**
 * Types for the `window.calm` API that CALM injects into every plugin page, and for the
 * `calm-plugin.json` manifest.
 *
 * @remarks
 * This file is the contract. The tests in `crates/calm-plugins/src/bridge.rs` keep the
 * injected prelude and `BRIDGE_METHODS` in sync with it. Keep one member per line: the Rust
 * conformance test reads this file line by line.
 *
 * @see docs/plugins.md
 *
 * @packageDocumentation
 */

/**
 * A canvas panel id, as the host reports it.
 *
 * @remarks
 * Exact while below 2^53 (`Number.MAX_SAFE_INTEGER`).
 */
export type PanelId = number;

/** A rectangle in canvas points. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A size in canvas points. */
export interface Size {
  width: number;
  height: number;
}

/**
 * The shape of `calm-plugin.json`.
 *
 * @remarks
 * `kind` picks the shape: absent or `"web"` is a {@link CalmWebPluginManifest}, `"tui"` a
 * {@link CalmTuiPluginManifest}. CALM rejects a manifest with a bad value; it never falls
 * back to a default. Sizes include the panel's title bar.
 */
export type CalmPluginManifest = CalmWebPluginManifest | CalmTuiPluginManifest;

/** A web plugin: an HTML page in a WKWebView panel. */
export interface CalmWebPluginManifest {
  /** @defaultValue `"web"` */
  kind?: "web";
  id: string;
  name: string;
  version: string;
  /** The HTML page, relative to the plugin folder. */
  entry: string;
  defaultSize: Size;
  /**
   * `false` keeps the panel at the size it was created with.
   * @defaultValue `true`
   */
  resizable?: boolean;
  /** Smallest size a resize may reach. At least 120x80; not allowed with `resizable: false`. */
  minSize?: Size;
  /** Largest size a resize may reach. At least `minSize`; not allowed with `resizable: false`. */
  maxSize?: Size;
  /**
   * Plugin API version, a positive integer. Above the host's, the plugin is skipped.
   * @defaultValue `1`
   */
  apiVersion?: number;
  /** What the plugin does, shown to the user. Declarative, never checked. Unknown words warn. */
  capabilities?: Capability[];
  /** Context menu items. Does not need `apiVersion` 2. */
  contributes?: PluginContributes;
  /**
   * Backend entry run under bun, relative to the plugin folder.
   * @remarks Parsed, not run yet.
   */
  main?: string;
  /** Needs `main`. */
  backend?: { start?: "on-demand" | "at-launch"; restart?: "on-failure" | "never" };
  /** Needs `main`. `bun` is a semver range such as `">=1.2"`. */
  engines?: { bun?: string };
  /**
   * Opt-in hidden page.
   * @remarks Parsed, not served yet.
   */
  background?: boolean;
}

/**
 * A TUI app: a terminal panel that runs one command.
 *
 * @remarks
 * It has no `entry`, `main`, `backend`, `engines` or `background`; those and any unknown
 * field are errors. `fontSize`, `fontFamily` and `padding` are planned and rejected for now.
 */
export interface CalmTuiPluginManifest {
  kind: "tui";
  id: string;
  /** Must not be empty. */
  name: string;
  /** @defaultValue `""` */
  version?: string;
  /** Must not be empty, start with `-`, or contain `=`. */
  command: string;
  /** Passed before a menu item's `run`. */
  args?: string[];
  /** Absolute, `~` or starting with `~/`. */
  cwd?: string;
  /** Keys match `[A-Za-z_][A-Za-z0-9_]*`. */
  env?: Record<string, string>;
  /**
   * TUI panels always resize freely.
   * @defaultValue 640x400
   */
  defaultSize?: Size;
  /** 1 to 4 characters without spaces, or a relative `.png` or `.svg` path inside the plugin folder. */
  icon?: string;
  description?: string;
  /**
   * What happens when the command exits.
   * @defaultValue `"prompt"`
   */
  restart?: "never" | "on-exit" | "prompt";
  /**
   * Run through the user's login shell, so its `PATH` applies.
   * @defaultValue `true`
   */
  shell?: boolean;
  /**
   * Title bar text, at most 120 characters. `{name}` is the only placeholder.
   * @defaultValue the manifest `name`
   */
  title?: string;
  /** `"#rrggbb"` or `"#rrggbbaa"`. Focus ring, join divider and icon tint. */
  accentColor?: string;
  /** `"#rrggbb"` or `"#rrggbbaa"`. */
  titleBarColor?: string;
  /** `"#rrggbb"` or `"#rrggbbaa"`. */
  titleColor?: string;
  terminalColors?: TerminalColors;
  contributes?: PluginContributes;
  /** As on {@link CalmWebPluginManifest.apiVersion}. */
  apiVersion?: number;
  capabilities?: Capability[];
}

/** Terminal surface colors. All three are required and take `"#rrggbb"` only, no alpha. */
export interface TerminalColors {
  background: string;
  foreground: string;
  /** Exactly 16 colors. */
  palette: string[];
}

/** `contributes` in the manifest. Any other key is an error. */
export interface PluginContributes {
  /** At most 32 items. */
  menus?: MenuContribution[];
}

/**
 * One context menu item.
 *
 * @remarks
 * Choosing it sends a `command` event to a web plugin ({@link MenuCommand}) or opens a new
 * panel of a TUI app.
 */
export interface MenuContribution {
  /** `[a-z0-9-]+`, unique in the manifest. Sent back as {@link MenuCommand.id}. */
  id: string;
  /** 1 to 80 characters. */
  title: string;
  /** `"canvas"`: right-click on empty canvas. `"panel"`: on a title bar or minimized chip. */
  location: "canvas" | "panel";
  /**
   * Panel items only.
   * @defaultValue every panel
   */
  when?: MenuWhen;
  /**
   * Items in one group sit together.
   * @defaultValue no group, listed first
   */
  group?: string;
  /**
   * Integer, sorts items inside a group. Ties keep manifest order.
   * @defaultValue `0`
   */
  order?: number;
  /** 1 to 4 characters drawn before the title. */
  icon?: string;
  /**
   * Web plugins only. `"reuse"` sends to an open panel of the plugin, `"new"` opens one.
   * @defaultValue `"reuse"`
   */
  open?: "reuse" | "new";
  /** TUI plugins only. Arguments appended to the manifest's `args`. */
  run?: string[];
}

/** Which panels show a panel menu item. Every key set must match; lists must not be empty. */
export interface MenuWhen {
  kind?: Array<"terminal" | "browser" | "plugin" | "tui">;
  /** Plugin ids. Matches plugin and TUI panels of those plugins. */
  plugin?: string[];
  minimized?: boolean;
  /** `true`: pinned panels only. `false`: panels on the canvas only. */
  pinned?: boolean;
}

/** The capability words. Declarative only. */
export type Capability =
  | "panels.create"
  | "panels.control"
  | "panels.close"
  | "camera.control"
  | "terminal.read"
  | "terminal.write"
  | "terminal.control"
  | "browser.control"
  | "browser.script"
  | "browser.data"
  | "plugins.call"
  | "settings.write"
  | "process.exec"
  | "github"
  | "open.external"
  | "layouts"
  | "backend";

/** The `code` of a rejected call. */
export type ErrorCode =
  | "unknown_method"
  | "not_implemented"
  | "invalid_params"
  | "not_found"
  | "wrong_kind"
  | "not_alive"
  | "busy"
  | "cancelled"
  | "unavailable"
  | "too_large"
  | "storage_corrupt"
  | "gh_not_installed"
  | "gh_not_logged_in"
  | "gh_failed"
  | "gh_invalid_path"
  | "timeout"
  | "not_allowed"
  | "not_installed"
  | "not_running"
  | "method_not_found"
  | "remote_error"
  | "host_error";

/**
 * What every Promise from `window.calm` rejects with.
 *
 * @remarks
 * A v1 host's string reject gets `host_error`.
 */
export interface CalmError extends Error {
  code: ErrorCode;
}

/** Injected by the host before the prelude. */
export interface CalmHostInfo {
  apiVersion: number;
  /** The methods this host serves. */
  methods: string[];
}

/** `"unsupported"` is a panel from a layout written by a newer CALM; it draws chrome only. */
export type PanelKind = "terminal" | "browser" | "plugin" | "tui" | "unsupported";

/**
 * Where a new panel goes.
 *
 * @remarks
 * `rect` places it exactly, in world units with the title bar. Without `rect` it opens at
 * the viewport center at the kind's default size (terminal from settings, browser 800x600,
 * plugin and TUI app from the manifest). The size is held within the panel's resize limits;
 * a non-resizable plugin rejects any other size. `focus` defaults to `true`. The v2 fields
 * `near`, `side`, `size`, `gap`, `title`, `args`, `env` and `appearance` reject with
 * `unavailable`; unknown fields reject with `invalid_params`.
 */
export type PanelSpec =
  | { kind: "terminal"; command?: string; cwd?: string; rect?: Rect; focus?: boolean }
  | { kind: "browser"; url: string; rect?: Rect; focus?: boolean }
  | { kind: "plugin"; pluginId: string; settings?: Record<string, unknown>; rect?: Rect; focus?: boolean }
  | { kind: "tui"; pluginId: string; rect?: Rect; focus?: boolean };

/** The resize limits of a panel, title bar included. */
export interface ResizeInfo {
  resizable: boolean;
  minSize: Size;
  maxSize: Size | null;
}

/** One panel as {@link CalmCanvas.listPanels} reports it. */
export interface PanelInfo {
  id: PanelId;
  kind: PanelKind;
  title: string;
  /** World units, title bar included. */
  rect: Rect;
  minimized: boolean;
  /** Fixed to the screen instead of the canvas. */
  pinned: boolean;
  /** Stacking position, 0 is backmost. */
  z: number;
  focused: boolean;
  /** Set for kind `"plugin"` and `"tui"`. */
  pluginId?: string;
  /** Set for kind `"browser"`: the committed URL. */
  url?: string;
  resize: ResizeInfo;
}

/** Panel size in canvas points, title bar included, and the current camera zoom. */
export interface PanelBounds {
  width: number;
  height: number;
  zoom: number;
}

/** Options for {@link CalmProcess.exec}. */
export interface ExecOptions {
  /**
   * Absolute path to an existing directory.
   * @defaultValue CALM's working directory
   */
  cwd?: string;
  /**
   * 1 to 600000. Past it the process group is killed and the call rejects with `timeout`.
   * @defaultValue `30000`
   */
  timeoutMs?: number;
}

/** What {@link CalmProcess.exec} resolves with. */
export interface ExecResult {
  /** The exit code, or 128 plus the signal number when a signal ended the process. */
  code: number;
  /** Decoded as UTF-8; invalid bytes become U+FFFD. Over 4 MiB rejects with `too_large`. */
  stdout: string;
  /** Decoded like `stdout`. */
  stderr: string;
}

export interface Theme {
  /** `"#rrggbb"` */
  background: string;
}

/** The payload of each event, by name. */
export interface CalmEventPayloads {
  focus: undefined;
  blur: undefined;
  resize: PanelBounds;
  visibility: { mode: "live" | "preview" | "hidden" };
  settingsChanged: Record<string, unknown>;
  themeChanged: Theme;
  /** A context menu item from the manifest's `contributes.menus` was chosen. */
  command: MenuCommand;
}

/** The panel a panel menu item was chosen on. */
export interface MenuCommandPanel {
  id: PanelId;
  kind: PanelKind;
  /** Set for plugin and TUI panels. */
  pluginId: string | null;
  title: string;
  /** World coordinates, chrome included. */
  rect: Rect;
  minimized: boolean;
  /** Fixed to the screen instead of the canvas. */
  pinned: boolean;
}

/** The payload of the `command` event. */
export interface MenuCommand {
  /** The item's `id` from `contributes.menus`. */
  id: string;
  location: "canvas" | "panel";
  /** The right-clicked panel for `location: "panel"`, else `null`. */
  panel: MenuCommandPanel | null;
  /** The right-click point in world coordinates for `location: "canvas"`, else `null`. */
  world: { x: number; y: number } | null;
}

/** An event name for {@link CalmEvents.on}. */
export type CalmEvent = keyof CalmEventPayloads;

/**
 * Per-panel settings, stored in the layout.
 *
 * @remarks
 * Never put a token or password here.
 */
export interface CalmSettings {
  get(): Promise<Record<string, unknown>>;
  /** Replaces the whole settings object. */
  set(value: Record<string, unknown>): Promise<void>;
}

/** Plugin-scoped persistent data in `~/.config/calm/plugin-data/<plugin-id>.json`, not in layouts. */
export interface CalmStorage {
  /** @returns The stored value, or `undefined` when `key` is not set. A stored `null` also reads back as `undefined`. */
  get<T>(key: string): Promise<T | undefined>;
  set(key: string, value: unknown): Promise<void>;
  remove(key: string): Promise<void>;
}

/** The panel that hosts this page. */
export interface CalmPanel {
  setTitle(title: string): Promise<void>;
  getBounds(): Promise<PanelBounds>;
  /** The page keeps running while minimized. */
  minimize(): Promise<void>;
  /** Goes through the normal close path. A plugin panel has no process, so it closes at once. */
  close(): Promise<void>;
}

export interface CalmCanvas {
  /**
   * Adds a panel to the canvas.
   * @param spec - What to open and where.
   * @returns The new panel's id.
   * @throws {@link CalmError} with `not_installed` for an unknown `pluginId`.
   */
  createPanel(spec: PanelSpec): Promise<PanelId>;
  /**
   * Restores a minimized panel, raises it and gives it the keyboard.
   * @throws {@link CalmError} with `not_found` for a bad id.
   */
  focusPanel(id: PanelId): Promise<void>;
  /** @returns Every panel on the canvas, back to front, not only this plugin's. */
  listPanels(): Promise<PanelInfo[]>;
}

/** Runs programs on the host. Plugins are trusted code. */
export interface CalmProcess {
  /**
   * Runs a program with the user's rights, without a shell.
   * @param command - The program. A bare name is looked up in CALM's `PATH`, which is short
   * when CALM starts from the Dock; use an absolute path or `/bin/sh -lc` for user tools.
   * @param args - Its arguments.
   * @throws {@link CalmError} with `timeout` past `timeoutMs`, `too_large` for output over 4 MiB.
   */
  exec(command: string, args: string[], options?: ExecOptions): Promise<ExecResult>;
}

export interface CalmGithub {
  /**
   * Runs `gh api <path>` on the host.
   * @returns The parsed JSON.
   */
  api(path: string): Promise<unknown>;
}

export interface CalmTheme {
  get(): Promise<Theme>;
}

/** One effective key binding: an action with the user's override applied, or a fixed key. */
export interface KeyHelpRow {
  /** Action id such as "panel.delete", or "fixed.browser.<n>" and "fixed.restart" for fixed keys. */
  id: string;
  label: string;
  /** Group title such as "Canvas" or "View"; fixed keys use "Fixed". */
  group: string;
  /** The key as macOS writes it, like "⇧⌘F". `null` when the action is unbound. */
  key: string | null;
}

export interface CalmKeys {
  /** Every action and fixed key in Keys tab order, with the current settings applied. */
  list(): Promise<{ keys: KeyHelpRow[] }>;
}

/** Host to plugin events. Served by the prelude, not a host request. */
export interface CalmEvents {
  /**
   * Adds a handler for `event`.
   * @returns A function that removes the handler.
   * @example
   * ```ts
   * const off = window.calm.events.on("command", (command) => console.log(command.id));
   * ```
   */
  on<E extends CalmEvent>(event: E, handler: (payload: CalmEventPayloads[E]) => void): () => void;
}

/** The API on `window.calm`. */
export interface Calm {
  settings: CalmSettings;
  storage: CalmStorage;
  panel: CalmPanel;
  canvas: CalmCanvas;
  process: CalmProcess;
  github: CalmGithub;
  theme: CalmTheme;
  keys: CalmKeys;
  events: CalmEvents;
  /** Opens an `http` or `https` URL in the default browser. Other schemes reject. */
  openExternal(url: string): Promise<void>;
  /**
   * Whether this host serves `method`, such as `"panel.setTitle"`. Synchronous, served by
   * the prelude.
   */
  has(method: string): boolean;
  /**
   * Writes to CALM's stderr, tagged with the plugin.
   * @remarks
   * Fire and forget: the prelude posts the message and keeps no pending entry, so nothing
   * waits for a host answer.
   */
  log(message: string): void;
}

declare global {
  interface Window {
    calm: Calm;
    __calmHost?: CalmHostInfo;
  }
}
