.DEFAULT_GOAL := help
SHELL := /bin/bash
.SHELLFLAGS := -euo pipefail -c
MAKEFLAGS += --no-builtin-rules

ZIG ?= /opt/homebrew/opt/zig@0.16/bin/zig
N ?= 20
SECONDS ?= 30
ZOOM ?= 1.0
VERSION ?= 0.1.0
OUT ?= dist

export ZIG

.PHONY: help setup-check build test lint fmt smoke run bench bundle plugins plugins-check ci graph clean

help: ## Show the available targets.
	@awk 'BEGIN { FS = ":.*## " } /^[a-zA-Z0-9_-]+:.*## / { printf "  %-14s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

setup-check: ## Check the local build prerequisites.
	@failures=0; \
	if rust_version=$$(rustc --version 2>/dev/null); then \
		printf '%s\n' "$$rust_version"; \
		if [[ "$$rust_version" =~ rustc[[:space:]]+([0-9]+)\.([0-9]+) ]]; then \
			rust_major=$${BASH_REMATCH[1]}; rust_minor=$${BASH_REMATCH[2]}; \
			if (( rust_major > 1 || (rust_major == 1 && rust_minor >= 95) )); then printf 'ok: rustc meets 1.95 or newer\n'; else printf 'error: rustc 1.95 or newer is required\n' >&2; failures=$$((failures + 1)); fi; \
		else printf 'error: could not parse rustc version\n' >&2; failures=$$((failures + 1)); fi; \
	else printf 'error: rustc is missing\n' >&2; failures=$$((failures + 1)); fi; \
	if cargo_version=$$(cargo --version 2>/dev/null); then printf '%s\n' "$$cargo_version"; else printf 'error: cargo is missing\n' >&2; failures=$$((failures + 1)); fi; \
	zig_path="$${ZIG:-/opt/homebrew/opt/zig@0.16/bin/zig}"; \
	if zig_version=$$("$$zig_path" version 2>/dev/null); then \
		printf 'zig %s\n' "$$zig_version"; \
		if [[ "$$zig_version" =~ ^0\.16\.[0-9]+ ]]; then printf 'ok: Zig 0.16.x\n'; else printf 'error: Zig 0.16.x is required\n' >&2; failures=$$((failures + 1)); fi; \
	else printf 'error: Zig is missing or unavailable at %s\n' "$$zig_path" >&2; failures=$$((failures + 1)); fi; \
	if metal_path=$$(xcrun -f metal 2>/dev/null); then printf 'Metal tool: %s\n' "$$metal_path"; else printf 'error: xcrun could not find the Metal tool\n' >&2; failures=$$((failures + 1)); fi; \
	if gh_version=$$(gh --version 2>/dev/null); then \
		printf '%s\n' "$$gh_version"; \
		if gh auth status >/dev/null 2>&1; then printf 'ok: gh is authenticated\n'; else printf 'error: gh auth status failed\n' >&2; failures=$$((failures + 1)); fi; \
	else printf 'error: gh is missing\n' >&2; failures=$$((failures + 1)); fi; \
	if bun_version=$$(bun --version 2>/dev/null); then printf 'bun %s\n' "$$bun_version"; else printf 'error: bun is missing\n' >&2; failures=$$((failures + 1)); fi; \
	if actionlint_version=$$(actionlint --version 2>/dev/null); then printf 'actionlint %s\n' "$$actionlint_version"; else printf 'warning: actionlint is not installed (optional)\n'; fi; \
	if (( failures == 0 )); then printf 'setup check passed\n'; else printf 'setup check failed (%d prerequisite(s))\n' "$$failures" >&2; exit 1; fi

build: ## Build all workspace targets.
	@cargo build --workspace --all-targets

test: ## Run all workspace tests.
	@cargo test --workspace

lint: ## Run Clippy and check Rust formatting.
	@cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check

fmt: ## Format all Rust code.
	@cargo fmt --all

smoke: ## Run the application smoke gate.
	@cargo run -p calm-app -- --smoke

run: ## Run the application, optionally with ARGS.
	@cargo run -p calm-app $(if $(strip $(ARGS)),-- $(ARGS))

bench: ## Run a benchmark with N, SECONDS, and ZOOM options.
	@cargo run --release -p calm-app -- --bench $(N) --bench-seconds $(SECONDS) --bench-zoom $(ZOOM)

bundle: ## Build a macOS bundle with VERSION and OUT options.
	@if [[ ! -f scripts/bundle-macos.sh ]]; then printf 'error: scripts/bundle-macos.sh is not available yet\n' >&2; exit 1; fi; scripts/bundle-macos.sh "$(VERSION)" "$(OUT)"

plugins: ## Install and build every plugin package.
	@for manifest in plugins/*/package.json templates/*/package.json; do \
		[[ -f "$$manifest" ]] || continue; \
		package_dir=$${manifest%/*}; \
		(cd "$$package_dir" && bun install && bun run build) || exit $$?; \
	done

plugins-check: ## Type-check every plugin package.
	@for manifest in plugins/*/package.json templates/*/package.json; do \
		[[ -f "$$manifest" ]] || continue; \
		package_dir=$${manifest%/*}; \
		(cd "$$package_dir" && bunx tsc --noEmit -p .) || exit $$?; \
	done

ci: ## Run lint, tests, smoke, and plugin checks in order.
	@$(MAKE) lint && $(MAKE) test && $(MAKE) smoke && $(MAKE) plugins-check

graph: ## Build the repo-intel graphs and report the exit code.
	@if repo-intel build; then result=0; else result=$$?; fi; \
	printf 'repo-intel build exit code: %s\n' "$$result"; \
	exit "$$result"

clean: ## Remove Rust and plugin build outputs.
	@cargo clean && rm -rf plugins/*/dist templates/*/dist
