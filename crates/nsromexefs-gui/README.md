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
npm run check
npm run tauri -- dev
```

To preview only the frontend in a browser (native file dialogs and extraction require the desktop app):

```console
npm run dev
```

The GUI searches for `nsromexefs` beside its executable and in parent `target/debug` and `target/release` directories. If it cannot find the CLI, open **Settings** and select the binary built with:

```console
cargo build --release -p nsromexefs-cli
```

To build only the frontend or check its Svelte/TypeScript diagnostics:

```console
npm run build
npm run check
```

## Project status and credits

The GUI is an early development build and is not included in the v0.1.0 release packages. This project was created with assistance from AI tools; the maintainer reviews and maintains the code. See the repository's [third-party notices](../../THIRD-PARTY-NOTICES.md) for attribution and license information.
