# Developer entry points. `make` prints them.
#
# `.env` (gitignored, see .env.example) holds the keys; every target loads
# it. ORBIT_NO_KEYCHAIN keeps the Keychain out of the loop, so a
# rebuilt binary never asks for the login password.
.DEFAULT_GOAL := help
SHELL := /bin/bash
# A company fork's build settings, absent in the generic repo; it exports
# BUNDLE_ID (scripts/app.sh), ORBIT_RELEASES_REPO (owner/repo the app
# updates from; unset: Cargo.toml's repository) and RELEASE_REMOTES
# (scripts/release.sh), and for scripts/fork.sh UPSTREAM_REPO, OVERLAY_PATHS
# and UPSTREAM_DENY.
-include overlay.mk
ENV = set -a; [ -f .env ] && source .env; set +a; export ORBIT_NO_KEYCHAIN=1;

help: ## this list
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-8s %s\n", $$1, $$2}'

run: ## start the app with the keys from .env, no Keychain (as target/Orbit.app: own notifications)
	@$(ENV) cargo build --release && scripts/app.sh target/release/orbit target/Orbit.app && target/Orbit.app/Contents/MacOS/orbit

dev: ## same, debug build, against an isolated data dir (never your real sessions)
	@$(ENV) ORBIT_DATA=target/dev-data cargo run

# Only compile-time inputs: the synced skills are read from disk at runtime,
# and a restart on the app's own Linear pull would cut that sync short.
watch: ## make dev, rebuilt and restarted on every save (brew install watchexec)
	@command -v watchexec >/dev/null || { echo "watchexec missing: brew install watchexec" >&2; exit 1; }
	@$(ENV) ORBIT_DATA=target/dev-data watchexec -r -w src -w assets -w skills -w Cargo.toml -w Cargo.lock -- cargo run

check: fmt-check lint test ## fmt + clippy + tests, what CI runs

fmt-check: ## the code is formatted
	cargo fmt --check

lint: ## clippy, warnings are errors
	cargo clippy --all-targets --release -- -D warnings

test: ## the tests, incl. every product decision's guard (and the pi extension's, with node 23.6+)
	cargo test --release
	@if node -e 'const [a,b]=process.versions.node.split(".").map(Number);process.exit(a>23||(a==23&&b>=6)?0:1)' 2>/dev/null; \
	then node --test --test-reporter=dot tests/pi-extension.test.mjs; \
	else echo "skipped tests/pi-extension.test.mjs: needs node 23.6 or newer"; fi

fix: ## format and apply clippy's suggestions
	cargo fmt
	cargo clippy --all-targets --release --fix --allow-dirty

shot: ## screenshot a view: make shot VIEW=foryou|stories|intake|sessions|customers [TAB=claude]
	@$(ENV) scripts/shot.sh $(or $(VIEW),foryou) $(TAB)

bundle: ## build dist/Orbit.app (Apple silicon, ad-hoc signed)
	scripts/bundle-macos.sh

sync: ## in a fork: merge upstream's main, then make check (docs/forks.md)
	scripts/fork.sh sync

upstream: ## in a fork: send commits upstream as a PR: make upstream C="<sha> <sha>"
	scripts/fork.sh upstream $(C)

release: ## cut a release: make release V=0.2.0 (tag, push; CI publishes the zip)
	scripts/release.sh $(V)

.PHONY: help run dev watch check fmt-check lint test fix shot bundle sync upstream release
