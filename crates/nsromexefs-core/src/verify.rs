//! Verificación de un árbol extraído contra su `SHA256SUMS`.
//!
//! El manifiesto es la fuente de verdad: cada línea declara el hash de una
//! ruta relativa. La verificación comprueba que el archivo existe, que es un
//! archivo normal (no un enlace), que su SHA-256 coincide y, opcionalmente,
//! que no sobran archivos. Las rutas del manifiesto nunca pueden salir de la
//! raíz verificada (`..`, absolutas, unidades de Windows).

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{Error, Result};

const HASH_HEX_LEN: usize = 64;
const HASH_BUFFER: usize = 1024 * 1024;

/// Una línea válida de `SHA256SUMS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyEntry {
    /// Ruta relativa tal como aparece en el manifiesto, con `/`.
    pub path: String,
    /// SHA-256 en minúsculas (64 hex).
    pub sha256: String,
}

/// Resultado de verificar un árbol.
#[derive(Debug, Clone, Default)]
pub struct VerifyReport {
    pub root: PathBuf,
    pub listed: usize,
    pub verified: usize,
    pub bytes: u64,
    pub missing: Vec<String>,
    pub mismatched: Vec<String>,
    pub invalid: Vec<String>,
    /// Archivos presentes que el manifiesto no lista.
    pub extra: Vec<String>,
    /// `Some(true)` si `exefs/main` empieza por `NSO0`; `None` si no existe.
    pub main_magic_ok: Option<bool>,
    /// `Some(true)` cuando hay manifest con schema y conteos válidos.
    pub manifest_ok: Option<bool>,
}

impl VerifyReport {
    /// Número de problemas. Los extras y el magic de `main` solo cuentan como
    /// fallo en modo estricto.
    pub fn issues(&self, strict: bool) -> usize {
        let mut count = self.missing.len() + self.mismatched.len() + self.invalid.len();
        if strict {
            count += self.extra.len();
            if self.main_magic_ok == Some(false) {
                count += 1;
            }
        }
        count
    }

    pub fn ok(&self, strict: bool) -> bool {
        self.issues(strict) == 0
    }
}

/// Verifica `root` usando `root/SHA256SUMS`.
pub fn verify_tree(root: &Path) -> Result<VerifyReport> {
    let sums_path = root.join("SHA256SUMS");
    let sums_metadata = fs::symlink_metadata(&sums_path)
        .map_err(|err| Error::Invalid(format!("cannot inspect {}: {err}", sums_path.display())))?;
    if !sums_metadata.file_type().is_file() {
        return Err(Error::Invalid(format!(
            "{}: SHA256SUMS must be a regular file, not a link",
            sums_path.display()
        )));
    }
    let text = fs::read_to_string(&sums_path)
        .map_err(|err| Error::Invalid(format!("cannot read {}: {err}", sums_path.display())))?;
    let entries = parse_sha256sums(&text)?;

    let manifest_path = root.join("manifest.json");
    let manifest_metadata = fs::symlink_metadata(&manifest_path);
    let manifest_present = manifest_metadata.is_ok();
    let manifest = match manifest_metadata {
        Ok(metadata) if metadata.file_type().is_file() => fs::read(&manifest_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()),
        Ok(_) => None,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => None,
    };

    let mut report = VerifyReport {
        root: root.to_path_buf(),
        listed: entries.len(),
        ..VerifyReport::default()
    };
    if manifest_present {
        let valid = manifest.as_ref().is_some_and(|value| {
            let schema_ok = value["schema"].as_u64() == Some(1);
            let exefs_files = value["exefs"]["files"].as_u64().unwrap_or(0);
            let romfs_files = value["romfs"]["files"].as_u64().unwrap_or(0);
            let count_ok = exefs_files.saturating_add(romfs_files) == entries.len() as u64;
            let main_ok = match value["exefs"]["main_sha256"].as_str() {
                Some(hash) => entries.iter().any(|entry| {
                    entry.path.eq_ignore_ascii_case("exefs/main")
                        && entry.sha256.eq_ignore_ascii_case(hash)
                }),
                None => true,
            };
            schema_ok && count_ok && main_ok
        });
        report.manifest_ok = Some(valid);
        if !valid {
            report
                .invalid
                .push("manifest.json: invalid format, schema, or counts".into());
        }
    }
    let expected: HashSet<String> = entries
        .iter()
        .map(|entry| entry.path.to_lowercase())
        .collect();
    let mut actual_sizes = HashMap::new();

    for entry in &entries {
        let Some(relative) = safe_relative(&entry.path) else {
            report
                .invalid
                .push(format!("{}: path not allowed", entry.path));
            continue;
        };
        let full = root.join(&relative);
        match has_symlink_component(root, &relative) {
            Ok(true) => {
                report.invalid.push(format!(
                    "{}: the path traverses a symbolic link",
                    entry.path
                ));
                continue;
            }
            Ok(false) => {}
            Err(err) => {
                report.invalid.push(format!("{}: {err}", entry.path));
                continue;
            }
        }
        let metadata = match fs::symlink_metadata(&full) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                report.missing.push(entry.path.clone());
                continue;
            }
            Err(err) => {
                report.invalid.push(format!("{}: {err}", entry.path));
                continue;
            }
        };

        if !metadata.file_type().is_file() {
            report
                .invalid
                .push(format!("{}: not a regular file", entry.path));
            continue;
        }

        match hash_file(&full) {
            Ok((sha256, size)) => {
                report.bytes = report.bytes.saturating_add(size);
                actual_sizes.insert(entry.path.to_lowercase(), size);
                if sha256 == entry.sha256 {
                    report.verified += 1;
                } else {
                    report.mismatched.push(entry.path.clone());
                }
            }
            Err(err) => report.invalid.push(format!("{}: {err}", entry.path)),
        }
    }

    let mut present = Vec::new();
    walk_files(root, root, &mut present)?;
    for path in present {
        if is_auxiliary(&path) {
            continue;
        }
        if !expected.contains(&path.to_lowercase()) {
            report.extra.push(path);
        }
    }

    let main_relative = Path::new("exefs/main");
    let main = root.join(main_relative);
    if !has_symlink_component(root, main_relative)? && main.is_file() {
        let mut magic = [0u8; 4];
        let ok = File::open(&main)
            .and_then(|mut file| file.read_exact(&mut magic))
            .is_ok()
            && &magic == b"NSO0";
        report.main_magic_ok = Some(ok);
    }

    if report.manifest_ok == Some(true) {
        if let Some(size) = manifest
            .as_ref()
            .and_then(|value| value["exefs"]["main_size"].as_u64())
        {
            if actual_sizes.get("exefs/main").copied() != Some(size) {
                report.manifest_ok = Some(false);
                report
                    .invalid
                    .push("manifest.json: exefs/main size does not match".into());
            }
        }
    }

    Ok(report)
}

/// Interpreta un `SHA256SUMS` en formato GNU (`<hash>  <ruta>` o
/// `<hash> *<ruta>`).
pub fn parse_sha256sums(text: &str) -> Result<Vec<VerifyEntry>> {
    let mut entries = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let number = line_no + 1;
        if line.len() < HASH_HEX_LEN + 3 {
            return Err(invalid_line(number, "line too short"));
        }
        let bytes = line.as_bytes();
        let hash_bytes = &bytes[..HASH_HEX_LEN];
        if !hash_bytes.iter().all(u8::is_ascii_hexdigit) {
            return Err(invalid_line(number, "the hash is not hexadecimal"));
        }
        let hash = std::str::from_utf8(hash_bytes)
            .map_err(|_| invalid_line(number, "the hash is not hexadecimal"))?;
        if !matches!(
            bytes.get(HASH_HEX_LEN..HASH_HEX_LEN + 2),
            Some(b"  " | b" *")
        ) {
            return Err(invalid_line(number, "missing the two separator spaces"));
        }
        // El separador ASCII garantiza una frontera UTF-8 en el byte 66.
        let path = &line[HASH_HEX_LEN + 2..];
        if path.is_empty() {
            return Err(invalid_line(number, "empty path"));
        }
        if safe_relative(path).is_none() {
            return Err(invalid_line(number, "path not allowed"));
        }
        entries.push(VerifyEntry {
            path: path.to_string(),
            sha256: hash.to_ascii_lowercase(),
        });
    }

    if entries.is_empty() {
        return Err(Error::Invalid("SHA256SUMS contains no entries".into()));
    }
    Ok(entries)
}

fn invalid_line(line: usize, reason: &str) -> Error {
    Error::Invalid(format!("SHA256SUMS line {line}: {reason}"))
}

/// Ruta relativa segura: sin componentes vacíos, `.`, `..`, separadores de
/// Windows ni unidades.
fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty()
        || path.contains('\0')
        || path.contains('\\')
        || path.contains(':')
        || path.starts_with('/')
    {
        return None;
    }
    let mut out = PathBuf::new();
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return None;
        }
        out.push(component);
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn hash_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut reader = BufReader::with_capacity(HASH_BUFFER, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_BUFFER];
    let mut size = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }
    let digest: [u8; 32] = hasher.finalize().into();
    Ok((hex(&digest), size))
}

/// Detecta enlaces simbólicos en cualquier componente de la ruta. `symlink_metadata`
/// inspecciona el componente en sí, sin seguirlo.
fn has_symlink_component(root: &Path, relative: &Path) -> std::io::Result<bool> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err),
        };
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
        if index + 1 < components.len() && !metadata.is_dir() {
            return Ok(false);
        }
    }
    Ok(false)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Recorre el árbol sin seguir enlaces y devuelve rutas relativas con `/`.
fn walk_files(root: &Path, current: &Path, out: &mut Vec<String>) -> Result<()> {
    let mut entries = fs::read_dir(current)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk_files(root, &entry.path(), out)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            out.push(relative);
        }
    }
    Ok(())
}

fn is_auxiliary(path: &str) -> bool {
    matches!(path, "SHA256SUMS" | "manifest.json") || path.ends_with(".part")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "nsromexefs-verify-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn sums_for(files: &[(&str, &[u8])]) -> String {
        let mut text = String::new();
        for (path, data) in files {
            let digest: [u8; 32] = Sha256::digest(data).into();
            text.push_str(&hex(&digest));
            text.push_str("  ");
            text.push_str(path);
            text.push('\n');
        }
        text
    }

    #[test]
    fn parses_gnu_and_binary_markers() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let entries = parse_sha256sums(&format!("{a}  exefs/main\n{b} *romfs/x.bin\n")).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "exefs/main");
        assert_eq!(entries[1].sha256, "b".repeat(64));
    }

    #[test]
    fn rejects_traversal_and_malformed_lines() {
        let hash = "a".repeat(64);
        for bad in [
            format!("{hash}  ../evil"),
            format!("{hash}  /abs"),
            format!("{hash}  a\\b"),
            format!("{hash}  C:/x"),
            format!("{hash}  a//b"),
            format!("{hash} x"),
            "zz  a".to_string(),
            "corto".to_string(),
        ] {
            assert!(parse_sha256sums(&bad).is_err(), "{bad:?}");
        }
        // Un prefijo no ASCII no debe provocar un panic si byte 64 no es
        // una frontera UTF-8.
        assert!(parse_sha256sums(&format!("{}é  exefs/main", "a".repeat(63))).is_err());
        assert!(parse_sha256sums("").is_err());
    }

    #[test]
    fn verifies_a_tree_and_detects_damage() {
        let root = temp_root("ok");
        fs::create_dir_all(root.join("exefs")).unwrap();
        fs::create_dir_all(root.join("romfs")).unwrap();
        fs::write(root.join("exefs/main"), b"NSO0resto").unwrap();
        fs::write(root.join("romfs/a.txt"), b"hola").unwrap();
        let sums = sums_for(&[("exefs/main", b"NSO0resto"), ("romfs/a.txt", b"hola")]);
        fs::write(root.join("SHA256SUMS"), sums).unwrap();

        let report = verify_tree(&root).unwrap();
        assert_eq!(report.listed, 2);
        assert_eq!(report.verified, 2);
        assert_eq!(report.issues(false), 0);
        assert_eq!(report.main_magic_ok, Some(true));
        assert!(report.extra.is_empty());

        // Un byte distinto y un archivo que el manifiesto no lista.
        fs::write(root.join("romfs/a.txt"), b"adios").unwrap();
        fs::write(root.join("romfs/extra.txt"), b"x").unwrap();
        let report = verify_tree(&root).unwrap();
        assert_eq!(report.mismatched, vec!["romfs/a.txt"]);
        assert_eq!(report.extra, vec!["romfs/extra.txt"]);
        assert!(!report.ok(false));
        assert!(report.issues(true) > report.issues(false));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn flags_missing_files_and_bad_main_magic() {
        let root = temp_root("missing");
        fs::create_dir_all(root.join("exefs")).unwrap();
        fs::write(root.join("exefs/main"), b"NOPE").unwrap();
        let sums = sums_for(&[("exefs/main", b"NOPE"), ("romfs/falta.txt", b"x")]);
        fs::write(root.join("SHA256SUMS"), sums).unwrap();

        let report = verify_tree(&root).unwrap();
        assert_eq!(report.missing, vec!["romfs/falta.txt"]);
        assert_eq!(report.main_magic_ok, Some(false));
        assert_eq!(report.verified, 1);
        assert!(report.issues(false) > 0);
        assert!(report.issues(true) > report.issues(false));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn validates_manifest_schema_counts_and_main_size() {
        let root = temp_root("manifest");
        fs::create_dir_all(root.join("exefs")).unwrap();
        fs::create_dir_all(root.join("romfs")).unwrap();
        fs::write(root.join("exefs/main"), b"NSO0").unwrap();
        fs::write(root.join("romfs/a"), b"x").unwrap();
        let main_hash: [u8; 32] = Sha256::digest(b"NSO0").into();
        let sums = sums_for(&[("exefs/main", b"NSO0"), ("romfs/a", b"x")]);
        fs::write(root.join("SHA256SUMS"), sums).unwrap();
        let manifest = serde_json::json!({
            "schema": 1,
            "exefs": {
                "files": 1,
                "main_sha256": hex(&main_hash),
                "main_size": 4,
            },
            "romfs": {"files": 1},
        });
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let report = verify_tree(&root).unwrap();
        assert_eq!(report.manifest_ok, Some(true));
        assert!(report.ok(false));

        let bad = serde_json::json!({
            "schema": 2,
            "exefs": {"files": 10, "main_sha256": hex(&main_hash), "main_size": 99},
            "romfs": {"files": 0},
        });
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&bad).unwrap(),
        )
        .unwrap();
        let report = verify_tree(&root).unwrap();
        assert_eq!(report.manifest_ok, Some(false));
        assert!(!report.invalid.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
