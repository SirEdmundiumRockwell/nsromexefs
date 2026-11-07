# Changelog

All notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-11-07

### Added

- Version synchronization script (`scripts/version.mjs`) and a CI consistency check.
- Dependabot configuration for Cargo, npm, and GitHub Actions.
- `CHANGELOG.md`.

### Changed

- The release workflow now builds and attaches Windows MSI and NSIS installers for `v*` tags.
- Release workflow actions updated (`checkout@v7`, `upload-artifact@v7`, `download-artifact@v8`).
- Rust dependencies updated to the RustCrypto 2025 releases: `aes 0.9`, `ctr 0.10`, and `sha2 0.11`, with the crypto code migrated to the new cipher API.
- JavaScript dependencies updated (Vite 8 and `@sveltejs/vite-plugin-svelte` 7); TypeScript stays on 6.x until `svelte-check` supports 7.

### Fixed

- Dependabot no longer proposes TypeScript major updates that break `svelte-check`.

## [0.1.0] - 2026-11-07

### Added

- `nsromexefs` CLI for inspecting and extracting `exefs` and `romfs` from NSP/XCI containers.
- Base, update, and DLC handling with BKTR update merging.
- Batch extraction, tree layout, dry-run plans, resume, and SHA-256 verification.
- Tauri 2 desktop GUI with a bilingual English/Spanish interface, selectable palettes and fonts, and a persistent local event log.
- CLI sidecar bundled into the desktop app.
- Windows MSI and NSIS installers.
- CLI archives for Windows x64, Linux x64, macOS Apple Silicon, and macOS Intel.
- English user-facing CLI and core messages, with Spanish localization in the GUI.
- Documentation set: README, key guide, technical design, contributing, security, and third-party notices.
