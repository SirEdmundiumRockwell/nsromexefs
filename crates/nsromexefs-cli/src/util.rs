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
        | Error::KeysetInvalid { .. } => 2,
        Error::MissingBase | Error::BaseMismatch { .. } => 3,
        Error::Unsupported(_) => 4,
        Error::BatchFailed { exit, .. } => *exit,
        _ => 1,
    }
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
