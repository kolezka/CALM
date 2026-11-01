// Types for the `window.calm` API that CALM injects into every plugin page.
// This file is the contract. `crates/calm-plugins/src/bridge.rs` tests keep the injected
// prelude and `BRIDGE_METHODS` in sync with it. Reference: docs/plugins.md.
//
// Keep one member per line: the Rust conformance test reads this file line by line.

/** A canvas panel id, as the host reports it. Exact while below 2^53 (Number.MAX_SAFE_INTEGER). */
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
 * The shape of `calm-plugin.json`. CALM rejects a manifest with a bad value; it never
 * falls back to a default. Sizes include the panel's title bar.
 */
export interface CalmPluginManifest {
  id: string;
  name: string;
  version: string;
  entry: string;
  defaultSize: Size;
  /** `false` keeps the panel at the size it was created with. Absent means `true`. */
  resizable?: boolean;
  /** Smallest size a resize may reach. At least 120x80; not allowed with `resizable: false`. */
  minSize?: Size;
  /** Largest size a resize may reach. At least `minSize`; not allowed with `resizable: false`. */
  maxSize?: Size;
  /** Plugin API version, a positive integer. Missing means 1. Above the host's, the plugin is skipped. */
  apiVersion?: number;
  /** What the plugin does, shown to the user. Declarative, never checked. Unknown words warn. */
  capabilities?: Capability[];
  /** Backend entry run under bun, relative to the plugin folder. Parsed, not run yet. */
  main?: string;
  /** Needs `main`. */
  backend?: { start?: "on-demand" | "at-launch"; restart?: "on-failure" | "never" };
  /** Needs `main`. `bun` is a semver range such as ">=1.2". */
  engines?: { bun?: string };
  /** Opt-in hidden page. Parsed, not served yet. */
  background?: boolean;
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

/** What every Promise from `window.calm` rejects with. A v1 host's string reject gets `host_error`. */
export interface CalmError extends Error {
  code: ErrorCode;
}

/** Injected by the host before the prelude. `methods` is what this host serves. */
export interface CalmHostInfo {
  apiVersion: number;
  methods: string[];
}

/** `"unsupported"` is a panel from a layout written by a newer CALM; it draws chrome only. */
export type PanelKind = "terminal" | "browser" | "plugin" | "tui" | "unsupported";

/**
 * Where a new panel goes: `rect` places it exactly, in world units with the title bar.
 * Without `rect` it opens at the viewport center at the kind's default size (terminal from
 * settings, browser 800x600, plugin and TUI app from the manifest). The size is held within
 * the panel's resize limits; a non-resizable plugin rejects any other size.
 * `focus` defaults to `true`. The v2 fields `near`, `side`, `size`, `gap`, `title`, `args`,
 * `env` and `appearance` reject with `unavailable`; unknown fields reject with `invalid_params`.
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

export interface PanelInfo {
  id: PanelId;
  kind: PanelKind;
  title: string;
  /** World units, title bar included. */
  rect: Rect;
  minimized: boolean;
  /** Stacking position, 0 is backmost. */
  z: number;
  focused: boolean;
  /** Kind "plugin" or "tui". */
  pluginId?: string;
  /** Kind "browser": the committed URL. */
  url?: string;
  resize: ResizeInfo;
}

/** Panel size in canvas points, title bar included, and the current camera zoom. */
export interface PanelBounds {
  width: number;
  height: number;
  zoom: number;
}

export interface ExecOptions {
  /** Absolute path to an existing directory. Default: CALM's working directory. */
  cwd?: string;
  /** 1 to 600000. Default 30000. Past it the process group is killed and the call rejects with `timeout`. */
  timeoutMs?: number;
}

export interface ExecResult {
  /** The exit code, or 128 plus the signal number when a signal ended the process. */
  code: number;
  /** Decoded as UTF-8; invalid bytes become U+FFFD. Over 4 MiB rejects with `too_large`. */
  stdout: string;
  stderr: string;
}

export interface Theme {
  /** "#rrggbb" */
  background: string;
}

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
  id: number;
  kind: "terminal" | "browser" | "plugin" | "tui";
  /** Set for plugin and TUI panels. */
  pluginId: string | null;
  title: string;
  /** World coordinates, chrome included. */
  rect: { x: number; y: number; width: number; height: number };
  minimized: boolean;
}

export interface MenuCommand {
  /** The item's `id` from `contributes.menus`. */
  id: string;
  location: "canvas" | "panel";
  /** The right-clicked panel for `location: "panel"`, else null. */
  panel: MenuCommandPanel | null;
  /** The right-click point in world coordinates for `location: "canvas"`, else null. */
  world: { x: number; y: number } | null;
}

export type CalmEvent = keyof CalmEventPayloads;

/** Per-panel settings, stored in the layout. Never put a token or password here. */
export interface CalmSettings {
  get(): Promise<Record<string, unknown>>;
  set(value: Record<string, unknown>): Promise<void>;
}

/** Plugin-scoped persistent data in `~/.config/calm/plugin-data/<plugin-id>.json`, not in layouts. */
export interface CalmStorage {
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
  /** Resolves with the new panel's id. Rejects with `not_installed` for an unknown `pluginId`. */
  createPanel(spec: PanelSpec): Promise<PanelId>;
  /** Restores a minimized panel, raises it and gives it the keyboard. `not_found` for a bad id. */
  focusPanel(id: PanelId): Promise<void>;
  /** Every panel on the canvas, back to front, not only this plugin's. */
  listPanels(): Promise<PanelInfo[]>;
}

/**
 * Runs a program on the host with the user's rights, without a shell: `command` is the
 * program and `args` its arguments. A bare name is looked up in CALM's PATH, which is short
 * when CALM starts from the Dock; use an absolute path or `/bin/sh -lc` for user tools.
 * Plugins are trusted code.
 */
export interface CalmProcess {
  exec(command: string, args: string[], options?: ExecOptions): Promise<ExecResult>;
}

export interface CalmGithub {
  /** Runs `gh api <path>` on the host and resolves with the parsed JSON. */
  api(path: string): Promise<unknown>;
}

export interface CalmTheme {
  get(): Promise<Theme>;
}

/** Host to plugin events. Served by the prelude, not a host request. */
export interface CalmEvents {
  /** Returns a function that removes the handler. */
  on<E extends CalmEvent>(event: E, handler: (payload: CalmEventPayloads[E]) => void): () => void;
}

export interface Calm {
  settings: CalmSettings;
  storage: CalmStorage;
  panel: CalmPanel;
  canvas: CalmCanvas;
  process: CalmProcess;
  github: CalmGithub;
  theme: CalmTheme;
  events: CalmEvents;
  openExternal(url: string): Promise<void>;
  /** Synchronous: whether this host serves `method`, such as "panel.setTitle". Served by the prelude. */
  has(method: string): boolean;
  /**
   * Writes to CALM's stderr, tagged with the plugin. Fire and forget: the prelude posts the
   * message and keeps no pending entry, so nothing waits for a host answer.
   */
  log(message: string): void;
}

declare global {
  interface Window {
    calm: Calm;
    __calmHost?: CalmHostInfo;
  }
}
