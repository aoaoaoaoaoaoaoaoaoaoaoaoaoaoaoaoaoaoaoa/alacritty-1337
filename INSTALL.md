# Install alacritty-1337

`alacritty-1337` is distributed from the
[GitHub repository](https://github.com/aoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoa/alacritty-1337).
The executable remains named `alacritty` for command, config, terminfo, and shell
compatibility.

## Release Artifacts

Release `v1.0.0` provides:

- a universal macOS disk image;
- a Windows x86_64 per-user MSI and portable executable;
- Linux desktop, AppStream, icon, manual, completion, and terminfo sources;
- GitHub's source archives; and
- `SHA256SUMS` covering every attached artifact.

Download all files from the
[v1.0.0 release](https://github.com/aoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoa/alacritty-1337/releases/tag/v1.0.0).
Verify an artifact from the same directory as `SHA256SUMS`:

```sh
sha256sum --check SHA256SUMS
```

## macOS

Open `alacritty-1337-v1.0.0.dmg` and move `alacritty-1337.app` to
`Applications`. The bundle contains native x86_64 and arm64 code.

The GitHub artifact is ad-hoc signed, not Apple Developer-ID signed or notarized.
Systems that require notarization can build from source instead; no
Gatekeeper-frictionless installation is claimed.

## Windows

Run `alacritty-1337-v1.0.0-installer.msi` for a per-user installation. It puts
`alacritty.exe` below `%LOCALAPPDATA%\Programs\alacritty-1337`, adds that directory
to the user's `PATH`, creates a Start Menu shortcut, and adds the
`Open alacritty-1337 here` Explorer actions. Uninstalling the MSI removes those
owned entries.

`alacritty-1337-v1.0.0-portable.exe` is the same application without an
installer. Rename it to `alacritty.exe` if command-name compatibility matters.

## Build From Source

The release requires Rust 1.97.1. Install CMake, pkg-config, FreeType,
Fontconfig, libxcb, and libxkbcommon development files for Linux builds. X11
also requires Xcursor, Xi, and Xrandr development files. macOS requires Xcode
command-line tools. Windows requires the MSVC Rust target and Visual Studio
Build Tools.

Acquire the exact release and select its toolchain:

```sh
git clone https://github.com/aoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoa/alacritty-1337.git
cd alacritty-1337
git checkout --detach v1.0.0
rustup toolchain install 1.97.1 --profile minimal
```

Build the locked release:

```sh
cargo +1.97.1 build --locked --release -p alacritty-1337 --bin alacritty
target_dir=$(cargo +1.97.1 metadata --locked --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
"$target_dir/release/alacritty" --version
```

The final command must print `alacritty-1337 1.0.0`. Cargo may place its target
directory outside the checkout; querying metadata avoids assuming otherwise.

Linux feature-specific builds are available when a host supports only one
display stack:

```sh
cargo +1.97.1 build --locked --release -p alacritty-1337 \
  --no-default-features --features x11
cargo +1.97.1 build --locked --release -p alacritty-1337 \
  --no-default-features --features wayland
```

FreeBSD remains best-effort source compatibility. It has no 1.0 release
artifact or canonical release gate.

## Arch Package

Arch users building on the target machine can produce the private native
package:

```sh
packaging/arch/build-package
```

The build uses fat LTO, one codegen unit, `target-cpu=native`, stripped symbols,
and zstd ultra level 22. It is intentionally machine-specific and should not be
redistributed as a generic x86_64 binary. Install it through pacman so every
global file remains package-owned:

```sh
sudo pacman -U packaging/arch/alacritty-1337-1.0.0-1-x86_64.pkg.tar.zst
```

The package provides and replaces `alacritty`; pacman will request confirmation
before replacing a distribution package.

## Configuration

The config filename and schema remain `alacritty.toml`. The first existing path
wins. On Unix, the principal search order is:

1. `$XDG_CONFIG_HOME/alacritty-1337/alacritty.toml`
2. `$XDG_CONFIG_HOME/alacritty/alacritty.toml` (legacy)
3. `$XDG_CONFIG_HOME/alacritty.toml` (legacy)
4. `$HOME/.config/alacritty-1337/alacritty.toml`
5. `$HOME/.config/alacritty/alacritty.toml` (legacy)
6. `$HOME/.alacritty.toml` (legacy)
7. `/etc/alacritty-1337/alacritty.toml`
8. `/etc/alacritty/alacritty.toml` (legacy)

Windows checks `%APPDATA%\alacritty-1337\alacritty.toml` before the legacy
`%APPDATA%\alacritty\alacritty.toml`. Existing users do not need to move their
configuration.

See `extra/man/alacritty.5.scd` for the complete format and
`extra/man/alacritty-bindings.5.scd` for bindings.

## Removal

- Arch: `sudo pacman -R alacritty-1337`
- Windows MSI: uninstall `alacritty-1337` from Installed Apps
- macOS: remove `alacritty-1337.app`
- portable/source builds: remove only the copied executable or dedicated build tree

Removal does not delete user configuration. Delete it separately only when that
is your intent.

Report defects at the
[fork issue tracker](https://github.com/aoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoaoa/alacritty-1337/issues).
