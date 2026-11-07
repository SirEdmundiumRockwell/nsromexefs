# Fuzzing

This directory contains the [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
harness for `nsromexefs-core`. It is a **detached Cargo workspace** so normal
builds keep using stable Rust while fuzzing uses nightly and libFuzzer.

## Targets

| Target | Entry points |
| --- | --- |
| `containers` | `Xci::read`, `Pfs0::read`, `Hfs0::read_at` (including nested bases) |
| `nca` | `NcaHeader::read_at` with plaintext, encrypted, and offset headers |
| `metadata` | `Cnmt`, `Ticket`, `RomFsHeader`, `Ivfc`, and `BktrHeader` parsers |
| `romfs` | `RomFs::new` + `RomFs::tree` using the offsets declared by the header |
| `bktr` | `BktrTables::parse` with both tables in one framed input |
| `keys` | `KeySet::merge_str` for `prod.keys` and `title.keys` text |
| `sha256sums` | `parse_sha256sums` |

Every target asserts nothing: a crash, panic, hang, or out-of-memory report is
the failure signal.

## Seeds

`scripts/generate-seeds.mjs` writes synthetic, minimal files to
`corpus/<target>/`. The corpus is generated, not committed, and contains no
real keys or game content.

```sh
node fuzz/scripts/generate-seeds.mjs
```

## Running locally

Requirements:

- Rust **nightly** (`rustup toolchain install nightly`)
- `cargo install cargo-fuzz --locked`
- Node.js 22 or newer for the seed generator

From the repository root:

```sh
node fuzz/scripts/generate-seeds.mjs
cargo fuzz build
cargo fuzz run containers
cargo fuzz run nca -- -max_total_time=60
```

`cargo fuzz` finds this directory automatically. To run a single target for a
bounded time:

```sh
cargo fuzz run metadata -- -max_total_time=120 -timeout=10 -rss_limit_mb=4096
```

Crashes are written to `artifacts/<target>/`. Reproduce one with:

```sh
cargo fuzz run <target> artifacts/<target>/<crash-file>
```

> Windows: libFuzzer targets cannot be linked with the local MSVC/GNU stable
> toolchain used for normal builds. Use WSL or the Linux CI workflow for real
> fuzzing. `cargo check --manifest-path fuzz/Cargo.toml --bins` is still useful
> to validate the harness compiles.

## CI

`.github/workflows/fuzz.yml` runs on changes to the core crate or this
directory, and once a week on a schedule:

- builds every target with nightly,
- runs each target for a short bounded time on pull requests and pushes,
- runs longer sessions on the weekly schedule,
- uploads `artifacts/` when a target fails.

`cargo-fuzz` itself is not required by the normal CI jobs in `ci.yml`.
