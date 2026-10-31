<!-- repo-intel:begin -->
## Code graphs

This repo is indexed for faster lookup. Use the right tool for the question.

Concepts, architecture, why something was decided: use graphify.
`graphify query "<question>"`, `graphify explain X`, `graphify path "A" "B"`.

Literal text, config values, error strings, and every call site before calling
a change safe or code unused: use rg (or grep), not the graphs. They miss
dynamic dispatch, generated code and bodies wrapped in higher-order calls.

Refresh when stale: `repo-intel build` if installed, else `codegraph sync`
and `graphify update .`.
<!-- repo-intel:end -->
