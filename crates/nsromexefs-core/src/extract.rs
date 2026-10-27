//! Extracción de `exefs` (PFS0) y `romfs` (IVFC + RomFS) a disco.
//!
//! Todo se lee **en flujo**: nunca se descifra la NCA entera ni se cargan los
//! datos del RomFS en memoria. Cada archivo se escribe primero en un `.part`
//! y solo se renombra cuando está completo; así una interrupción no deja
//! archivos a medias con el nombre definitivo.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::ivfc::Ivfc;
use crate::nca::Pfs0Superblock;
use crate::pfs0::Pfs0;
use crate::romfs::{RomFs, RomFsEntry, RomFsHeader};
use crate::section::SectionRead;
use crate::{Error, Result};

/// Bloque de copia. 4 MiB amortiza el `seek` sin disparar la memoria.
pub const COPY_CHUNK: usize = 4 * 1024 * 1024;
/// Un PFS0 de exefs real no pasa de unas decenas de MB.
const MAX_PFS0_IN_MEMORY: u64 = 512 * 1024 * 1024;
/// Tablas de RomFS: con 3.836 archivos ocupan unos pocos MB.
const MAX_META_TABLE: u64 = 256 * 1024 * 1024;
const MAX_PATH_LEN: usize = 4096;

#[derive(Debug, Clone, Copy, Default)]
pub struct WriteOptions {
    /// Sobrescribir archivos existentes.
    pub force: bool,
}

/// Archivo extraído, con su hash calculado al escribirlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedFile {
    /// Ruta relativa al directorio de salida, con `/`.
    pub path: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

impl ExtractedFile {
    pub fn sha256_hex(&self) -> String {
        self.sha256.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Índice del RomFS sin extraer nada (para `--dry-run`).
#[derive(Debug, Clone, Default)]
pub struct RomFsIndex {
    pub files: Vec<RomFsEntry>,
    pub dirs: usize,
    pub bytes: u64,
    pub fs_size: u64,
}

impl RomFsIndex {
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

// ---------------------------------------------------------------------------
// exefs
// ---------------------------------------------------------------------------

/// Lee el PFS0 de la sección y devuelve sus entradas (nombre y tamaño).
pub fn exefs_index(source: &mut dyn SectionRead) -> Result<Vec<(String, u64)>> {
    let pfs0 = read_pfs0(source)?;
    Ok(pfs0
        .pfs
        .entries
        .iter()
        .map(|e| (e.name.clone(), e.size))
        .collect())
}

/// Extrae el `exefs` completo en `out_dir` (que se crea si hace falta).
pub fn extract_exefs(
    source: &mut dyn SectionRead,
    out_dir: &Path,
    opts: &WriteOptions,
    on_file: &mut dyn FnMut(&str, u64),
) -> Result<Vec<ExtractedFile>> {
    let Pfs0InMemory { pfs, pfs0_data } = read_pfs0(source)?;
    fs::create_dir_all(out_dir)?;

    let mut paths = PathGuard::default();
    let mut out = Vec::with_capacity(pfs.entries.len());

    for entry in &pfs.entries {
        validate_component(&entry.name)?;
        paths.register(&entry.name)?;
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.size as usize)
            .ok_or_else(|| Error::Invalid(format!("entrada '{}' desbordada", entry.name)))?;
        if end > pfs0_data.len() {
            return Err(Error::Invalid(format!(
                "entrada '{}' fuera del PFS0 (0x{start:X}..0x{end:X} > 0x{:X})",
                entry.name,
                pfs0_data.len()
            )));
        }

        on_file(&entry.name, entry.size);
        let file = write_bytes(
            &out_dir.join(&entry.name),
            &pfs0_data[start..end],
            &entry.name,
            opts,
        )?;
        out.push(file);
    }

    Ok(out)
}

struct Pfs0InMemory {
    pfs: Pfs0,
    pfs0_data: Vec<u8>,
}

fn read_pfs0(source: &mut dyn SectionRead) -> Result<Pfs0InMemory> {
    let superblock = Pfs0Superblock::parse(source.hash_info());

    if superblock.pfs0_size == 0 {
        return Err(Error::Invalid(
            "el superbloque PFS0 declara tamaño cero".into(),
        ));
    }
    if superblock.pfs0_size > MAX_PFS0_IN_MEMORY {
        return Err(Error::Unsupported(format!(
            "PFS0 de {} bytes: demasiado grande para leerlo en memoria (máximo {})",
            superblock.pfs0_size, MAX_PFS0_IN_MEMORY
        )));
    }
    let end = superblock
        .pfs0_offset
        .checked_add(superblock.pfs0_size)
        .ok_or_else(|| Error::Invalid("rango del PFS0 desbordado".into()))?;
    if end > source.size() {
        return Err(Error::Invalid(format!(
            "el PFS0 (0x{:X}..0x{end:X}) cae fuera de la sección {} (0x{:X} bytes)",
            superblock.pfs0_offset,
            source.index(),
            source.size()
        )));
    }

    let mut data = vec![0u8; superblock.pfs0_size as usize];
    source.read_at(superblock.pfs0_offset, &mut data)?;

    let pfs = Pfs0::read(&mut Cursor::new(&data), superblock.pfs0_size)?;
    Ok(Pfs0InMemory {
        pfs,
        pfs0_data: data,
    })
}

// ---------------------------------------------------------------------------
// romfs
// ---------------------------------------------------------------------------

/// Lee el árbol del RomFS sin escribir nada.
pub fn romfs_index(source: &mut dyn SectionRead) -> Result<RomFsIndex> {
    let ivfc = Ivfc::parse(source.hash_info())?;
    let fs_offset = ivfc.fs_offset();
    let fs_size = ivfc.fs_size();

    if fs_size == 0 {
        return Err(Error::Invalid(
            "la cabecera IVFC declara un RomFS vacío".into(),
        ));
    }
    let fs_end = fs_offset
        .checked_add(fs_size)
        .ok_or_else(|| Error::Invalid("rango del RomFS desbordado".into()))?;
    if fs_end > source.size() {
        return Err(Error::Invalid(format!(
            "el RomFS (0x{fs_offset:X}..0x{fs_end:X}) cae fuera de la sección {} (0x{:X} bytes)",
            source.index(),
            source.size()
        )));
    }

    let mut header_bytes = vec![0u8; crate::romfs::ROMFS_HEADER_LEN];
    source.read_at(fs_offset, &mut header_bytes)?;
    let header = RomFsHeader::parse(&header_bytes)?;

    let dir_meta = read_table(
        source,
        fs_offset,
        fs_size,
        header.dir_meta_offset,
        header.dir_meta_size,
        "directorios",
    )?;
    let file_meta = read_table(
        source,
        fs_offset,
        fs_size,
        header.file_meta_offset,
        header.file_meta_size,
        "archivos",
    )?;

    let romfs = RomFs::new(header, dir_meta, file_meta)?;
    let tree = romfs.tree()?;

    let mut bytes = 0u64;
    let mut seen_names: HashSet<String> = HashSet::new();

    for entry in &tree.files {
        validate_romfs_path(&entry.path)?;
        let key = entry.path.to_lowercase();
        if !seen_names.insert(key) {
            return Err(Error::NameCollision {
                path: entry.path.clone(),
            });
        }
        let end = entry
            .offset
            .checked_add(entry.size)
            .ok_or_else(|| Error::Invalid(format!("tamaño desbordado en '{}'", entry.path)))?;
        if end > fs_size {
            return Err(Error::Invalid(format!(
                "'{}' (0x{:X}..0x{end:X}) cae fuera del RomFS (0x{fs_size:X})",
                entry.path, entry.offset
            )));
        }
        bytes = bytes
            .checked_add(entry.size)
            .ok_or_else(|| Error::Invalid("tamaño total del RomFS desbordado".into()))?;
    }

    reject_file_directory_clashes(&tree.files, &seen_names)?;

    Ok(RomFsIndex {
        files: tree.files,
        dirs: tree.dirs,
        bytes,
        fs_size,
    })
}

/// Lee un archivo concreto del RomFS sin escribir nada (p. ej. `control.nacp`).
///
/// Recorre el árbol igual que [`romfs_index`] para localizar la ruta exacta;
/// devuelve `None` si el archivo no existe.
pub fn romfs_read_file(source: &mut dyn SectionRead, path: &str) -> Result<Option<Vec<u8>>> {
    /// `control.nacp` son unas decenas de KB; 64 MiB es un techo holgado.
    const MAX_SINGLE_FILE: u64 = 64 * 1024 * 1024;

    let index = romfs_index(source)?;
    let Some(entry) = index.files.iter().find(|e| e.path == path) else {
        return Ok(None);
    };
    if entry.size > MAX_SINGLE_FILE {
        return Err(Error::Unsupported(format!(
            "'{path}' ocupa {} bytes: demasiado grande para leerlo de una vez",
            entry.size
        )));
    }

    let fs_offset = Ivfc::parse(source.hash_info())?.fs_offset();
    let mut data = vec![0u8; entry.size as usize];
    source.read_at(fs_offset + entry.offset, &mut data)?;
    Ok(Some(data))
}

/// Extrae los archivos del índice en `out_dir` (la raíz RomFS se convierte en
/// la raíz de `out_dir`).
pub fn extract_romfs(
    source: &mut dyn SectionRead,
    index: &RomFsIndex,
    out_dir: &Path,
    opts: &WriteOptions,
    on_file: &mut dyn FnMut(&str, u64),
) -> Result<Vec<ExtractedFile>> {
    let fs_offset = Ivfc::parse(source.hash_info())?.fs_offset();

    fs::create_dir_all(out_dir)?;

    let mut buffer = vec![0u8; COPY_CHUNK];
    let mut out = Vec::with_capacity(index.files.len());

    for entry in &index.files {
        let relative = entry.path.replace('/', std::path::MAIN_SEPARATOR_STR);
        let target = out_dir.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        on_file(&entry.path, entry.size);
        let file = write_section_range(
            source,
            fs_offset + entry.offset,
            entry.size,
            &target,
            &entry.path,
            opts,
            &mut buffer,
        )?;
        out.push(file);
    }

    Ok(out)
}

fn read_table(
    source: &mut dyn SectionRead,
    fs_offset: u64,
    fs_size: u64,
    table_offset: u64,
    table_size: u64,
    label: &str,
) -> Result<Vec<u8>> {
    if table_size == 0 {
        return Err(Error::Invalid(format!(
            "el RomFS no tiene tabla de {label}"
        )));
    }
    if table_size > MAX_META_TABLE {
        return Err(Error::Invalid(format!(
            "tabla de {label} de {table_size} bytes (máximo {MAX_META_TABLE})"
        )));
    }
    let end = table_offset
        .checked_add(table_size)
        .ok_or_else(|| Error::Invalid(format!("rango de la tabla de {label} desbordado")))?;
    if end > fs_size {
        return Err(Error::Invalid(format!(
            "la tabla de {label} (0x{table_offset:X}..0x{end:X}) cae fuera del RomFS (0x{fs_size:X})"
        )));
    }

    let mut data = vec![0u8; table_size as usize];
    source.read_at(fs_offset + table_offset, &mut data)?;
    Ok(data)
}

// ---------------------------------------------------------------------------
// escritura atómica y hashes
// ---------------------------------------------------------------------------

fn write_bytes(
    path: &Path,
    bytes: &[u8],
    label: &str,
    opts: &WriteOptions,
) -> Result<ExtractedFile> {
    let mut writer = AtomicWriter::create(path, opts.force)?;
    writer.write(bytes)?;
    writer.commit(relative_name(label), bytes.len() as u64)
}

fn write_section_range(
    source: &mut dyn SectionRead,
    offset: u64,
    size: u64,
    path: &Path,
    label: &str,
    opts: &WriteOptions,
    buffer: &mut [u8],
) -> Result<ExtractedFile> {
    let mut writer = AtomicWriter::create(path, opts.force)?;
    let mut done = 0u64;
    while done < size {
        let n = (size - done).min(buffer.len() as u64) as usize;
        source.read_at(offset + done, &mut buffer[..n])?;
        writer.write(&buffer[..n])?;
        done += n as u64;
    }
    writer.commit(relative_name(label), size)
}

fn relative_name(label: &str) -> String {
    label.replace('\\', "/")
}

struct AtomicWriter {
    final_path: PathBuf,
    tmp_path: PathBuf,
    file: File,
    hasher: Sha256,
    written: u64,
    force: bool,
    committed: bool,
}

impl AtomicWriter {
    fn create(path: &Path, force: bool) -> Result<Self> {
        if path.exists() && !force {
            return Err(Error::OutputExists {
                path: path.to_path_buf(),
            });
        }
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "salida".to_string());
        let tmp_path = path.with_file_name(format!(".{file_name}.part"));

        Ok(Self {
            final_path: path.to_path_buf(),
            tmp_path,
            file: File::create(path.with_file_name(format!(".{file_name}.part")))?,
            hasher: Sha256::new(),
            written: 0,
            force,
            committed: false,
        })
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes)?;
        self.hasher.update(bytes);
        self.written += bytes.len() as u64;
        Ok(())
    }

    fn commit(mut self, path: String, expected: u64) -> Result<ExtractedFile> {
        use std::io::Write as _;
        self.file.flush()?;
        self.file.sync_all()?;

        if self.written != expected {
            return Err(Error::Invalid(format!(
                "'{path}': se escribieron {} bytes, se esperaban {expected}",
                self.written
            )));
        }

        if self.final_path.exists() && self.force {
            // `rename` no reemplaza en Windows.
            fs::remove_file(&self.final_path)?;
        }
        fs::rename(&self.tmp_path, &self.final_path)?;
        self.committed = true;

        let digest: [u8; 32] = std::mem::take(&mut self.hasher).finalize().into();
        Ok(ExtractedFile {
            path,
            size: self.written,
            sha256: digest,
        })
    }
}

impl Drop for AtomicWriter {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.tmp_path);
        }
    }
}

// ---------------------------------------------------------------------------
// rutas
// ---------------------------------------------------------------------------

/// Valida un nombre suelto (sin separadores) para el sistema de archivos actual.
fn validate_component(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Invalid("nombre de archivo vacío".into()));
    }
    if name == "." || name == ".." {
        return Err(Error::Invalid(format!(
            "nombre de archivo no permitido: '{name}'"
        )));
    }
    if name.len() > MAX_PATH_LEN {
        return Err(Error::Invalid("nombre de archivo demasiado largo".into()));
    }
    if name.contains(['/', '\\']) {
        return Err(Error::Invalid(format!(
            "el nombre '{name}' contiene un separador"
        )));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(Error::Invalid(format!(
            "el nombre '{name}' contiene caracteres de control"
        )));
    }
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err(Error::Invalid(format!(
            "el nombre '{name}' contiene caracteres no válidos en Windows"
        )));
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err(Error::Invalid(format!(
            "el nombre '{name}' termina en espacio o punto (no válido en Windows)"
        )));
    }
    Ok(())
}

fn validate_romfs_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return Err(Error::Invalid(format!("ruta de RomFS inválida: '{path}'")));
    }
    for component in path.split('/') {
        validate_component(component)?;
    }
    Ok(())
}

/// Detecta que un archivo haga de directorio de otro (`a` y `a/b`).
fn reject_file_directory_clashes(files: &[RomFsEntry], lower: &HashSet<String>) -> Result<()> {
    for entry in files {
        let mut end = entry.path.len();
        while let Some(pos) = entry.path[..end].rfind('/') {
            let parent = entry.path[..pos].to_lowercase();
            if lower.contains(&parent) {
                return Err(Error::NameCollision {
                    path: entry.path.clone(),
                });
            }
            end = pos;
        }
    }
    Ok(())
}

/// Registro de rutas ya usadas, comparando sin distinguir mayúsculas.
#[derive(Default)]
struct PathGuard {
    files: HashSet<String>,
    dirs: HashSet<String>,
}

impl PathGuard {
    fn register(&mut self, path: &str) -> Result<()> {
        let lower = path.to_lowercase();
        if self.files.contains(&lower) || self.dirs.contains(&lower) {
            return Err(Error::NameCollision {
                path: path.to_string(),
            });
        }
        self.files.insert(lower.clone());

        let mut end = lower.len();
        while let Some(pos) = lower[..end].rfind('/') {
            let parent = lower[..pos].to_string();
            if self.files.contains(&parent) {
                return Err(Error::NameCollision {
                    path: path.to_string(),
                });
            }
            self.dirs.insert(parent);
            end = pos;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_windows_invalid_names() {
        for name in [
            "a:b", "a?b", "a*b", "a\"b", "a<b", "a>b", "a|b", "a\\b", "a/b",
        ] {
            assert!(validate_component(name).is_err(), "{name} debería fallar");
        }
        assert!(validate_component("main").is_ok());
        assert!(validate_component("SinkingSword.bfevfl").is_ok());
        assert!(validate_component("ñandú.txt").is_ok());
    }

    #[test]
    fn rejects_traversal_and_empty_components() {
        assert!(validate_romfs_path("../etc/passwd").is_err());
        assert!(validate_romfs_path("a//b").is_err());
        assert!(validate_romfs_path("/a").is_err());
        assert!(validate_romfs_path("a/b").is_ok());
    }

    #[test]
    fn path_guard_detects_case_collisions() {
        let mut guard = PathGuard::default();
        guard.register("Data/Config").unwrap();
        assert_eq!(
            guard.register("data/config").unwrap_err().code(),
            "E_NAME_COLLISION"
        );
    }

    #[test]
    fn path_guard_detects_file_as_directory() {
        let mut guard = PathGuard::default();
        guard.register("a").unwrap();
        assert!(guard.register("a/b").is_err());
    }
}
