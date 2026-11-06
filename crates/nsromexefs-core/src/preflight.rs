//! Preflight: comprobaciones antes de escribir el primer byte.
//!
//! El objetivo es que una extracción de varios GB no falle a mitad por causas
//! previsibles (destino ocupado, volumen sin espacio). El margen de seguridad
//! cubre el bloqueo de metadatos del sistema de archivos y los archivos
//! temporales `.part`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Margen absoluto reservado además de los datos a escribir.
pub const SPACE_MARGIN_BYTES: u64 = 64 * 1024 * 1024;
/// Margen relativo (1 %) para absorber diferencias de tamaño de bloque.
pub const SPACE_MARGIN_PERCENT: u64 = 1;

/// Espacio que debe quedar libre para escribir `needed` bytes con seguridad.
pub fn required_space(needed: u64) -> u64 {
    needed
        .saturating_add(
            needed
                .saturating_mul(SPACE_MARGIN_PERCENT)
                .saturating_div(100),
        )
        .saturating_add(SPACE_MARGIN_BYTES)
}

/// Comprueba que el volumen que contiene `path` tiene espacio suficiente.
///
/// `path` puede no existir todavía: se sube hasta el primer ancestro existente.
pub fn ensure_space(path: &Path, needed: u64) -> Result<()> {
    let probe = existing_ancestor(path).ok_or_else(|| {
        Error::Invalid(format!(
            "{}: no parent directory exists to check available space",
            path.display()
        ))
    })?;
    let available = fs2::available_space(probe)?;
    let required = required_space(needed);
    if available < required {
        return Err(Error::NoSpace {
            path: probe.to_path_buf(),
            needed: required,
            available,
        });
    }
    Ok(())
}

/// Comprueba el destino antes de escribir.
///
/// Un directorio vacío se acepta sin `--force`; uno con contenido se rechaza
/// para no mezclar dos extracciones. Los archivos que ocupan la ruta del
/// destino siempre son un error.
pub fn ensure_output(path: &Path, force: bool) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Invalid(format!(
            "{}: the destination cannot be a symbolic link",
            path.display()
        ))),
        Ok(metadata) if metadata.is_dir() => {
            if force {
                return Ok(());
            }
            let mut entries = fs::read_dir(path)?;
            if entries.next().is_some() {
                return Err(Error::OutputExists {
                    path: path.to_path_buf(),
                });
            }
            Ok(())
        }
        Ok(_) => Err(Error::OutputExists {
            path: path.to_path_buf(),
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Preflight completo: destino, espacio y creación del directorio.
///
/// Con `dry_run` no se toca el disco ni se rechaza un destino ocupado; solo se
/// comprueba el espacio, que es información útil para el plan.
pub fn prepare_output(path: &Path, force: bool, needed: u64, dry_run: bool) -> Result<()> {
    if !dry_run {
        ensure_output(path, force)?;
    }
    ensure_space(path, needed)?;
    if !dry_run {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

fn existing_ancestor(path: &Path) -> Option<&Path> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// Comprueba que la ruta canónica de `base` es prefijo de `path` (§22.4).
pub fn ensure_within(base: &Path, path: &Path) -> Result<PathBuf> {
    let base = base.canonicalize()?;
    let path = path.canonicalize()?;
    if path.starts_with(&base) {
        Ok(path)
    } else {
        Err(Error::Invalid(format!(
            "{}: the final path falls outside {}",
            path.display(),
            base.display()
        )))
    }
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
            "nsromexefs-preflight-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn required_space_adds_margin() {
        assert_eq!(required_space(0), SPACE_MARGIN_BYTES);
        let needed = 10 * 1024 * 1024 * 1024u64;
        assert_eq!(
            required_space(needed),
            needed + needed / 100 + SPACE_MARGIN_BYTES
        );
        assert_eq!(required_space(u64::MAX), u64::MAX);
    }

    #[test]
    fn accepts_missing_or_empty_output_and_rejects_content() {
        let root = temp_dir("output");
        let target = root.join("nuevo");
        ensure_output(&target, false).unwrap();

        fs::create_dir(&target).unwrap();
        ensure_output(&target, false).unwrap();

        fs::write(target.join("archivo"), b"x").unwrap();
        assert!(matches!(
            ensure_output(&target, false),
            Err(Error::OutputExists { .. })
        ));
        ensure_output(&target, true).unwrap();

        let file = root.join("archivo");
        fs::write(&file, b"x").unwrap();
        assert!(matches!(
            ensure_output(&file, true),
            Err(Error::OutputExists { .. })
        ));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dry_run_does_not_create_or_reject() {
        let root = temp_dir("dry");
        let target = root.join("nuevo");
        prepare_output(&target, false, 1, true).unwrap();
        assert!(!target.exists());

        fs::create_dir(&target).unwrap();
        fs::write(target.join("archivo"), b"x").unwrap();
        prepare_output(&target, false, 1, true).unwrap();

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prepare_output_creates_the_directory() {
        let root = temp_dir("create");
        let target = root.join("a").join("b");
        prepare_output(&target, false, 1, false).unwrap();
        assert!(target.is_dir());
        fs::remove_dir_all(&root).unwrap();
    }
}
