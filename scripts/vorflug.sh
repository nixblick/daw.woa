#!/bin/sh
# Vorflug vor jedem Push: dieselben drei Prüfungen wie die CI (fmt, clippy, test).
# Aufruf aus dem Repo:
#   scripts/vorflug.sh                       fmt, clippy, ganze Testsuite (wie CI)
#   scripts/vorflug.sh -p woa-gui -p woa-agent   Tests nur für diese Crates (Notebook, 7 GB RAM)
#   scripts/vorflug.sh --fmt                 nur Format (pre-push-Hook)
set -eu
cd "$(dirname "$0")/.."
echo "vorflug: cargo fmt --check"
cargo fmt --all --check
[ "${1:-}" = "--fmt" ] && exit 0
echo "vorflug: cargo clippy -D warnings"
cargo clippy --workspace --all-targets -- -D warnings
echo "vorflug: cargo test"
if [ "$#" -gt 0 ]; then cargo test "$@"; else cargo test --workspace; fi
echo "vorflug: alles grün"
