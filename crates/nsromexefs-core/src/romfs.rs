//! RomFS: cabecera, tablas de metadatos y recorrido del árbol.
//!
//! Los offsets de la cabecera y de las tablas son **relativos al inicio del
//! RomFS** (el `logical_offset` del último nivel IVFC), no a la sección.
//! Los datos de cada archivo están en `data_offset + entrada.offset`.
//!
//! Portado de `hactool` (ISC, SciresM) `ivfc.h` / `romfs.c`.

use std::collections::HashSet;

use crate::error::{le32, le64};
use crate::{Error, Result};

pub const ROMFS_HEADER_LEN: usize = 0x50;
pub const ENTRY_EMPTY: u32 = 0xFFFF_FFFF;
const DIR_ENTRY_LEN: usize = 0x18;
const FILE_ENTRY_LEN: usize = 0x20;
const MAX_ENTRIES: usize = 1_000_000;

/// Cabecera de `0x50` bytes al inicio del RomFS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RomFsHeader {
    pub header_size: u64,
    pub dir_meta_offset: u64,
    pub dir_meta_size: u64,
    pub file_meta_offset: u64,
    pub file_meta_size: u64,
    pub data_offset: u64,
}

impl RomFsHeader {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < ROMFS_HEADER_LEN {
            return Err(Error::Invalid(format!(
                "truncated RomFS header: {} bytes",
                data.len()
            )));
        }
        let header = Self {
            header_size: le64(&data[0x00..]),
            dir_meta_offset: le64(&data[0x18..]),
            dir_meta_size: le64(&data[0x20..]),
            file_meta_offset: le64(&data[0x38..]),
            file_meta_size: le64(&data[0x40..]),
            data_offset: le64(&data[0x48..]),
        };
        if header.header_size != ROMFS_HEADER_LEN as u64 {
            return Err(Error::Unsupported(format!(
                "RomFS header of 0x{:X} bytes (only 0x50 is known)",
                header.header_size
            )));
        }
        Ok(header)
    }
}

/// Un archivo del RomFS, con su ruta relativa y su offset **dentro del RomFS**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomFsEntry {
    /// Ruta con separador `/`.
    pub path: String,
    /// Offset desde el inicio del RomFS (ya incluye `data_offset`).
    pub offset: u64,
    pub size: u64,
}

/// Resultado de recorrer el árbol.
#[derive(Debug, Clone, Default)]
pub struct RomFsTree {
    pub files: Vec<RomFsEntry>,
    /// Subdirectorios, sin contar la raíz.
    pub dirs: usize,
}

/// Tablas de metadatos de un RomFS (los datos no se cargan).
#[derive(Debug, Clone)]
pub struct RomFs {
    header: RomFsHeader,
    dir_meta: Vec<u8>,
    file_meta: Vec<u8>,
}

/// Vista de una entrada de directorio.
struct DirView<'a> {
    sibling: u32,
    child: u32,
    file: u32,
    name: &'a str,
}

/// Vista de una entrada de archivo.
struct FileView<'a> {
    sibling: u32,
    offset: u64,
    size: u64,
    name: &'a str,
}

impl RomFs {
    pub fn new(header: RomFsHeader, dir_meta: Vec<u8>, file_meta: Vec<u8>) -> Result<Self> {
        let romfs = Self {
            header,
            dir_meta,
            file_meta,
        };
        // La raíz tiene que ser legible antes de aceptar el árbol.
        romfs.dir_at(0)?;
        Ok(romfs)
    }

    pub fn header(&self) -> &RomFsHeader {
        &self.header
    }

    fn dir_at(&self, offset: u32) -> Result<DirView<'_>> {
        let at = offset as usize;
        let data = &self.dir_meta;
        if at
            .checked_add(DIR_ENTRY_LEN)
            .is_none_or(|end| end > data.len())
        {
            return Err(Error::Invalid(format!(
                "directory entry at 0x{offset:X} outside the table (0x{:X} bytes)",
                data.len()
            )));
        }
        let name_size = le32(&data[at + 0x14..]) as usize;
        let name_at = at + DIR_ENTRY_LEN;
        let name_end = name_at
            .checked_add(name_size)
            .ok_or_else(|| Error::Invalid("directory name length overflow".into()))?;
        if name_end > data.len() {
            return Err(Error::Invalid(format!(
                "directory name at 0x{offset:X} ends outside the table"
            )));
        }
        Ok(DirView {
            sibling: le32(&data[at + 0x04..]),
            child: le32(&data[at + 0x08..]),
            file: le32(&data[at + 0x0C..]),
            name: parse_name(&data[name_at..name_end])?,
        })
    }

    fn file_at(&self, offset: u32) -> Result<FileView<'_>> {
        let at = offset as usize;
        let data = &self.file_meta;
        if at
            .checked_add(FILE_ENTRY_LEN)
            .is_none_or(|end| end > data.len())
        {
            return Err(Error::Invalid(format!(
                "file entry at 0x{offset:X} outside the table (0x{:X} bytes)",
                data.len()
            )));
        }
        let name_size = le32(&data[at + 0x1C..]) as usize;
        let name_at = at + FILE_ENTRY_LEN;
        let name_end = name_at
            .checked_add(name_size)
            .ok_or_else(|| Error::Invalid("file name length overflow".into()))?;
        if name_end > data.len() {
            return Err(Error::Invalid(format!(
                "file name at 0x{offset:X} ends outside the table"
            )));
        }
        Ok(FileView {
            sibling: le32(&data[at + 0x04..]),
            offset: le64(&data[at + 0x08..]),
            size: le64(&data[at + 0x10..]),
            name: parse_name(&data[name_at..name_end])?,
        })
    }

    /// Recorre el árbol desde la raíz.
    pub fn tree(&self) -> Result<RomFsTree> {
        let mut out = RomFsTree::default();
        let mut visited: HashSet<u32> = HashSet::new();
        let mut stack: Vec<(u32, String)> = vec![(0, String::new())];

        while let Some((dir_offset, prefix)) = stack.pop() {
            if !visited.insert(dir_offset) {
                return Err(Error::Invalid(format!(
                    "loop in the directory tree at 0x{dir_offset:X}"
                )));
            }
            let dir = self.dir_at(dir_offset)?;

            let mut file = dir.file;
            let mut seen = 0usize;
            while file != ENTRY_EMPTY {
                let entry = self.file_at(file)?;
                let path = join_path(&prefix, entry.name);
                out.files.push(RomFsEntry {
                    path,
                    offset: self
                        .header
                        .data_offset
                        .checked_add(entry.offset)
                        .ok_or_else(|| Error::Invalid("RomFS file offset overflow".into()))?,
                    size: entry.size,
                });
                file = entry.sibling;
                seen += 1;
                if seen > MAX_ENTRIES {
                    return Err(Error::Invalid(
                        "a directory file list does not terminate".into(),
                    ));
                }
            }

            let mut child = dir.child;
            let mut seen = 0usize;
            while child != ENTRY_EMPTY {
                let sub = self.dir_at(child)?;
                if sub.name.is_empty() {
                    return Err(Error::Invalid(format!(
                        "unnamed subdirectory at 0x{child:X}"
                    )));
                }
                stack.push((child, join_path(&prefix, sub.name)));
                child = sub.sibling;
                seen += 1;
                if seen > MAX_ENTRIES {
                    return Err(Error::Invalid(
                        "the subdirectory list does not terminate".into(),
                    ));
                }
            }

            if out.files.len() > MAX_ENTRIES {
                return Err(Error::Invalid(format!(
                    "the RomFS exceeds {MAX_ENTRIES} entries"
                )));
            }
            out.dirs = visited.len() - 1;
        }

        Ok(out)
    }
}

fn parse_name(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes)
        .map_err(|_| Error::Invalid("RomFS entry name that is not UTF-8".into()))
}

fn join_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(data_offset: u64) -> RomFsHeader {
        RomFsHeader {
            header_size: 0x50,
            dir_meta_offset: 0x50,
            dir_meta_size: 0,
            file_meta_offset: 0,
            file_meta_size: 0,
            data_offset,
        }
    }

    fn dir_entry(name: &str, child: u32, sibling: u32, file: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // parent
        out.extend_from_slice(&sibling.to_le_bytes());
        out.extend_from_slice(&child.to_le_bytes());
        out.extend_from_slice(&file.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // hash
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }

    fn file_entry(name: &str, offset: u64, size: u64, sibling: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // parent
        out.extend_from_slice(&sibling.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // hash
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }

    /// Raíz con `main` (0x20 bytes) y `romfs/` con `data.bin` (0x10 bytes).
    fn build_tree() -> RomFs {
        // El hijo de la raíz vive justo después de su propia entrada.
        let root_len = dir_entry("", 0, ENTRY_EMPTY, 0).len();
        let dir_offset = root_len as u32;

        // La raíz apunta al hijo en `dir_offset` y al primer archivo en 0.
        let root = dir_entry("", dir_offset, ENTRY_EMPTY, 0);

        let sub = dir_entry("romfs", ENTRY_EMPTY, ENTRY_EMPTY, 0);
        let mut dir_meta = Vec::new();
        dir_meta.extend_from_slice(&root);
        dir_meta.extend_from_slice(&sub);

        let f0_len = file_entry("main", 0x00, 0x20, ENTRY_EMPTY).len() as u32;
        let f0 = file_entry("main", 0x00, 0x20, ENTRY_EMPTY);
        let f1 = file_entry("data.bin", 0x20, 0x10, ENTRY_EMPTY);
        let mut file_meta = Vec::new();
        file_meta.extend_from_slice(&f0);
        file_meta.extend_from_slice(&f1);

        // El subdirectorio `romfs` apunta al segundo archivo (offset f0_len).
        let sub_start = dir_offset as usize;
        dir_meta[sub_start + 0x0C..sub_start + 0x10].copy_from_slice(&f0_len.to_le_bytes());

        RomFs::new(header(0x1000), dir_meta, file_meta).unwrap()
    }

    #[test]
    fn walks_files_and_directories() {
        let tree = build_tree().tree().unwrap();
        assert_eq!(tree.dirs, 1);
        assert_eq!(tree.files.len(), 2);
        assert_eq!(
            tree.files[0],
            RomFsEntry {
                path: "main".into(),
                offset: 0x1000,
                size: 0x20
            }
        );
        assert_eq!(
            tree.files[1],
            RomFsEntry {
                path: "romfs/data.bin".into(),
                offset: 0x1020,
                size: 0x10
            }
        );
    }

    #[test]
    fn rejects_header_size_other_than_0x50() {
        let mut raw = [0u8; ROMFS_HEADER_LEN];
        raw[0..8].copy_from_slice(&0x60u64.to_le_bytes());
        assert_eq!(
            RomFsHeader::parse(&raw).unwrap_err().code(),
            "E_FORMAT_UNSUPPORTED"
        );
    }

    #[test]
    fn rejects_entry_outside_table() {
        let rf = RomFs::new(header(0), vec![0u8; 0x10], Vec::new()).unwrap_err();
        assert_eq!(rf.code(), "E_LAYOUT_UNEXPECTED");
    }

    #[test]
    fn rejects_cycles() {
        // La raíz se apunta a sí misma como hijo.
        let mut dir_meta = dir_entry("", 0, ENTRY_EMPTY, ENTRY_EMPTY);
        // parent/sibling/child/file: child = 0.
        dir_meta[0x08..0x0C].copy_from_slice(&0u32.to_le_bytes());
        let rf = RomFs::new(header(0), dir_meta, Vec::new()).unwrap();
        assert_eq!(rf.tree().unwrap_err().code(), "E_LAYOUT_UNEXPECTED");
    }
}
