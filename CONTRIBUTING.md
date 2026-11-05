# Contributing to nsromexefs

Thanks for taking the time to improve the project. This guide covers the local workflow and the few rules that keep contributions reviewable.

## Ground rules

- **Never include keys, game content, or console firmware.** Do not commit `prod.keys`, `title.keys`, `*.nsp`, `*.xci`, or extracted game data, and do not paste their contents into issues, pull requests, or logs.
- Use only content and keys you are legally entitled to use. See the [key file guide](docs/keys.md).
- Keep changes focused. Separate unrelated refactors from behavior changes.
- User-facing documentation is written in English. The technical design in `docs/design.es.md` is kept in Spanish as a historical reference.
- By contributing, you agree that your changes are licensed under the project's [MIT License](LICENSE).

## Development setup

### CLI and core

Requirements: Rust 1.82 or newer.

```console
cargo build --locked --release -p nsromexefs-cli
cargo test --locked -p nsromexefs-core -p nsromexefs-cli
```

Workspace-wide checks also build the Tauri GUI crate, which requires the sidecar described in the next section.

### Desktop GUI

Requirements: Node.js 22 or newer, Rust 1.90 or newer, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.

```console
cd crates/nsromexefs-gui
npm ci
npm run build:sidecar
npm run check
npm run tauri dev
```

`build:sidecar` compiles the CLI and copies it to `src-tauri/binaries/`. Tauri requires that file when building or testing the GUI crate, so run it again after changing the CLI. The generated binary is ignored by Git.

## Checks before opening a pull request

Run the same checks as CI:

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

npm --prefix crates/nsromexefs-gui run check
npm --prefix crates/nsromexefs-gui run build
```

The workspace includes the Tauri GUI crate. Run the GUI setup (`npm ci` and `npm run build:sidecar`) before workspace-wide Clippy or tests; otherwise scope those commands to `-p nsromexefs-core -p nsromexefs-cli`.

If you change the GUI or the bundling workflow, also verify that `npm run tauri build` still produces an installer.

## Pull requests

- Explain the problem and the chosen approach.
- Mention the platforms and commands you tested.
- Update the relevant documentation when behavior or requirements change.
- Keep generated files, build output, and local test data out of the commit.
- AI-assisted contributions are welcome. The contributor remains responsible for reviewing, testing, and understanding the change.

## Reporting bugs and security issues

- Use the [issue templates](https://github.com/SirEdmundiumRockwell/nsromexefs/issues/new/choose) for reproducible bugs and feature requests.
- Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md). Do not open a public issue for a security problem.

## Questions

Open a discussion or an issue with the `question` label. Do not attach ROMs, keys, or personal paths.
