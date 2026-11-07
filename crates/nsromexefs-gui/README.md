# nsromexefs Desktop GUI

The desktop app is built with Tauri 2 and Svelte 5. It provides a graphical workflow for selecting NSP/XCI inputs, validating key files, reviewing a dry-run plan, starting or resuming extraction, and viewing logs and results. Extraction is delegated to the `nsromexefs` CLI; the GUI does not implement a separate parser or extraction engine.

The interface supports English and Spanish. Use the language control in the top bar; your selection is saved locally in the app.

**Appearance:** Settings offers four color palettes and four font styles, saved on the device. The default is *Classic Red and Cream* with the *Rounded* font.

**Diagnostics:** the Events tab keeps a local log of recent actions, warnings, and errors (up to 250 entries). Personal paths and key-file names are redacted, and the log can be downloaded or cleared.

## Requirements

- Node.js 22 or newer and npm.
- Rust 1.90 or newer for Tauri (MSVC toolchain on Windows). The CLI/core have an MSRV of Rust 1.82.
- Windows: Visual Studio Build Tools with C++ and the WebView2 Runtime.
- Linux: Tauri 2 system dependencies, including WebKitGTK. See the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Run in development

From this directory:

```console
npm ci
npm run build:sidecar
npm run check
npm run tauri -- dev
```

`build:sidecar` compiles `nsromexefs-cli` in release mode and copies it to `src-tauri/binaries/` with the host target triple, which Tauri requires for the bundled engine. Run it again after changing the CLI. The GUI also finds local `target/debug` and `target/release` builds during development.

To preview only the frontend in a browser (native file dialogs and extraction require the desktop app):

```console
npm run dev
```

The GUI searches for `nsromexefs` beside its executable and in parent `target/debug` and `target/release` directories. Release builds bundle the CLI as a Tauri sidecar, so the installed app finds its engine automatically. In development, build the workspace first if needed:

```console
cargo build --release -p nsromexefs-cli
```

To build only the frontend or check its Svelte/TypeScript diagnostics:

```console
npm run build
npm run check
```

## Build the installer

Prebuilt Windows installers are attached to each [GitHub release](https://github.com/SirEdmundiumRockwell/nsromexefs/releases); they bundle the GUI and the CLI sidecar. Build locally when you want a different platform, a custom engine, or to test changes.

`npm run tauri -- build` runs the frontend build and `npm run build:sidecar` through `beforeBuildCommand`, then writes the installer to the workspace `target/release/bundle` directory. The sidecar (`src-tauri/binaries/nsromexefs-<target-triple>`) is generated during the build and is not committed. Users do not need Rust or a separate CLI after installing.

The optional CLI path in **Settings** remains available for development or for using an alternative engine build.

### Windows Smart App Control

The generated installers and executables are not code-signed. Windows Smart App Control may block them. If that happens, confirm the file came from your own build or a trusted release and verify its SHA-256 checksum. You can turn off Smart App Control under **Windows Security → App & browser control → Smart App Control**; Microsoft only allows turning it back on after a clean install or reset of Windows. Only disable it for files you trust. This project does not sign its binaries.

## Project status and credits

The GUI shares the `nsromexefs` extraction engine, which is bundled as a sidecar in the Windows installers. Prebuilt Windows installers are attached to each GitHub release; other platforms build from source. This project was created with assistance from AI tools; the maintainer reviews and maintains the code. See the repository's [third-party notices](../../THIRD-PARTY-NOTICES.md) for attribution and license information.
