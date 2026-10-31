// Types for the window.calm API that CALM injects. See docs/plugins.md.
interface CalmApi {
  settings: {
    get(): Promise<unknown>;
    set(settings: unknown): Promise<unknown>;
  };
  panel: { setTitle(title: string): Promise<unknown> };
  github: { api(path: string): Promise<unknown> };
  log(message: string): Promise<unknown>;
  openExternal(url: string): Promise<unknown>;
}

interface Window {
  calm?: CalmApi;
}
