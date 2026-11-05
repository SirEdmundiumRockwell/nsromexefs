# nsromexefs

<p align="center">
  <strong>Extract Nintendo Switch <code>exefs</code> and <code>romfs</code> from NSP and XCI containers.</strong><br />
  Inspect titles, merge supported updates, verify output, and resume interrupted extractions.
</p>

<p align="center">
  <a href="https://github.com/SirEdmundiumRockwell/nsromexefs/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/SirEdmundiumRockwell/nsromexefs/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <img alt="Rust" src="https://img.shields.io/badge/Rust-1.82%2B-orange?logo=rust">
</p>

`nsromexefs` is a command-line tool for working with Nintendo Switch NSP and XCI containers. It identifies base games, updates, and DLC from their metadata (CNMT), rather than relying on filenames, and can merge supported BKTR updates with their base content.

> **Development note:** This project was created with the assistance of AI tools. The project maintainer is responsible for reviewing, testing, and maintaining the code. Third-party software and code are credited separately in [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

> **Status:** v0.1.0. The CLI is available; the Tauri desktop GUI is an early development build and is not yet included in release packages.

## Features

- Inspect NSP (PFS0) and XCI (HFS0) containers, including an XCI's internal update partition.
- Extract `exefs` and `romfs` from base games, updates, and DLC.
- Detect title type and version from CNMT metadata; input container order does not matter for base + update extraction.
- Merge supported BKTR RomFS updates against the base without writing a full intermediate decrypted NCA.
- Process multiple containers with `batch` and organize output by title and version with `--tree`.
- Verify output with SHA-256 manifests; resume interrupted extraction after checking existing files.
- Preview an extraction plan with `--dry-run`; output can also be emitted as JSON.

## Quick start

Download the archive for your operating system from [Releases](https://github.com/SirEdmundiumRockwell/nsromexefs/releases), extract it, and run:

```console
nsromexefs --help
```

Inspect a container:

```console
nsromexefs info "game.nsp" -k "prod.keys" -k "title.keys"
```

Extract a base game, update, or XCI:

```console
nsromexefs dump "game.nsp" -o output -k "prod.keys" -k "title.keys"
```

Extract a base and its update together (the tool identifies them by metadata):

```console
nsromexefs dump "game-base.nsp" "game-update.nsp" -o output -k "prod.keys" -k "title.keys"
```

Verify or resume an extraction:

```console
nsromexefs verify output --strict
nsromexefs dump "game.xci" --tree -o output --resume -k "prod.keys"
```

Use only keys obtained from your own console. The application does not include or distribute keys.

## Output

By default, extraction writes `exefs/`, `romfs/`, `SHA256SUMS`, and `manifest.json` to the selected destination. Use `--exefs-only` or `--romfs-only` to select a filesystem. `--tree` creates title/version folders, which is useful for containers holding multiple titles.

## Downloads and verification

Release archives are provided for Windows x64, Linux x64 (glibc), and macOS (Apple Silicon and Intel), when available. Each release includes `SHA256SUMS`; compare the archive's SHA-256 digest with its entry before use. Releases are built by GitHub Actions and published from reviewed draft releases.

## Build from source

Requirements: Rust 1.82 or newer for the CLI. From the repository root:

```console
cargo build --locked --release -p nsromexefs-cli
cargo test --workspace --locked
```

The desktop GUI uses Tauri 2 and Svelte 5. See [`crates/nsromexefs-gui/README.md`](crates/nsromexefs-gui/README.md) for its Node.js, Rust, and platform-specific prerequisites.

## Desktop GUI

The Tauri desktop application wraps the same extraction engine and runs entirely on your machine.

- **Bilingual interface:** switch between English and Spanish at any time; the choice is saved locally.
- **Appearance:** choose from four color palettes and four font styles. The default is *Classic Red and Cream* with the *Rounded* font, both inspired by classic console interfaces. No proprietary assets or fonts are bundled.
- **Event log:** recent actions, warnings, and errors are recorded on the device, with personal paths and key-file names redacted. The log keeps up to 250 entries and can be downloaded or cleared.
- **Credits in the app:** Settings links to the projects and documentation the format work builds on, and notes that the project was created with AI assistance.

Build and run the GUI from `crates/nsromexefs-gui`:

```console
npm install
npm run build:sidecar
npm run tauri dev
```

`npm run tauri build` produces a platform installer that includes the extraction engine as a bundled sidecar, so end users do not need Rust or a separate CLI installation.

## Documentation

- [Key file guide](docs/keys.md)
- [GUI development guide](crates/nsromexefs-gui/README.md)
- [Technical design (currently Spanish)](DISENO-nsromexefs.md)
- [Third-party notices and attributions](THIRD-PARTY-NOTICES.md)
- [MIT License](LICENSE)

## Credits and third-party code

This project builds on open-source tools, libraries, and publicly documented formats. Their authors, licenses, and applicable notices are listed in [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md); Rust dependencies are also recorded in `Cargo.lock`. Please consult those files when redistributing this project. No third-party project is implied to endorse nsromexefs.

## Legal and responsible use

This software does not include game content, keys, or console firmware. It is not affiliated with or endorsed by Nintendo. Use it only with content you are legally entitled to access, and comply with applicable laws and the terms governing your content. The user is responsible for their use of the software.

## License

MIT. See [`LICENSE`](LICENSE) and [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) for project and dependency attributions.
