# Contributing to alacritty-1337

Report defects and submit pull requests through the
[fork issue tracker](https://github.com/aoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoa/alacritty-1337/issues)
and
[fork pull-request surface](https://github.com/aoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoa/alacritty-1337/pulls).
Include the operating system, display backend, input method, configuration
needed to reproduce the problem, and the output of `alacritty --version`.

## Gate

Use Rust 1.97.1 and run the canonical nonmutating gate before submission:

```sh
rustup toolchain install 1.97.1 --profile minimal --component clippy,rustfmt
./check.py verify
cargo deny check
```

`./check.py check` additionally applies the repository's canonical formatting
and lint fixes. `./check.py deep` includes rustdoc. Linux verification covers
the X11-only and Wayland-only feature sets independently.

Tests belong at the narrowest contract boundary that can prove a change. A bug
fix should fail under the old implementation and pass under the new one. Keep
configuration behavior synchronized with `extra/man/alacritty.5.scd`, CLI
behavior synchronized with the command manpages and generated completions, and
user-visible changes synchronized with `CHANGELOG.md`.

## Release

Application releases use stable SemVer. The application version in
`alacritty/Cargo.toml` is authoritative; internal library crates retain their
own version lines. The `Release` GitHub workflow must first pass under
`workflow_dispatch`, producing inspected platform artifacts and `SHA256SUMS`.
A tag is valid only when it equals `v` plus the Cargo application version. Tag
runs create a draft release; publication occurs only after the draft artifacts
match the proved candidate.

The public release profile is portable. `packaging/arch` is a separate private
`target-cpu=native` package path and must never supply a generic public binary.
