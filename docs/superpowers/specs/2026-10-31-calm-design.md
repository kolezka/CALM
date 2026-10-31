# CALM design

Status: draft for review. No implementation or performance claims have been verified.

## Purpose

CALM is a spatial workspace for developers. It offers a nearly infinite, pannable and zoomable canvas containing interactive terminals, browser views and third-party panels. The initial target is macOS. The model and plugin contract should leave room for other desktop platforms without promising their support in the first release.

The main use case is arranging several differently sized terminals on the canvas and running ordinary terminal applications such as Claude Code, slk and tmux. Users can arrange a browser and other panels alongside them without relying on tabs. Optional groups of nested terminal tabs and direct agent integration are later capabilities, not prerequisites for the first release.

## Product requirements

- Create terminal panels by drawing a rectangle on the canvas. Move and resize each independently. Support overlapping panels and predictable focus and stacking.
- Pan along both axes and zoom around the pointer or gesture focal point. The canvas is practically unbounded, subject to numeric and resource limits. Camera zoom changes visual scale, not the terminal's logical grid size. Resizing a terminal panel changes its logical viewport.
- Support full-screen use on macOS, without requiring the app to stay full-screen.
- Terminals support interactive shell and TUI input, ordinary mouse and keyboard interaction, text selection, clipboard, and input methods. Running tmux inside a terminal is allowed; CALM does not require or integrate tmux in the first release.
- Provide browser panels and the plugin system described below. Users can minimize panels and restore them without stopping their contents. Define the visual form of minimized panels during interaction design.
- For performance, panels at a sufficiently distant zoom level or outside the viewport may show a preview or stop updating their visible representation. Their underlying processes and plugin state must continue running while the app is open. Returning to a panel must restore its interaction and current content. Preview freshness and the switching threshold are implementation choices to measure and communicate.
- Support roughly 5, 10 and 20 simultaneously running terminals as test scenarios, not hard limits. Smooth interaction is a product goal, not a measured guarantee. Define hardware, workloads, display refresh rate and frame-time and memory budgets before accepting benchmark results.
- Save named layouts and load them on demand. A layout includes element type, coordinates, dimensions, stacking, panel settings, minimized state and camera state. On load, terminal panels start new shell sessions without replaying prior commands, and browser panels open their configured or default page. Do not store credentials in layouts.
- Processes are not required to survive a full application quit. A fresh app launch after a computer restart starts with an empty canvas; users can explicitly load a saved layout. Closing or minimizing an individual panel must not silently kill a running terminal process: provide an explicit close action and confirmation when a process might be lost. Distinguish closing the app window from quitting the app during design.

## Technical direction

Use Rust and GPUI for the application core and canvas. Integrate Ghostty terminal surfaces using libghostty, evaluating the alpha gpui-libghostty project as a candidate rather than committing to it without a spike. Pin tested upstream revisions and put integrations behind project-owned interfaces because GPUI and Ghostty integrations may change.

The canvas owns panel identity, world-space geometry, z-order, camera transform and focus routing. Panel providers own their content lifecycle and panel-specific settings. Moving, zooming, minimizing or virtualizing a panel must not unintentionally destroy its session. The system must distinguish view lifecycle from terminal process lifecycle while the app runs.

Use a native browser view on macOS, likely WebKit, subject to a GPUI embedding spike. A browser panel is distinct from the web-based plugin host. Native view compositing, overlapping order, clipping, zoom, mouse mapping, keyboard focus, selection, high-DPI backing scales and input method placement are risks to test rather than assumed capabilities. Use platform adapters for native views and avoid promising equivalent support on other operating systems until tested.

## Plugins

Plugin customization by third parties is a first-release requirement, not a future extension point. A plugin can register a panel type with metadata, a user interface, settings and lifecycle hooks, without rebuilding CALM. The proposed runtime format is JavaScript with HTML and CSS; TypeScript and TSX are supported authoring choices compiled to JavaScript. Provide a starter template and document install, development, configuration and troubleshooting. The exact packaging and host API remain design decisions subject to the prototype.

The initial examples are:

1. **Hello World:** a minimal plugin showing text, installable independently of an app rebuild.
2. **GitHub pull requests:** a plugin whose user configures one or more public or private repositories and sees a pull-request list. Show loading, empty and error states. Evaluate the existing authenticated GitHub CLI as an initial credential source, without assuming it is installed or logged in; provide a clear setup path. Do not store or log tokens in layouts, plugin manifests, documentation or project state.

For the first version, users explicitly install plugins they trust. Fine-grained plugin permissions and an enforced sandbox are deferred. A malicious plugin could access private data, so documentation must not describe plugins as sandboxed. Remote sites displayed in browser panels remain untrusted and must not receive plugin host privileges. Keep credentials out of layouts, plugin files and logs, and prefer host-mediated GitHub access without exposing tokens to plugin UI code. The exact host boundary and authentication mechanism remain to be tested.

## Prototype and acceptance

The first technical prototype validates the highest-risk integrations before a stable architecture is claimed:

1. Launch a real macOS window and present at least two independently positioned and resized live Ghostty terminals in a pannable, zoomable canvas.
2. Exercise keyboard input, shell output, selection, mouse interactions and input methods at different zoom levels and pixel densities. Verify actual panel resizing separately from camera zoom.
3. Show one browser or web plugin panel alongside a terminal. Exercise overlap, clipping, focus and interaction while panning and zooming.
4. Exercise preview mode and restoration while terminal output continues. Verify minimizing and restoring does not stop the running process.
5. Measure the defined 5, 10 and 20 terminal workloads, including offscreen, visible and output-producing terminals. Record hardware, workload, method, frame-time distribution, CPU and memory, with budget and limitations. No target is considered met until measured.
6. Exercise layout saving and loading, including fresh terminal sessions and browser default/configured page. Confirm layout data contains no credentials.
7. Load the Hello World plugin without rebuilding the host and exercise a configured GitHub PR plugin against a permitted test repository. Private-repository and authentication tests need an authorized account and must not expose credentials.

The prototype can identify incompatible approaches and lead to a revised design. It is not permission to claim cross-platform support or to ship unverified renderer behavior.

## Project setup and documentation

The project location and name are `/Users/me/Development/CALM` and CALM. Set up version control and isolate code changes in a worktree when implementation begins, following the user's global instructions. The directory is currently empty and not a Git repository.

Create and maintain a project `CLAUDE.md`, onboarding/build documentation, architecture notes, plugin author documentation and the benchmark method as implementation progresses. Use `repo-intel build` from the repository root when there is a repository and material to index; inspect its configuration first, capture its exit status, and report failures or partial parsing rather than treating an incomplete graph as verified. Do not run graph extraction subagents when the configured backend disallows them.

## Not in the first release

- Persisting live terminal processes after the application quits or a computer restarts.
- Automatic spawning or controlling Claude Code agents as separate canvas panels. This requires separate feasibility and product design.
- A full tmux-like session manager, automatic command replay, or mandatory terminal tabs.
- A marketplace or a guarantee that arbitrary installed plugins are safe.
- A promise of Windows or Linux support, or a performance target unsupported by measurement.

## Open decisions and risks

- Select the Ghostty integration and verify its rendering and lifecycle on macOS, including whether it allows previewing without interrupting sessions.
- Choose the browser and plugin hosting model, including how it transforms and clips against GPUI's rendering and how panels overlap.
- Choose the GitHub authentication mechanism and define which host capabilities trusted plugins receive. Fine-grained permissions and a sandbox are deferred.
- Set benchmark budgets on target hardware, preview behavior and camera zoom limits after a measured prototype.
- Define the close-window versus quit-app behavior, minimized-panel interaction, and the exact initial plugin API without changing the user-visible requirements above.
