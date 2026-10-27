//! Contenedor HFS0 (particiones de un XCI).
//!
//! Formato:
//! ```text
//! 0x00  magic "HFS0"
//! 0x04  u32 num_files
//! 0x08  u32 string_table_size
//! 0x0C  u32 reservado
//! 0x10  entradas (0x40 c/u):
//!         u64 offset (relativo al inicio de datos)
//!         u64 size
//!         u32 string_table_offset
//!         u32 hashed_size
//!         u64 reservado
//!         [u8; 0x20] hash SHA-256 de los datos (alineados a 0x200)
//!       tabla de nombres UTF-8 (terminados en NUL)
//!       datos
//! ```
//!
//! El HFS0 puede empezar en cualquier offset del archivo (en un XCI la raíz
//! está en `hfs0_offset` y cada partición es un HFS0 anidado). Todos los
//! offsets que devuelve este módulo son **absolutos** dentro del archivo.

use std::io::{Read, Seek, SeekFrom};

use crate::{Error, Result};

const HFS0_MAGIC: &[u8; 4] = b"HFS0";
const HEADER_SIZE: u64 = 0x10;
const ENTRY_SIZE: u64 = 0x40;
/// Un XCI real declara menos de diez particiones; el límite es defensivo.
const MAX_FILES: u32 = 100_000;
const MAX_STRING_TABLE: u32 = 16 * 1024 * 1024;

/// Entrada de un HFS0, con el offset ya resuelto contra el inicio del archivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hfs0Entry {
    pub name: String,
    /// Offset absoluto del contenido dentro del archivo contenedor.
    pub offset: u64,
    pub size: u64,
    /// Longitud cubierta por `hash` (el tamaño alineado a 0x200).
    pub hashed_size: u32,
    pub hash: [u8; 0x20],
}

/// HFS0 leído (solo metadatos; los datos no se cargan).
#[derive(Debug, Clone)]
pub struct Hfs0 {
    /// Offset absoluto donde empieza la cabecera HFS0.
    pub base: u64,
    /// `0x10 + num_files * 0x40 + string_table_size`.
    pub header_size: u64,
    /// Offset absoluto donde empiezan los datos de las entradas.
    pub data_offset: u64,
    pub entries: Vec<Hfs0Entry>,
}

impl Hfs0 {
    /// Lee un HFS0 que empieza en `base`, sin leer más allá de `file_len`.
    ///
    /// `file_len` es el límite válido: el tamaño del archivo o, en un HFS0
    /// anidado, el final de la partición que lo contiene.
    pub fn read_at<R: Read + Seek>(reader: &mut R, base: u64, file_len: u64) -> Result<Self> {
        if base
            .checked_add(HEADER_SIZE)
            .is_none_or(|end| end > file_len)
        {
            return Err(Error::Invalid(format!(
                "el HFS0 en 0x{base:X} queda fuera del contenedor (0x{file_len:X})"
            )));
        }

        reader.seek(SeekFrom::Start(base))?;
        let mut header = [0u8; HEADER_SIZE as usize];
        reader.read_exact(&mut header)?;
        if &header[0..4] != HFS0_MAGIC {
            return Err(Error::Invalid(format!(
                "se esperaba magic HFS0 en 0x{base:X}"
            )));
        }

        let num_files = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let string_table_size = u32::from_le_bytes(header[8..12].try_into().unwrap());

        if num_files > MAX_FILES {
            return Err(Error::Invalid(format!(
                "HFS0 declara {num_files} archivos (máximo {MAX_FILES})"
            )));
        }
        if string_table_size > MAX_STRING_TABLE {
            return Err(Error::Invalid(format!(
                "tabla de nombres HFS0 de {string_table_size} bytes (máximo {MAX_STRING_TABLE})"
            )));
        }

        let table_size = u64::from(num_files) * ENTRY_SIZE;
        let header_size = HEADER_SIZE
            .checked_add(table_size)
            .and_then(|v| v.checked_add(u64::from(string_table_size)))
            .ok_or_else(|| Error::Invalid("tamaño de cabecera HFS0 desbordado".into()))?;
        let data_offset = base
            .checked_add(header_size)
            .ok_or_else(|| Error::Invalid("offset de datos HFS0 desbordado".into()))?;
        if data_offset > file_len {
            return Err(Error::Invalid(format!(
                "la cabecera HFS0 termina en 0x{data_offset:X}, más allá del contenedor (0x{file_len:X})"
            )));
        }

        let mut table = vec![0u8; table_size as usize];
        reader.read_exact(&mut table)?;
        let mut names = vec![0u8; string_table_size as usize];
        reader.read_exact(&mut names)?;

        let mut entries = Vec::with_capacity(num_files as usize);
        for i in 0..num_files as usize {
            let raw = &table[i * ENTRY_SIZE as usize..(i + 1) * ENTRY_SIZE as usize];
            let offset = u64::from_le_bytes(raw[0..8].try_into().unwrap());
            let size = u64::from_le_bytes(raw[8..16].try_into().unwrap());
            let name_offset = u32::from_le_bytes(raw[16..20].try_into().unwrap()) as usize;
            let hashed_size = u32::from_le_bytes(raw[20..24].try_into().unwrap());
            let mut hash = [0u8; 0x20];
            hash.copy_from_slice(&raw[32..64]);

            let name = read_c_string(&names, name_offset).ok_or_else(|| {
                Error::Invalid(format!("entrada HFS0 {i}: nombre fuera de la tabla"))
            })?;

            let absolute = data_offset.checked_add(offset).ok_or_else(|| {
                Error::Invalid(format!("entrada HFS0 '{name}': offset desbordado"))
            })?;
            let end = absolute.checked_add(size).ok_or_else(|| {
                Error::Invalid(format!("entrada HFS0 '{name}': tamaño desbordado"))
            })?;
            if end > file_len {
                return Err(Error::Invalid(format!(
                    "entrada HFS0 '{name}': rango 0x{absolute:X}..0x{end:X} fuera del contenedor (0x{file_len:X})"
                )));
            }

            entries.push(Hfs0Entry {
                name,
                offset: absolute,
                size,
                hashed_size,
                hash,
            });
        }

        Ok(Self {
            base,
            header_size,
            data_offset,
            entries,
        })
    }

    /// Busca una entrada por nombre exacto (las particiones del XCI no
    /// distinguen mayúsculas de forma significativa, pero se conserva el
    /// nombre tal cual viene en el contenedor).
    pub fn find(&self, name: &str) -> Option<&Hfs0Entry> {
        self.entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    pub fn content_size(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }
}

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

    /// Construye un HFS0 en `base` dentro de un archivo mayor.
    fn build_at(base: usize, files: &[(&str, &[u8])]) -> (Vec<u8>, Vec<u64>) {
        let mut names = Vec::new();
        let mut name_offsets = Vec::new();
        for (name, _) in files {
            name_offsets.push(names.len() as u32);
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }

        let header_size = 0x10 + files.len() * 0x40;
        let data_start = header_size + names.len();
        let mut table = Vec::new();
        let mut data = Vec::new();
        let mut absolutes = Vec::new();
        for ((_, content), name_offset) in files.iter().zip(name_offsets) {
            let hashed = (content.len() as u32).div_ceil(0x200) * 0x200;
            table.extend_from_slice(&(data.len() as u64).to_le_bytes());
            table.extend_from_slice(&(content.len() as u64).to_le_bytes());
            table.extend_from_slice(&name_offset.to_le_bytes());
            table.extend_from_slice(&hashed.to_le_bytes());
            table.extend_from_slice(&0u64.to_le_bytes());
            table.extend_from_slice(&[0u8; 0x20]);
            absolutes.push((base + data_start + data.len()) as u64);
            data.extend_from_slice(content);
        }

        let mut out = vec![0u8; base];
        out.extend_from_slice(b"HFS0");
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&table);
        out.extend_from_slice(&names);
        assert_eq!(out.len(), base + data_start);
        out.extend_from_slice(&data);
        (out, absolutes)
    }

    fn parse(bytes: &[u8], base: u64) -> Result<Hfs0> {
        let len = bytes.len() as u64;
        Hfs0::read_at(&mut Cursor::new(bytes), base, len)
    }

    #[test]
    fn reads_entries_at_zero() {
        let (buf, absolutes) = build_at(0, &[("secure", b"hello"), ("logo", b"xy")]);
        let hfs0 = parse(&buf, 0).unwrap();
        assert_eq!(hfs0.entries.len(), 2);
        assert_eq!(hfs0.entries[0].name, "secure");
        assert_eq!(hfs0.entries[0].offset, absolutes[0]);
        assert_eq!(&buf[absolutes[0] as usize..][..5], b"hello");
        assert_eq!(hfs0.entries[0].hashed_size, 0x200);
        assert_eq!(hfs0.content_size(), 7);
    }

    #[test]
    fn resolves_offsets_against_base() {
        let (buf, absolutes) = build_at(0x400, &[("update", b"abcd")]);
        let hfs0 = parse(&buf, 0x400).unwrap();
        assert_eq!(hfs0.base, 0x400);
        assert_eq!(hfs0.entries[0].offset, absolutes[0]);
        assert_eq!(&buf[hfs0.entries[0].offset as usize..][..4], b"abcd");
        // El offset es absoluto, no relativo al HFS0.
        assert!(hfs0.entries[0].offset > 0x400);
    }

    #[test]
    fn finds_partition_by_name() {
        let (buf, _) = build_at(0, &[("secure", b"a"), ("normal", b"")]);
        let hfs0 = parse(&buf, 0).unwrap();
        assert_eq!(hfs0.find("SECURE").unwrap().name, "secure");
        assert!(hfs0.find("update").is_none());
    }

    #[test]
    fn shared_prefix_is_not_treated_as_header() {
        let (buf, _) = build_at(0, &[("secure", b"a")]);
        // Sin desplazamiento, el parser debe rechazar basura inicial.
        let mut prefixed = vec![0xFFu8; 0x200];
        prefixed.extend_from_slice(&buf);
        assert!(parse(&prefixed, 0).is_err());
        // Y aceptarlo en su offset real.
        assert!(parse(&prefixed, 0x200).is_ok());
    }

    #[test]
    fn rejects_bad_magic() {
        let (mut buf, _) = build_at(0, &[("a", b"")]);
        buf[0] = b'X';
        assert!(parse(&buf, 0).is_err());
    }

    #[test]
    fn rejects_entry_beyond_container() {
        let (mut buf, _) = build_at(0, &[("a", b"abc")]);
        let size_off = 0x10 + 8;
        buf[size_off..size_off + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(parse(&buf, 0).is_err());
    }

    #[test]
    fn rejects_absurd_file_count() {
        let (mut buf, _) = build_at(0, &[("a", b"")]);
        buf[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&buf, 0).is_err());
    }

    #[test]
    fn rejects_header_beyond_container() {
        let (buf, _) = build_at(0, &[("a", b"")]);
        let len = buf.len() as u64;
        assert!(Hfs0::read_at(&mut Cursor::new(&buf), len, len).is_err());
    }

    #[test]
    fn empty_container_is_valid() {
        let (buf, _) = build_at(0, &[]);
        let hfs0 = parse(&buf, 0).unwrap();
        assert!(hfs0.entries.is_empty());
        assert_eq!(hfs0.header_size, 0x10);
    }
}
