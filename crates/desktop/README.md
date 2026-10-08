# Desktop

Rust installed, commands from the repo root:

```sh
cargo run -r -p desktop
cargo run -r -p desktop -- --print-config  # endpoints + storage paths
```

You'll need Xcode on macOS, or a Visual Studio C++/Windows SDK developer shell on Windows. Debian/Ubuntu deps:

```sh
sudo apt-get install -y libasound2-dev libfontconfig-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libvulkan-dev libclang-dev llvm \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxcb1-dev
```

No env setup needed for hosted sync. Works signed out too; signing in doesn't upload your local workspace. One process per saved session, please. Credentials go in the OS credential store; the local database isn't encrypted.

Local-only (Nushell):

```nu
with-env { SUBROUTINE_LITE_OFFLINE_ONLY: 'true' } { cargo run -r -p desktop }
```

Uses the local workspace, not your account cache. Leaves saved credentials alone; running normally can resume sync.

## Bundles

Build on the target OS. Release packaging needs this for license notices:

```sh
cargo install cargo-about --version 0.9.2 --locked --features cli
```

### macOS

Xcode with Metal tools, Python 3.9+:

```sh
cargo install cargo-bundle --version 0.11.0 --locked
script/bundle-mac --release --install --open
# or just make a .pkg:
script/bundle-mac --release --installer
```

`--install` replaces the app in `~/Applications`; the `.pkg` targets `/Applications`. Ad-hoc signed app, unsigned installer, no notarization.

### Windows

PowerShell 7, Visual Studio C++/Windows SDK, Inno Setup 6.3+, Python 3.12+. Close the app first.

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File ./script/bundle-windows.ps1 -Install
```

Omit `-Install` to just build. Per-user install, unsigned, no bundled Visual C++ runtime.

### Linux

Python 3, glibc, libraries above. Dependencies must already be cached.

```sh
script/bundle-linux --release
# extract the archive, then:
./subroutine-lite/install
```
