#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! -f $1 ]]; then
    printf 'usage: %s FILE\n' "${0##*/}" >&2
    exit 64
fi

tag=${GITHUB_REF_NAME:-$(git describe --tags --exact-match)}
version=$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -c 'import json,sys; print(next(package["version"] for package in json.load(sys.stdin)["packages"] if package["name"] == "alacritty-1337"))')
if [[ $tag != "v$version" ]]; then
    printf 'tag %s disagrees with Cargo version %s\n' "$tag" "$version" >&2
    exit 1
fi
if ! gh release view "$tag" >/dev/null 2>&1; then
    gh release create "$tag" --draft --verify-tag \
        --title "alacritty-1337 $version" \
        --notes 'Draft assembled by the release workflow; publish only after artifact adjudication.' \
        || gh release view "$tag" >/dev/null
fi
gh release upload "$tag" "$1" --clobber
