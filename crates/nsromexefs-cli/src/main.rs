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
    about = "Extrae exefs y romfs desde NSP/XCI de Nintendo Switch",
    long_about = None,
    disable_help_subcommand = true
)]
struct Cli {
    /// Archivos de claves (repetible). Si se omite, se busca prod.keys/title.keys.
    #[arg(short = 'k', long = "keys", global = true, value_name = "FILE")]
    keys: Vec<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Muestra el contenido de un NSP/XCI sin extraer nada
    Info {
        /// Archivos .nsp o .xci
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Salida legible por máquina
        #[arg(long)]
        json: bool,
    },
    /// Extrae exefs y romfs de un NSP/XCI base, DLC o base + update
    Dump {
        /// Archivos .nsp/.xci: la base y, opcionalmente, su update (en cualquier orden)
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Directorio de salida (por defecto ./<title_id>)
        #[arg(short = 'o', long = "out", value_name = "DIR")]
        out: Option<PathBuf>,
        /// Organiza la salida como <app_id>/v<versión> (obligatorio en lote)
        #[arg(long)]
        tree: bool,
        /// Reanuda una extracción incompleta, verificando hash y tamaño de cada archivo
        #[arg(long, conflicts_with = "dry_run")]
        resume: bool,
        /// Fuerza una versión (visible 1.1.0, CNMT 2.0.0 o entero 131072)
        #[arg(long, value_name = "V")]
        version: Option<String>,
        /// Extrae solo exefs
        #[arg(long, conflicts_with = "romfs_only")]
        exefs_only: bool,
        /// Extrae solo romfs
        #[arg(long)]
        romfs_only: bool,
        /// Analiza y muestra el plan sin escribir nada
        #[arg(long)]
        dry_run: bool,
        /// Sobrescribe una extracción anterior
        #[arg(long)]
        force: bool,
        /// Lista cada archivo extraído
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Salida legible por máquina
        #[arg(long)]
        json: bool,
    },
    /// Extrae todos los NSP/XCI de una carpeta, con layout por título/versión
    Batch {
        /// Carpeta que contiene los .nsp/.xci (se recorre recursivamente)
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        /// Carpeta raíz de salida (por defecto ./nsromexefs-out)
        #[arg(short = 'o', long = "out", value_name = "DIR")]
        out: Option<PathBuf>,
        /// Extrae solo exefs
        #[arg(long, conflicts_with = "romfs_only")]
        exefs_only: bool,
        /// Extrae solo romfs
        #[arg(long)]
        romfs_only: bool,
        /// Analiza y muestra el plan sin escribir nada
        #[arg(long)]
        dry_run: bool,
        /// Sobrescribe extracciones anteriores
        #[arg(long)]
        force: bool,
        /// Reanuda las extracciones incompletas del lote
        #[arg(long, conflicts_with = "dry_run")]
        resume: bool,
        /// Lista cada archivo extraído
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Salida legible por máquina
        #[arg(long)]
        json: bool,
    },
    /// Verifica un árbol extraído contra su SHA256SUMS
    Verify {
        /// Árbol con exefs/ y romfs/, o carpeta que contiene varios árboles
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        /// Falla también ante archivos sobrantes o un `main` sin NSO0
        #[arg(long)]
        strict: bool,
        /// Salida legible por máquina
        #[arg(long)]
        json: bool,
    },
    /// Valida y sanea archivos de claves
    Keys {
        /// (Compatibilidad) valida el keyset; es el comportamiento por defecto
        #[arg(long)]
        check: bool,
        /// Archivos prod.keys / title.keys (si se omiten, se usan los de -k)
        #[arg(value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Salida legible por máquina
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
                    eprintln!("  pista: {hint}");
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
            path: PathBuf::from("(ninguno)"),
            reason: "no se indicó ningún archivo de claves; usa -k prod.keys".into(),
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
        let status = if present { "OK" } else { "falta" };
        let extra = match (family, max) {
            ("titlekek" | "master_key" | "key_area_key_application", Some(rev)) if present => {
                format!("  hasta revisión {rev}")
            }
            _ => String::new(),
        };
        println!("  {:<28} {:<6}{}", family, status, extra);
    }
    println!("  {:<28} {} entradas", "titlekeys", set.titlekey_count());
    println!("  {:<28} {}", "claves totales", set.key_count());

    if set.warnings().is_empty() {
        println!("  avisos: ninguno");
    } else {
        println!("  avisos:");
        for warning in set.warnings() {
            println!("    {warning}");
        }
    }
    Ok(())
}
