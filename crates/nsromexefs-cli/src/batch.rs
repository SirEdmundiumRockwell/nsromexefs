//! Subcomando `batch`: extrae todos los NSP/XCI de una carpeta.
//!
//! Un fallo por título no aborta el lote (§19.5 del diseño): se registra, se
//! continúa con el siguiente y el código de salida resume el conjunto.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use nsromexefs_core::{Error, KeySet, MetaType};

use crate::container::probe_titles;
use crate::dump::{dump_paths, DumpOptions, DumpReport};
use crate::util::{exit_code, load_keys_lenient};

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    title_id: String,
    version: u32,
}

#[derive(Default)]
struct Group {
    bases: Vec<Candidate>,
    updates: Vec<Candidate>,
    dlcs: BTreeMap<String, Candidate>,
}

impl Group {
    fn add(&mut self, meta_type: MetaType, candidate: Candidate) {
        match meta_type {
            MetaType::Application => self.bases.push(candidate),
            MetaType::Patch => self.updates.push(candidate),
            MetaType::AddOnContent => {
                let replace = self
                    .dlcs
                    .get(&candidate.title_id)
                    .is_none_or(|current| candidate.version > current.version);
                if replace {
                    self.dlcs.insert(candidate.title_id.clone(), candidate);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn cmd_batch(
    dir: &Path,
    out: Option<&Path>,
    explicit_keys: &[PathBuf],
    opts: DumpOptions,
) -> Result<(), Error> {
    if !dir.is_dir() {
        return Err(Error::Invalid(format!(
            "{}: no es una carpeta",
            dir.display()
        )));
    }
    let keys = load_keys_lenient(explicit_keys).ok_or(Error::MissingHeaderKey)?;
    if keys.header_key().is_none() {
        return Err(Error::MissingHeaderKey);
    }

    let scan = scan_media(dir)?;
    if scan.media.is_empty() {
        return Err(Error::Invalid(format!(
            "{}: no se encontró ningún .nsp ni .xci",
            dir.display()
        )));
    }

    let root = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("nsromexefs-out"));

    // Primera pasada: solo CNMT, para agrupar base/update/DLC del mismo juego.
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    let mut lines: Vec<BatchLine> = Vec::new();
    for path in &scan.media {
        match probe_titles(path, &keys) {
            Ok(probes) => {
                for probe in probes {
                    let candidate = Candidate {
                        path: path.clone(),
                        title_id: probe.title_id,
                        version: probe.version,
                    };
                    if matches!(
                        probe.meta_type,
                        MetaType::Application | MetaType::Patch | MetaType::AddOnContent
                    ) {
                        groups
                            .entry(probe.app_id)
                            .or_default()
                            .add(probe.meta_type, candidate);
                    } else {
                        lines.push(BatchLine::Skipped {
                            path: path.clone(),
                            reason: format!("{}: fuera de alcance", probe.meta_type.short_label()),
                        });
                    }
                }
            }
            Err(error) => lines.push(BatchLine::Failed {
                inputs: vec![path.clone()],
                error,
            }),
        }
    }

    // Segunda pasada: extracción con layout por título/versión.
    let dump_opts = DumpOptions { tree: true, ..opts };
    for group in groups.values() {
        let base = best(&group.bases);
        let update = best(&group.updates);

        match base {
            Some(base) => {
                let mut inputs = vec![base.path.clone()];
                if let Some(update) = update {
                    if update.path != base.path {
                        inputs.push(update.path.clone());
                    }
                }
                run_one(&inputs, &root, &keys, dump_opts, &mut lines);
            }
            None => {
                if let Some(update) = update {
                    // Sin base no hay nada que fusionar: se registra y se sigue.
                    lines.push(BatchLine::Failed {
                        inputs: vec![update.path.clone()],
                        error: Error::MissingBase,
                    });
                }
            }
        }

        for dlc in group.dlcs.values() {
            run_one(
                std::slice::from_ref(&dlc.path),
                &root,
                &keys,
                dump_opts,
                &mut lines,
            );
        }
    }

    let ok = lines
        .iter()
        .filter(|line| matches!(line, BatchLine::Done(_)))
        .count();
    let failures: Vec<&Error> = lines
        .iter()
        .filter_map(|line| match line {
            BatchLine::Failed { error, .. } => Some(error),
            _ => None,
        })
        .collect();
    let failed = failures.len();

    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&batch_json(&root, &lines, ok, failed, &scan.skipped))
                .unwrap()
        );
    } else {
        for line in &lines {
            match line {
                BatchLine::Done(report) => {
                    let version = report
                        .display_version
                        .clone()
                        .unwrap_or_else(|| report.version_raw.to_string());
                    println!(
                        "  {:<5} {:<5} {}  v{:<10} {}",
                        "ok",
                        report.meta_type.short_label(),
                        report.title_id,
                        version,
                        report.out_dir.display()
                    );
                }
                BatchLine::Failed { inputs, error } => {
                    println!(
                        "  {:<5} {}  [{}] {}",
                        "error",
                        join_paths(inputs),
                        error.code(),
                        error
                    );
                    for warning in error_warnings(error) {
                        println!("        {warning}");
                    }
                }
                BatchLine::Skipped { path, reason } => {
                    println!("  {:<5} {}  {reason}", "omitido", path.display());
                }
            }
        }
        println!();
        println!(
            "lote       {} de {} título(s) · {} error(es) · salida {}",
            ok,
            ok + failed,
            failed,
            root.display()
        );
        if !scan.skipped.is_empty() {
            println!(
                "  omitidos   {} archivo(s) comprimido(s) (.nsz/.xcz): descomprímelos primero",
                scan.skipped.len()
            );
        }
    }

    if failed == 0 {
        return Ok(());
    }

    let codes: BTreeSet<u8> = failures.iter().map(|err| exit_code(err)).collect();
    let exit = if codes.len() == 1 {
        *codes.iter().next().unwrap()
    } else {
        1
    };
    Err(Error::BatchFailed {
        failed,
        total: ok + failed,
        exit,
    })
}

fn run_one(
    inputs: &[PathBuf],
    root: &Path,
    keys: &KeySet,
    opts: DumpOptions,
    lines: &mut Vec<BatchLine>,
) {
    match dump_paths(inputs, keys, Some(root), opts) {
        Ok(report) => lines.push(BatchLine::Done(Box::new(report))),
        Err(error) => lines.push(BatchLine::Failed {
            inputs: inputs.to_vec(),
            error,
        }),
    }
}

enum BatchLine {
    Done(Box<DumpReport>),
    Failed { inputs: Vec<PathBuf>, error: Error },
    Skipped { path: PathBuf, reason: String },
}

fn batch_json(
    root: &Path,
    lines: &[BatchLine],
    ok: usize,
    failed: usize,
    skipped: &[PathBuf],
) -> serde_json::Value {
    serde_json::json!({
        "root": root.display().to_string(),
        "ok": ok,
        "failed": failed,
        "titles": lines.iter().map(|line| match line {
            BatchLine::Done(report) => serde_json::json!({
                "status": "ok",
                "report": report.to_json(),
            }),
            BatchLine::Failed { inputs, error } => serde_json::json!({
                "status": "error",
                "inputs": inputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "code": error.code(),
                "error": error.to_string(),
            }),
            BatchLine::Skipped { path, reason } => serde_json::json!({
                "status": "skipped",
                "path": path.display().to_string(),
                "reason": reason,
            }),
        }).collect::<Vec<_>>(),
        "compressed": skipped.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    })
}

/// La variante con más versión; en empate se conserva la primera encontrada.
fn best(candidates: &[Candidate]) -> Option<&Candidate> {
    candidates.iter().reduce(|best, candidate| {
        if candidate.version > best.version {
            candidate
        } else {
            best
        }
    })
}

fn join_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn error_warnings(error: &Error) -> Vec<String> {
    error
        .hint()
        .map(|hint| vec![format!("pista: {hint}")])
        .unwrap_or_default()
}

struct Scan {
    media: Vec<PathBuf>,
    skipped: Vec<PathBuf>,
}

/// Recorre la carpeta (subcarpetas incluidas) sin seguir enlaces simbólicos.
fn scan_media(dir: &Path) -> Result<Scan, Error> {
    let mut media = Vec::new();
    let mut compressed = Vec::new();

    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries = std::fs::read_dir(&current)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            match path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("nsp" | "xci") => media.push(path),
                Some("nsz" | "xcz") => compressed.push(path),
                _ => {}
            }
        }
    }

    media.sort();
    compressed.sort();
    Ok(Scan {
        media,
        skipped: compressed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(title_id: &str, version: u32) -> Candidate {
        Candidate {
            path: PathBuf::from(format!("{title_id}.nsp")),
            title_id: title_id.to_string(),
            version,
        }
    }

    #[test]
    fn best_picks_highest_version_and_keeps_first_on_tie() {
        let candidates = vec![candidate("A", 1), candidate("B", 3), candidate("C", 3)];
        assert_eq!(best(&candidates).unwrap().title_id, "B");
        assert!(best(&[]).is_none());
    }

    #[test]
    fn dlc_keeps_highest_version_per_title() {
        let mut group = Group::default();
        group.add(MetaType::AddOnContent, candidate("0100DLC1", 0));
        group.add(MetaType::AddOnContent, candidate("0100DLC1", 65536));
        group.add(MetaType::AddOnContent, candidate("0100DLC2", 0));
        assert_eq!(group.dlcs.len(), 2);
        assert_eq!(group.dlcs["0100DLC1"].version, 65536);
    }

    #[test]
    fn base_and_update_are_kept_separately() {
        let mut group = Group::default();
        group.add(MetaType::Application, candidate("0100BASE", 0));
        group.add(MetaType::Patch, candidate("0100PATCH", 65536));
        assert_eq!(group.bases.len(), 1);
        assert_eq!(group.updates.len(), 1);
        assert!(group.dlcs.is_empty());
    }

    #[test]
    fn scan_finds_media_recursively_and_ignores_compressed() {
        let base = std::env::temp_dir().join(format!(
            "nsromexefs-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let nested = base.join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(base.join("a.nsp"), b"").unwrap();
        std::fs::write(nested.join("b.XCI"), b"").unwrap();
        std::fs::write(nested.join("c.nsz"), b"").unwrap();
        std::fs::write(nested.join("notas.txt"), b"").unwrap();

        let scan = scan_media(&base).unwrap();
        assert_eq!(scan.media.len(), 2);
        assert!(scan.media.iter().any(|p| p.ends_with("b.XCI")));
        assert_eq!(scan.skipped.len(), 1);
        assert!(scan.skipped[0].ends_with("c.nsz"));

        std::fs::remove_dir_all(&base).unwrap();
    }
}
