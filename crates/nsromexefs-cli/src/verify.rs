//! Subcomando `verify`: relee un árbol extraído y lo compara con su
//! `SHA256SUMS`.
//!
//! Acepta tanto un árbol concreto como una carpeta de lotes: en ese caso busca
//! todos los `SHA256SUMS` de forma recursiva. `--strict` convierte los archivos
//! sobrantes y un `main` sin magic `NSO0` en fallos.

use std::fs;
use std::path::{Path, PathBuf};

use nsromexefs_core::{verify_tree, Error, VerifyReport};

use crate::util::human_size;

/// Devuelve el código de salida: 0 si todo cuadra, 5 si algo falla.
pub(crate) fn cmd_verify(dir: &Path, strict: bool, json: bool) -> Result<u8, Error> {
    if !dir.exists() {
        return Err(Error::Invalid(format!("{}: no existe", dir.display())));
    }
    if !dir.is_dir() {
        return Err(Error::Invalid(format!(
            "{}: se esperaba una carpeta con exefs/ y romfs/",
            dir.display()
        )));
    }

    let roots = find_roots(dir)?;
    if roots.is_empty() {
        return Err(Error::Invalid(format!(
            "{}: no se encontró ningún SHA256SUMS",
            dir.display()
        )));
    }

    let mut reports = Vec::new();
    for root in roots {
        reports.push(verify_tree(&root)?);
    }
    let failed = reports.iter().filter(|report| !report.ok(strict)).count();

    if json {
        let payload = serde_json::json!({
            "ok": failed == 0,
            "strict": strict,
            "roots": reports.len(),
            "failed": failed,
            "reports": reports.iter().map(|report| report_json(report, strict)).collect::<Vec<_>>(),
            "code": if failed == 0 { serde_json::Value::Null } else { serde_json::json!("E_VERIFY_FAILED") },
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
    } else {
        for (index, report) in reports.iter().enumerate() {
            if index > 0 {
                println!();
            }
            print_report(report, strict);
        }
    }

    Ok(if failed == 0 { 0 } else { 5 })
}

fn report_json(report: &VerifyReport, strict: bool) -> serde_json::Value {
    serde_json::json!({
        "root": report.root.display().to_string(),
        "ok": report.ok(strict),
        "issues": report.issues(strict),
        "listed": report.listed,
        "verified": report.verified,
        "bytes": report.bytes,
        "missing": report.missing,
        "mismatched": report.mismatched,
        "invalid": report.invalid,
        "extra": report.extra,
        "main_nso0": report.main_magic_ok,
    })
}

fn print_report(report: &VerifyReport, strict: bool) {
    println!("{}", report.root.display());
    println!(
        "  verificados {} de {} · {}",
        report.verified,
        report.listed,
        human_size(report.bytes)
    );
    match report.main_magic_ok {
        Some(true) => println!("  main       NSO0 OK"),
        Some(false) => {
            let suffix = if strict {
                " · fallo"
            } else {
                " · aviso (usa --strict para que falle)"
            };
            println!("  main       no empieza por NSO0{suffix}");
        }
        None => println!("  main       no existe"),
    }

    print_list("faltan", &report.missing);
    print_list("distintos", &report.mismatched);
    print_list("inválidos", &report.invalid);
    if !report.extra.is_empty() {
        let suffix = if strict {
            " · fallo"
        } else {
            " · aviso (usa --strict para que falle)"
        };
        println!("  sobran     {} archivo(s){suffix}", report.extra.len());
        print_items(&report.extra);
    }

    let issues = report.issues(strict);
    if issues == 0 {
        println!("  resultado  OK");
    } else {
        println!("  resultado  FALLÓ · {issues} problema(s)");
    }
}

fn print_list(label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    println!("  {label:<10} {} archivo(s)", items.len());
    print_items(items);
}

fn print_items(items: &[String]) {
    const LIMIT: usize = 20;
    for item in items.iter().take(LIMIT) {
        println!("    {item}");
    }
    if items.len() > LIMIT {
        println!("    … y {} más", items.len() - LIMIT);
    }
}

/// Busca raíces de verificación: un `SHA256SUMS` en la carpeta indicada o, si
/// no lo hay, en cualquier subcarpeta. No se desciende dentro de una raíz ya
/// encontrada.
fn find_roots(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    if dir.join("SHA256SUMS").is_file() {
        return Ok(vec![dir.to_path_buf()]);
    }
    let mut roots = Vec::new();
    walk_roots(dir, &mut roots)?;
    roots.sort();
    Ok(roots)
}

fn walk_roots(dir: &Path, roots: &mut Vec<PathBuf>) -> Result<(), Error> {
    if dir.join("SHA256SUMS").is_file() {
        roots.push(dir.to_path_buf());
        return Ok(());
    }
    let mut entries = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        if entry.file_type()?.is_dir() {
            walk_roots(&entry.path(), roots)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "nsromexefs-cli-verify-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_single_and_nested_roots() {
        let root = temp_dir("roots");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("a/SHA256SUMS"), "x").unwrap();
        fs::write(root.join("b/SHA256SUMS"), "x").unwrap();

        let roots = find_roots(&root).unwrap();
        assert_eq!(roots.len(), 2);
        assert!(roots[0].ends_with("a"));
        assert!(roots[1].ends_with("b"));

        let single = find_roots(&root.join("a")).unwrap();
        assert_eq!(single, vec![root.join("a")]);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_sums_is_reported_as_invalid() {
        let root = temp_dir("none");
        let error = cmd_verify(&root, false, false).unwrap_err();
        assert_eq!(error.code(), "E_LAYOUT_UNEXPECTED");
        fs::remove_dir_all(&root).unwrap();
    }
}
