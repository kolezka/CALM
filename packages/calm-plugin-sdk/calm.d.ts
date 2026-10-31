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
}

export type PanelKind = "terminal" | "browser" | "plugin";

export type PanelSpec =
  | { kind: "terminal"; command?: string; cwd?: string; rect?: Rect }
  | { kind: "browser"; url: string; rect?: Rect }
  | { kind: "plugin"; pluginId: string; settings?: Record<string, unknown>; rect?: Rect };

export interface PanelInfo {
  id: PanelId;
  kind: PanelKind;
  title: string;
  rect: Rect;
  minimized: boolean;
}

/** Panel size in canvas points and the current camera zoom. */
export interface PanelBounds {
  width: number;
  height: number;
  zoom: number;
}

export interface ExecOptions {
  cwd?: string;
  timeoutMs?: number;
}

export interface ExecResult {
  code: number;
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
  minimize(): Promise<void>;
  /** Goes through the normal close path, including its confirmation. */
  close(): Promise<void>;
}

export interface CalmCanvas {
  createPanel(spec: PanelSpec): Promise<PanelId>;
  focusPanel(id: PanelId): Promise<void>;
  listPanels(): Promise<PanelInfo[]>;
}

/** Runs a program on the host with the user's rights. Plugins are trusted code. */
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
  /**
   * Writes to CALM's stderr, tagged with the plugin. Fire and forget: the prelude posts the
   * message and keeps no pending entry, so nothing waits for a host answer.
   */
  log(message: string): void;
}

declare global {
  interface Window {
    calm: Calm;
  }
}
