#!/usr/bin/env bash
# Single entry point: checks, version, tag, push. The GitHub workflow builds and publishes.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
# Prefer rustup's Cargo over an older Homebrew Cargo.
if [ -x "$HOME/.cargo/bin/cargo" ]; then export PATH="$HOME/.cargo/bin:$PATH"; fi
export GIT_TERMINAL_PROMPT=0
export GIT_SSH_COMMAND="${GIT_SSH_COMMAND:-ssh -o BatchMode=yes}"
exec "$ROOT/scripts/release.sh" "$@" </dev/null
