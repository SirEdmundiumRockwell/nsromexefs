//! Utilidades compartidas por los subcomandos.

use std::path::PathBuf;

use nsromexefs_core::{Error, KeySet};

/// Código de salida estable por tipo de error (§19.4 del diseño).
pub(crate) fn exit_code(err: &Error) -> u8 {
    match err {
        Error::MissingHeaderKey
        | Error::MissingTitlekek { .. }
        | Error::MissingTitlekey { .. }
        | Error::MissingKeyArea { .. }
        | Error::TicketPersonalized
        | Error::KeysetInvalid { .. } => 2,
        Error::MissingBase | Error::BaseMismatch { .. } => 3,
        Error::Unsupported(_) => 4,
        Error::VerifyFailed { .. } => 5,
        Error::NameCollision { .. } => 6,
        Error::NoSpace { .. } => 7,
        Error::BatchFailed { exit, .. } => *exit,
        _ => 1,
    }
}

/// Contrato estable de error para `--json` (§22.1 del diseño).
pub(crate) fn error_json(err: &Error) -> serde_json::Value {
    let context = match err {
        Error::NoSpace {
            path,
            needed,
            available,
        } => serde_json::json!({
            "path": path.display().to_string(),
            "needed": needed,
            "available": available,
        }),
        Error::OutputExists { path } => serde_json::json!({
            "path": path.display().to_string(),
        }),
        Error::KeysetInvalid { path, reason } => serde_json::json!({
            "path": path.display().to_string(),
            "reason": reason,
        }),
        Error::MissingTitlekek { revision, declared } => serde_json::json!({
            "required_revision": revision,
            "crypto_type": declared,
        }),
        Error::MissingTitlekey { rights_id } => serde_json::json!({
            "rights_id": rights_id,
        }),
        Error::MissingKeyArea { bank, revision } => serde_json::json!({
            "bank": bank,
            "revision": revision,
        }),
        Error::BaseMismatch { expected, actual } => serde_json::json!({
            "expected": expected,
            "actual": actual,
        }),
        Error::VerifyFailed { checked, failed } => serde_json::json!({
            "checked": checked,
            "failed": failed,
        }),
        Error::VersionNotFound {
            requested,
            available,
        } => serde_json::json!({
            "requested": requested,
            "available": available,
        }),
        Error::NameCollision { path } => serde_json::json!({
            "path": path,
        }),
        Error::BatchFailed {
            failed,
            total,
            exit,
        } => serde_json::json!({
            "failed": failed,
            "total": total,
            "exit": exit,
        }),
        _ => serde_json::Value::Null,
    };

    serde_json::json!({
        "error": {
            "code": err.code(),
            "message": err.to_string(),
            "hint": err.hint(),
            "context": context,
        }
    })
}

/// Une dos listas de rutas sin duplicados, conservando el orden.
pub(crate) fn merge_paths(a: &[PathBuf], b: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in a.iter().chain(b) {
        if !out.contains(path) {
            out.push(path.clone());
        }
    }
    out
}

/// Carga el keyset sin abortar si no hay archivos (para `info`).
pub(crate) fn load_keys_lenient(explicit: &[PathBuf]) -> Option<KeySet> {
    let paths = if explicit.is_empty() {
        discover_keys()
    } else {
        explicit.to_vec()
    };
    if paths.is_empty() {
        return None;
    }
    KeySet::load(&paths).ok().filter(|set| set.key_count() > 0)
}

fn discover_keys() -> Vec<PathBuf> {
    let mut found = Vec::new();

    if let Ok(env) = std::env::var("NSROMEXEFS_KEYS") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        found.extend(
            env.split(sep)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        );
    }

    let mut candidates: Vec<PathBuf> =
        vec![PathBuf::from("prod.keys"), PathBuf::from("title.keys")];
    if let Some(home) = home_dir() {
        candidates.push(home.join(".switch").join("prod.keys"));
        candidates.push(home.join(".switch").join("title.keys"));
    }
    found.extend(candidates.into_iter().filter(|p| p.is_file()));

    found
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Abrevia un nombre de archivo NCA para la salida humana.
pub(crate) fn short_id(name: &str) -> String {
    let stem = name.split('.').next().unwrap_or(name);
    if stem.len() > 18 {
        format!("{}…", &stem[..16])
    } else {
        stem.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_the_contract() {
        assert_eq!(exit_code(&Error::MissingHeaderKey), 2);
        assert_eq!(exit_code(&Error::MissingBase), 3);
        assert_eq!(
            exit_code(&Error::BaseMismatch {
                expected: "a".into(),
                actual: "b".into()
            }),
            3
        );
        assert_eq!(exit_code(&Error::Unsupported("x".into())), 4);
        assert_eq!(
            exit_code(&Error::VerifyFailed {
                checked: 2,
                failed: 1
            }),
            5
        );
        assert_eq!(exit_code(&Error::NameCollision { path: "a".into() }), 6);
        assert_eq!(
            exit_code(&Error::NoSpace {
                path: PathBuf::from("x"),
                needed: 1,
                available: 0
            }),
            7
        );
        assert_eq!(
            exit_code(&Error::BatchFailed {
                failed: 1,
                total: 2,
                exit: 3
            }),
            3
        );
        assert_eq!(exit_code(&Error::Invalid("x".into())), 1);
    }

    #[test]
    fn json_error_has_the_stable_shape() {
        let error = Error::NoSpace {
            path: PathBuf::from("C:\\out"),
            needed: 10,
            available: 5,
        };
        let value = error_json(&error);
        assert_eq!(value["error"]["code"], "E_NO_SPACE");
        assert_eq!(value["error"]["context"]["needed"], 10);
        assert_eq!(value["error"]["context"]["available"], 5);
        assert_eq!(value["error"]["context"]["path"], "C:\\out");
        assert!(value["error"]["hint"].is_string());

        let plain = error_json(&Error::Invalid("x".into()));
        assert_eq!(plain["error"]["code"], "E_LAYOUT_UNEXPECTED");
        assert!(plain["error"]["context"].is_null());
    }

    #[test]
    fn short_id_does_not_duplicate_the_ellipsis() {
        let short = short_id("01006BB00C6F00000000000000000000.nca");
        assert!(short.ends_with('…'));
        assert_eq!(short.matches('…').count(), 1);
    }

    #[test]
    fn merges_paths_without_duplicates() {
        let a = vec![PathBuf::from("prod.keys")];
        let b = vec![PathBuf::from("title.keys"), PathBuf::from("prod.keys")];
        assert_eq!(
            merge_paths(&a, &b),
            vec![PathBuf::from("prod.keys"), PathBuf::from("title.keys")]
        );
    }
}
