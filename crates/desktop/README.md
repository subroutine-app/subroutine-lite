# Desktop

Rust installed, commands from the repo root:

```sh
cargo run -r -p desktop
cargo run -r -p desktop -- --print-config  # endpoints + storage paths
```

You'll need Xcode on macOS, or Visual Studio C++ tools and the Windows SDK on Windows. The Windows resource compiler and its include paths are detected automatically. Debian/Ubuntu deps:

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

## Starting view and empty workspaces

A fresh installation opens on Timeline at now. Choosing Home or another primary view remembers it for the next launch; opening a library, searching, or hovering a tab while dragging does not replace that choice. Existing settings retain the previously selected main view. Navigation order stays fixed.

Home, Upcoming, Queue, and Focus use their normal card layout even when empty. A plus slot becomes an inline draft in the same position; nothing is saved until the draft is submitted or finished. Empty Upcoming shows only Today with a static create slot; populated agendas use hoverable Add item controls and show additional dates only when they contain items or markers. The separate item creator remains available through New item. An account cache still awaiting its first snapshot is shown as loading or unavailable, not as an empty agenda.

## Regional holidays

Fresh local workspaces offer a region selector with **Add holidays** and **Skip**. The system locale suggests a country, never a state or province. Adding creates editable date-only markers for **2000–2050** from the bundled snapshot; the choice is saved once, so edits and deletions survive restarts. Existing workspaces and signed-in accounts are not seeded.

The catalog covers 31 selections across 10 countries. It is compiled into a static hash map, with no runtime Python or network access. See [Bundled holidays](../../docs/holidays.md) for coverage, accuracy limitations, and the pinned generator used to refresh build inputs.

## Frosted background

**Settings → Appearance → Frosted background** enables native behind-window blur on macOS and acrylic on Windows 10 version 1809 (build 17763) or newer. The wash lightens when the window loses focus and returns smoothly when activated.

Windows uses a solid background when Transparency effects are off, High Contrast or Energy Saver is on, the session is remote, or the required APIs are unavailable. Changes to transparency, contrast, and energy-saving preferences apply without restarting. macOS respects Reduce Transparency; other platforms remain opaque.

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
