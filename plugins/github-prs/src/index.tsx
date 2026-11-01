import type { Calm } from "@calm/plugin-sdk";
import { render } from "preact";
import { useEffect, useState } from "preact/hooks";

type Settings = { repos: string[] };

type PullRequest = {
  number: number;
  title: string;
  draft: boolean;
  html_url: string;
  user: { login: string } | null;
};

type RepoState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "loaded"; pulls: PullRequest[] };

const REPO_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function readSettings(value: unknown): Settings {
  const repos = (value as { repos?: unknown } | null)?.repos;
  if (!Array.isArray(repos)) return { repos: [] };
  return { repos: repos.filter((r): r is string => typeof r === "string") };
}

function RepoPulls({ calm, repo, reload, onRemove }: {
  calm: Calm;
  repo: string;
  reload: number;
  onRemove: () => void;
}) {
  const [state, setState] = useState<RepoState>({ kind: "loading" });

  useEffect(() => {
    let current = true;
    setState({ kind: "loading" });
    calm.github
      .api(`repos/${repo}/pulls?state=open&per_page=30`)
      .then((value) => {
        if (current) setState({ kind: "loaded", pulls: value as PullRequest[] });
      })
      .catch((err) => {
        if (current) setState({ kind: "error", message: errorText(err) });
      });
    // Ignore a slow answer that arrives after the repo or reload counter changed.
    return () => {
      current = false;
    };
  }, [calm, repo, reload]);

  return (
    <section>
      <h2>
        <span>{repo}</span>
        <button type="button" onClick={onRemove}>Remove</button>
      </h2>
      {state.kind === "loading" && <p class="muted">Loading...</p>}
      {state.kind === "error" && <p class="error">{state.message}</p>}
      {state.kind === "loaded" && state.pulls.length === 0 && (
        <p class="muted">No open pull requests</p>
      )}
      {state.kind === "loaded" && state.pulls.length > 0 && (
        <ul>
          {state.pulls.map((pr) => (
            <li key={pr.number} onClick={() => calm.openExternal(pr.html_url)}>
              <div>
                #{pr.number} {pr.title}
                {pr.draft && <span class="draft">Draft</span>}
              </div>
              <div class="meta">by {pr.user?.login ?? "unknown"}</div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function App({ calm }: { calm: Calm }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [inputError, setInputError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);

  // Menu items from contributes.menus.
  useEffect(
    () =>
      calm.events.on("command", (command) => {
        if (command.id !== "open-repo") return;
        calm.settings
          .get()
          .then((value) => {
            for (const repo of readSettings(value).repos) {
              calm.openExternal(`https://github.com/${repo}`).catch((err) => {
                calm.log(`open ${repo}: ${errorText(err)}`);
              });
            }
          })
          .catch((err) => calm.log(`open-repo: ${errorText(err)}`));
      }),
    [calm],
  );

  useEffect(() => {
    calm.panel.setTitle("GitHub PRs");
    calm.settings
      .get()
      .then((value) => setSettings(readSettings(value)))
      .catch((err) => setLoadError(errorText(err)));
  }, [calm]);

  function save(next: Settings) {
    setSettings(next);
    calm.settings.set(next).catch((err) => setLoadError(errorText(err)));
  }

  function addRepo(event: Event) {
    event.preventDefault();
    if (!settings) return;
    const repo = draft.trim();
    if (!REPO_PATTERN.test(repo)) {
      setInputError("Use the form owner/name");
      return;
    }
    setInputError(null);
    setDraft("");
    if (!settings.repos.includes(repo)) save({ repos: [...settings.repos, repo] });
  }

  if (loadError) return <p class="error">{loadError}</p>;
  if (!settings) return <p class="muted">Loading...</p>;

  return (
    <div>
      <form onSubmit={addRepo}>
        <input
          placeholder="owner/name"
          value={draft}
          onInput={(e) => setDraft((e.target as HTMLInputElement).value)}
        />
        <button type="submit">Add</button>
        <button type="button" onClick={() => setReload(reload + 1)}>Refresh</button>
      </form>
      {inputError && <p class="error">{inputError}</p>}
      {settings.repos.length === 0 && (
        <p class="muted">Add a repository to see its open pull requests.</p>
      )}
      {settings.repos.map((repo) => (
        <RepoPulls
          key={repo}
          calm={calm}
          repo={repo}
          reload={reload}
          onRemove={() => save({ repos: settings.repos.filter((r) => r !== repo) })}
        />
      ))}
    </div>
  );
}

const root = document.getElementById("app");
if (root) {
  if (window.calm) {
    render(<App calm={window.calm} />, root);
  } else {
    render(<p class="error">Open this plugin inside CALM.</p>, root);
  }
}
