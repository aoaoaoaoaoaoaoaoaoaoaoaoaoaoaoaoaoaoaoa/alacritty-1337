#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! -f $1 ]]; then
    printf 'usage: %s FILE\n' "${0##*/}" >&2
    exit 64
fi

tag=${GITHUB_REF_NAME:-$(git describe --tags --exact-match)}
if ! gh release view "$tag" >/dev/null 2>&1; then
    gh release create "$tag" --draft --verify-tag || gh release view "$tag" >/dev/null
fi
gh release upload "$tag" "$1" --clobber
