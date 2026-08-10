#!/usr/bin/env bash
set -euo pipefail

command -v perf >/dev/null || { printf '%s\n' 'error: perf is not installed' >&2; exit 1; }
command -v cargo-flamegraph >/dev/null || {
    printf '%s\n' 'error: cargo-flamegraph is not installed' >&2
    exit 1
}

exec cargo flamegraph --bin alacritty -- "$@"
