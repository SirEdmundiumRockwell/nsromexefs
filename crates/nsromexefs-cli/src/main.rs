//! CLI de `nsromexefs`.
//!
//! `info` inspecciona contenedores; `dump` extrae `exefs`/`romfs` de un base,
//! DLC o base+update (fusionando BKTR); `tree`/`batch` organizan lotes; `keys`
//! valida el keyset.

mod batch;
mod container;
mod dump;
mod info;
mod state;
mod util;
mod verify;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nsromexefs_core::{Error, KeySet};

use crate::dump::DumpOptions;
use crate::util::{error_json, exit_code, merge_paths};

#[derive(Parser)]
#[command(
    name = "nsromexefs",
    version,
    about = "Extract exefs and romfs from Nintendo Switch NSP/XCI containers",
    long_about = None,
    disable_help_subcommand = true
)]
struct Cli {
    /// Key files (repeatable). If omitted, prod.keys/title.keys are searched for.
    #[arg(short = 'k', long = "keys", global = true, value_name = "FILE")]
    keys: Vec<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the contents of an NSP/XCI without extracting anything
    Info {
        /// .nsp or .xci files
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Extract exefs and romfs from a base NSP/XCI, DLC, or base + update
    Dump {
        /// .nsp/.xci files: the base and, optionally, its update (in any order)
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Output directory (defaults to ./<title_id>)
        #[arg(short = 'o', long = "out", value_name = "DIR")]
        out: Option<PathBuf>,
        /// Organize output as <app_id>/v<version> (required in batch mode)
        #[arg(long)]
        tree: bool,
        /// Resume an incomplete extraction, verifying each file's hash and size
        #[arg(long, conflicts_with = "dry_run")]
        resume: bool,
        /// Force a version (display 1.1.0, CNMT 2.0.0, or raw integer 131072)
        #[arg(long, value_name = "V")]
        version: Option<String>,
        /// Extract exefs only
        #[arg(long, conflicts_with = "romfs_only")]
        exefs_only: bool,
        /// Extract romfs only
        #[arg(long)]
        romfs_only: bool,
        /// Analyze and show the plan without writing anything
        #[arg(long)]
        dry_run: bool,
        /// Overwrite a previous extraction
        #[arg(long)]
        force: bool,
        /// List each extracted file
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Extract every NSP/XCI in a folder, organized by title/version
    Batch {
        /// Folder containing the .nsp/.xci files (scanned recursively)
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        /// Output root folder (defaults to ./nsromexefs-out)
        #[arg(short = 'o', long = "out", value_name = "DIR")]
        out: Option<PathBuf>,
        /// Extract exefs only
        #[arg(long, conflicts_with = "romfs_only")]
        exefs_only: bool,
        /// Extract romfs only
        #[arg(long)]
        romfs_only: bool,
        /// Analyze and show the plan without writing anything
        #[arg(long)]
        dry_run: bool,
        /// Overwrite previous extractions
        #[arg(long)]
        force: bool,
        /// Resume incomplete extractions in the batch
        #[arg(long, conflicts_with = "dry_run")]
        resume: bool,
        /// List each extracted file
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Verify an extracted tree against its SHA256SUMS
    Verify {
        /// Tree containing exefs/ and romfs/, or a folder with multiple trees
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        /// Also fail on extra files or a `main` without NSO0
        #[arg(long)]
        strict: bool,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Validate and sanitize key files
    Keys {
        /// (Compatibility) validate the keyset; this is the default behavior
        #[arg(long)]
        check: bool,
        /// prod.keys / title.keys files (if omitted, the -k files are used)
        #[arg(value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            // Un lote ya imprimió su resumen (incluido el JSON): no se duplica.
            if wants_json(&cli.command) && matches!(err, Error::BatchFailed { .. }) {
                return ExitCode::from(exit_code(&err));
            }
            if wants_json(&cli.command) {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&error_json(&err)).unwrap()
                );
            } else {
                eprintln!("error [{}]: {err}", err.code());
                if let Some(hint) = err.hint() {
                    eprintln!("  hint: {hint}");
                }
            }
            ExitCode::from(exit_code(&err))
        }
    }
}

fn wants_json(command: &Command) -> bool {
    match command {
        Command::Info { json, .. }
        | Command::Dump { json, .. }
        | Command::Batch { json, .. }
        | Command::Verify { json, .. }
        | Command::Keys { json, .. } => *json,
    }
}

fn run(cli: &Cli) -> Result<u8, Error> {
    match &cli.command {
        Command::Info { files, json } => {
            info::cmd_info(files, &cli.keys, *json)?;
            Ok(0)
        }
        Command::Dump {
            files,
            out,
            tree,
            resume,
            version,
            exefs_only,
            romfs_only,
            dry_run,
            force,
            verbose,
            json,
        } => {
            dump::cmd_dump(
                files,
                &cli.keys,
                out.as_deref(),
                DumpOptions {
                    exefs_only: *exefs_only,
                    romfs_only: *romfs_only,
                    dry_run: *dry_run,
                    force: *force,
                    verbose: *verbose,
                    json: *json,
                    tree: *tree,
                    version: version.clone(),
                    resume: *resume,
                },
            )?;
            Ok(0)
        }
        Command::Batch {
            dir,
            out,
            exefs_only,
            romfs_only,
            dry_run,
            force,
            resume,
            verbose,
            json,
        } => {
            batch::cmd_batch(
                dir,
                out.as_deref(),
                &cli.keys,
                DumpOptions {
                    exefs_only: *exefs_only,
                    romfs_only: *romfs_only,
                    dry_run: *dry_run,
                    force: *force,
                    verbose: *verbose,
                    json: *json,
                    // El lote siempre usa layout por título/versión.
                    tree: true,
                    version: None,
                    resume: *resume,
                },
            )?;
            Ok(0)
        }
        Command::Verify { dir, strict, json } => verify::cmd_verify(dir, *strict, *json),
        Command::Keys { files, json, .. } => {
            cmd_keys(files, &cli.keys, *json)?;
            Ok(0)
        }
    }
}

// ---------------------------------------------------------------------------
// keys
// ---------------------------------------------------------------------------

fn cmd_keys(files: &[PathBuf], global: &[PathBuf], json: bool) -> Result<(), Error> {
    let paths = merge_paths(files, global);
    if paths.is_empty() {
        return Err(Error::KeysetInvalid {
            path: PathBuf::from("(none)"),
            reason: "no key file was provided; use -k prod.keys".into(),
        });
    }
    let set = KeySet::load(&paths)?;

    if json {
        let families: serde_json::Map<String, serde_json::Value> = set
            .families()
            .into_iter()
            .map(|(name, present)| (name.to_string(), serde_json::Value::Bool(present)))
            .collect();
        let out = serde_json::json!({
            "files": set.sources().iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "max_revision": set.max_revision(),
            "titlekeys": set.titlekey_count(),
            "keys": set.key_count(),
            "families": families,
            "warnings": set.warnings().iter().map(|w| w.message.clone()).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return Ok(());
    }

    for source in set.sources() {
        println!("{}", source.display());
    }
    let max = set.max_revision();
    for (family, present) in set.families() {
        let status = if present { "OK" } else { "missing" };
        let extra = match (family, max) {
            ("titlekek" | "master_key" | "key_area_key_application", Some(rev)) if present => {
                format!("  up to revision {rev}")
            }
            _ => String::new(),
        };
        println!("  {:<28} {:<6}{}", family, status, extra);
    }
    println!("  {:<28} {} entries", "titlekeys", set.titlekey_count());
    println!("  {:<28} {}", "total keys", set.key_count());

    if set.warnings().is_empty() {
        println!("  warnings: none");
    } else {
        println!("  warnings:");
        for warning in set.warnings() {
            println!("    {warning}");
        }
    }
    Ok(())
}
