//! Contenedor PFS0 (NSP y particiones de XCI).
//!
//! Formato:
//! ```text
//! 0x00  magic "PFS0"
//! 0x04  u32 num_files
//! 0x08  u32 string_table_size
//! 0x0C  u32 reservado
//! 0x10  entradas (0x18 c/u): u64 data_offset, u64 size, u32 name_offset, u32 reservado
//!       tabla de nombres UTF-8 (terminados en NUL)
//!       datos
//! ```

use std::io::{Read, Seek, SeekFrom};

use crate::{Error, Result};

const PFS0_MAGIC: &[u8; 4] = b"PFS0";
const HEADER_SIZE: u64 = 0x10;
const ENTRY_SIZE: u64 = 0x18;
/// Límite de seguridad: un NSP real no se acerca a esto.
const MAX_FILES: u32 = 100_000;
const MAX_STRING_TABLE: u32 = 16 * 1024 * 1024;

/// Entrada de un PFS0, con offsets ya resueltos contra el inicio del archivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pfs0Entry {
    pub name: String,
    /// Offset absoluto del contenido dentro del archivo contenedor.
    pub offset: u64,
    pub size: u64,
}

impl Pfs0Entry {
    /// Clasificación aproximada por nombre. El tipo real se decide leyendo la NCA.
    pub fn kind(&self) -> EntryKind {
        classify(&self.name)
    }
}

/// Clasificación aproximada por nombre. Sirve igual para PFS0 (NSP) y para
/// las entradas de una partición HFS0 (XCI): en ambos casos las NCA llevan el
/// id de contenido como nombre.
pub fn classify(name: &str) -> EntryKind {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tik") {
        EntryKind::Ticket
    } else if lower.ends_with(".cnmt.nca") {
        EntryKind::Meta
    } else if lower.ends_with(".nca") {
        EntryKind::Content
    } else if lower.ends_with(".xml") {
        EntryKind::Xml
    } else {
        EntryKind::Other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Ticket,
    Meta,
    Content,
    Xml,
    Other,
}

impl EntryKind {
    pub fn label(self) -> &'static str {
        match self {
            EntryKind::Ticket => "ticket",
            EntryKind::Meta => "meta",
            EntryKind::Content => "nca",
            EntryKind::Xml => "xml",
            EntryKind::Other => "otro",
        }
    }
}

/// Contenedor PFS0 leído (solo metadatos; los datos no se cargan).
#[derive(Debug, Clone)]
pub struct Pfs0 {
    pub entries: Vec<Pfs0Entry>,
    /// Offset absoluto donde empiezan los datos de las entradas.
    pub data_offset: u64,
}

impl Pfs0 {
    /// Lee la tabla de un PFS0 desde el inicio de `reader`.
    ///
    /// `file_len` se usa para rechazar offsets fuera del archivo antes de
    /// intentar leerlos.
    pub fn read<R: Read + Seek>(reader: &mut R, file_len: u64) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;

        let mut header = [0u8; 0x10];
        reader.read_exact(&mut header)?;
        if &header[0..4] != PFS0_MAGIC {
            return Err(Error::Invalid("se esperaba magic PFS0".into()));
        }

        let num_files = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let string_table_size = u32::from_le_bytes(header[8..12].try_into().unwrap());

        if num_files > MAX_FILES {
            return Err(Error::Invalid(format!(
                "PFS0 declara {num_files} archivos (máximo {MAX_FILES})"
            )));
        }
        if string_table_size > MAX_STRING_TABLE {
            return Err(Error::Invalid(format!(
                "tabla de nombres de {string_table_size} bytes (máximo {MAX_STRING_TABLE})"
            )));
        }

        let table_size = u64::from(num_files) * ENTRY_SIZE;
        let table_end = HEADER_SIZE
            .checked_add(table_size)
            .and_then(|v| v.checked_add(u64::from(string_table_size)))
            .ok_or_else(|| Error::Invalid("tamaño de tabla desbordado".into()))?;

        if table_end > file_len {
            return Err(Error::Invalid(format!(
                "la tabla PFS0 termina en 0x{table_end:X}, más allá del archivo (0x{file_len:X})"
            )));
        }

        let mut table = vec![0u8; table_size as usize];
        reader.read_exact(&mut table)?;

        let mut names = vec![0u8; string_table_size as usize];
        reader.read_exact(&mut names)?;

        let data_offset = table_end;
        let mut entries = Vec::with_capacity(num_files as usize);

        for i in 0..num_files as usize {
            let raw = &table[i * ENTRY_SIZE as usize..(i + 1) * ENTRY_SIZE as usize];
            let offset = u64::from_le_bytes(raw[0..8].try_into().unwrap());
            let size = u64::from_le_bytes(raw[8..16].try_into().unwrap());
            let name_offset = u32::from_le_bytes(raw[16..20].try_into().unwrap()) as usize;

            let name = read_c_string(&names, name_offset)
                .ok_or_else(|| Error::Invalid(format!("entrada {i}: nombre fuera de la tabla")))?;

            let absolute = data_offset
                .checked_add(offset)
                .ok_or_else(|| Error::Invalid(format!("entrada {i}: offset desbordado")))?;
            let end = absolute
                .checked_add(size)
                .ok_or_else(|| Error::Invalid(format!("entrada {i}: tamaño desbordado")))?;
            if end > file_len {
                return Err(Error::Invalid(format!(
                    "entrada '{name}': rango 0x{absolute:X}..0x{end:X} fuera del archivo (0x{file_len:X})"
                )));
            }

            entries.push(Pfs0Entry {
                name,
                offset: absolute,
                size,
            });
        }

        Ok(Self {
            entries,
            data_offset,
        })
    }

    /// Suma de tamaños de las entradas.
    pub fn content_size(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }
}

/// Lee una cadena UTF-8 terminada en NUL dentro de la tabla de nombres.
fn read_c_string(table: &[u8], offset: usize) -> Option<String> {
    if offset >= table.len() {
        return None;
    }
    let rest = &table[offset..];
    let end = rest.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&rest[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Construye un PFS0 mínimo en memoria.
    fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut names = Vec::new();
        let mut name_offsets = Vec::new();
        for (name, _) in files {
            name_offsets.push(names.len() as u32);
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }

        let header_size = 0x10 + files.len() * 0x18;
        let data_start = header_size + names.len();
        let mut data = Vec::new();
        let mut entries = Vec::new();
        for ((_, content), name_offset) in files.iter().zip(name_offsets) {
            entries.extend_from_slice(&(data.len() as u64).to_le_bytes());
            entries.extend_from_slice(&(content.len() as u64).to_le_bytes());
            entries.extend_from_slice(&name_offset.to_le_bytes());
            entries.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(content);
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"PFS0");
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&entries);
        out.extend_from_slice(&names);
        assert_eq!(out.len(), data_start);
        out.extend_from_slice(&data);
        out
    }

    fn parse(bytes: &[u8]) -> Result<Pfs0> {
        let len = bytes.len() as u64;
        Pfs0::read(&mut Cursor::new(bytes), len)
    }

    #[test]
    fn reads_entries_and_offsets() {
        let buf = build(&[("a.nca", b"hello"), ("b.tik", b"xy")]);
        let pfs0 = parse(&buf).unwrap();
        assert_eq!(pfs0.entries.len(), 2);
        assert_eq!(pfs0.entries[0].name, "a.nca");
        assert_eq!(pfs0.entries[0].size, 5);
        assert_eq!(&buf[pfs0.entries[0].offset as usize..][..5], b"hello");
        assert_eq!(pfs0.entries[1].name, "b.tik");
        assert_eq!(pfs0.content_size(), 7);
    }

    #[test]
    fn classifies_entries() {
        let buf = build(&[
            ("x.cnmt.nca", b""),
            ("y.nca", b""),
            ("z.tik", b""),
            ("other.bin", b""),
        ]);
        let pfs0 = parse(&buf).unwrap();
        let kinds: Vec<_> = pfs0.entries.iter().map(|e| e.kind()).collect();
        assert_eq!(
            kinds,
            vec![
                EntryKind::Meta,
                EntryKind::Content,
                EntryKind::Ticket,
                EntryKind::Other
            ]
        );
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = build(&[("a", b"")]);
        buf[0] = b'X';
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_entry_beyond_file() {
        let mut buf = build(&[("a", b"abc")]);
        // Corrompe el tamaño de la primera entrada a un valor enorme.
        let size_off = 0x10 + 8;
        buf[size_off..size_off + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_absurd_file_count() {
        let mut buf = build(&[("a", b"")]);
        buf[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn empty_container_is_valid() {
        let buf = build(&[]);
        let pfs0 = parse(&buf).unwrap();
        assert!(pfs0.entries.is_empty());
    }
}
