#!/bin/sh
# The local gate, cheapest check first, stopping at the first failure.
#   scripts/check.sh               before every commit: fmt, clippy, every test
#   scripts/check.sh fast [FILTER] while coding: the tests matching FILTER only
#                                  (a nextest filterset, e.g. 'test(post)')
# Tests run on the in-memory MySQL of docker-compose.yml (.cargo/config.toml).
set -eu
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

if ! cargo nextest --version >/dev/null 2>&1; then
  echo "check: cargo-nextest is missing: brew install cargo-nextest (or cargo install cargo-nextest --locked)" >&2
  exit 1
fi

step() {
  printf '\n==> %s\n' "$*"
}

step "MySQL (docker compose)"
docker compose up -d --wait mysql-test >/dev/null

if [ "${1:-}" = "fast" ]; then
  shift
  step "tests ${1:-(all)}"
  if [ $# -gt 0 ]; then
    cargo nextest run --locked -E "$1"
  else
    cargo nextest run --locked
  fi
  exit 0
fi

step "cargo fmt --check"
cargo fmt --check
step "cargo clippy"
cargo clippy --locked --all-targets -- -D warnings
step "tests"
cargo nextest run --locked
printf '\ncheck: all passed\n'
