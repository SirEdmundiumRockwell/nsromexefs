//! Cartucho XCI: cabecera gamecard (`HEAD`) y HFS0 raíz con particiones.
//!
//! ```text
//! 0x000  firma (0x100)
//! 0x100  "HEAD"
//! 0x104  u32 secure_offset
//! 0x118  u64 cart_size
//! 0x120  [u8; 0x10] reversed_iv
//! 0x130  u64 hfs0_offset
//! 0x138  u64 hfs0_header_size
//! 0x140  [u8; 0x20] hfs0_header_hash
//! ...
//! hfs0_offset  HFS0 raíz: update / normal / secure / logo
//! ```
//!
//! La partición `secure` contiene las NCA del juego (equivalente a un NSP);
//! `update` es un NSP delta con su propio CNMT. Un cartucho recortado puede
//! declarar particiones con tamaño cero: simplemente no están.

use std::io::{Read, Seek, SeekFrom};

use crate::hfs0::Hfs0;
use crate::{Error, Result};

/// Offset del magic `HEAD` dentro del archivo.
pub const XCI_MAGIC_OFFSET: u64 = 0x100;
const XCI_MAGIC: &[u8; 4] = b"HEAD";
const HEADER_READ_SIZE: usize = 0x200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XciHeader {
    pub secure_offset: u32,
    pub cart_size: u64,
    pub hfs0_offset: u64,
    pub hfs0_header_size: u64,
    pub hfs0_header_hash: [u8; 0x20],
}

impl XciHeader {
    /// Interpreta los primeros 0x200 bytes del archivo.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_READ_SIZE {
            return Err(Error::Invalid(format!(
                "truncated XCI header: {} bytes (minimum 0x200)",
                data.len()
            )));
        }
        if &data[0x100..0x104] != XCI_MAGIC {
            return Err(Error::Invalid(
                "expected HEAD magic at 0x100: not an XCI".into(),
            ));
        }

        let mut hfs0_header_hash = [0u8; 0x20];
        hfs0_header_hash.copy_from_slice(&data[0x140..0x160]);

        Ok(Self {
            secure_offset: u32::from_le_bytes(data[0x104..0x108].try_into().unwrap()),
            cart_size: u64::from_le_bytes(data[0x118..0x120].try_into().unwrap()),
            hfs0_offset: u64::from_le_bytes(data[0x130..0x138].try_into().unwrap()),
            hfs0_header_size: u64::from_le_bytes(data[0x138..0x140].try_into().unwrap()),
            hfs0_header_hash,
        })
    }
}

/// Partición del HFS0 raíz, ya parseada como HFS0 anidado.
#[derive(Debug, Clone)]
pub struct XciPartition {
    pub name: String,
    /// Tamaño declarado en la entrada del HFS0 raíz.
    pub size: u64,
    pub hfs0: Hfs0,
}

#[derive(Debug, Clone)]
pub struct Xci {
    pub header: XciHeader,
    pub root: Hfs0,
    pub partitions: Vec<XciPartition>,
    /// Particiones declaradas con datos que no se pudieron leer.
    pub partition_errors: Vec<(String, String)>,
}

impl Xci {
    pub fn read<R: Read + Seek>(reader: &mut R, file_len: u64) -> Result<Self> {
        if file_len < HEADER_READ_SIZE as u64 {
            return Err(Error::Invalid(format!(
                "file too small to be an XCI: {file_len} bytes"
            )));
        }

        reader.seek(SeekFrom::Start(0))?;
        let mut header_bytes = [0u8; HEADER_READ_SIZE];
        reader.read_exact(&mut header_bytes)?;
        let header = XciHeader::parse(&header_bytes)?;

        let root = Hfs0::read_at(reader, header.hfs0_offset, file_len)?;

        let mut partitions = Vec::new();
        let mut partition_errors = Vec::new();
        for entry in &root.entries {
            // Un cartucho recortado deja las particiones que no usa a tamaño 0.
            if entry.size < 0x10 {
                continue;
            }
            let end = entry.offset.saturating_add(entry.size);
            match Hfs0::read_at(reader, entry.offset, end) {
                Ok(hfs0) => partitions.push(XciPartition {
                    name: entry.name.clone(),
                    size: entry.size,
                    hfs0,
                }),
                Err(err) => partition_errors.push((entry.name.clone(), err.to_string())),
            }
        }

        Ok(Self {
            header,
            root,
            partitions,
            partition_errors,
        })
    }

    /// Partición por nombre (`secure`, `update`, `normal`, `logo`).
    pub fn partition(&self, name: &str) -> Option<&XciPartition> {
        self.partitions
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Partición `secure`, que es la que contiene el juego.
    pub fn secure(&self) -> Option<&XciPartition> {
        self.partition("secure")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Construye un HFS0 con entradas en el offset 0 (sin prefijo).
    fn build_hfs0(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut names = Vec::new();
        let mut name_offsets = Vec::new();
        for (name, _) in files {
            name_offsets.push(names.len() as u32);
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }

        let data_start = 0x10 + files.len() * 0x40 + names.len();
        let mut table = Vec::new();
        let mut data = Vec::new();
        for ((_, content), name_offset) in files.iter().zip(name_offsets) {
            table.extend_from_slice(&(data.len() as u64).to_le_bytes());
            table.extend_from_slice(&(content.len() as u64).to_le_bytes());
            table.extend_from_slice(&name_offset.to_le_bytes());
            table
                .extend_from_slice(&((content.len() as u32).div_ceil(0x200) * 0x200).to_le_bytes());
            table.extend_from_slice(&0u64.to_le_bytes());
            table.extend_from_slice(&[0u8; 0x20]);
            data.extend_from_slice(content);
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"HFS0");
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&table);
        out.extend_from_slice(&names);
        assert_eq!(out.len(), data_start);
        out.extend_from_slice(&data);
        out
    }

    /// Compone un XCI con el HFS0 raíz en 0x200 y las particiones dadas.
    fn build_xci(partitions: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let refs: Vec<(&str, &[u8])> = partitions
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice()))
            .collect();
        let root = build_hfs0(&refs);

        let mut header = vec![0u8; 0x200];
        header[0x100..0x104].copy_from_slice(b"HEAD");
        header[0x104..0x108].copy_from_slice(&0x200u32.to_le_bytes());
        header[0x118..0x120].copy_from_slice(&((0x200 + root.len()) as u64).to_le_bytes());
        header[0x130..0x138].copy_from_slice(&0x200u64.to_le_bytes());
        header[0x138..0x140].copy_from_slice(&(root.len() as u64).to_le_bytes());
        header[0x140..0x160].copy_from_slice(&[0xAB; 0x20]);

        let mut out = header;
        out.extend_from_slice(&root);
        out
    }

    fn parse(bytes: &[u8]) -> Result<Xci> {
        let len = bytes.len() as u64;
        Xci::read(&mut Cursor::new(bytes), len)
    }

    #[test]
    fn reads_header_fields() {
        let xci_bytes = build_xci(&[]);
        let xci = parse(&xci_bytes).unwrap();
        assert_eq!(xci.header.secure_offset, 0x200);
        assert_eq!(xci.header.hfs0_offset, 0x200);
        assert_eq!(xci.header.cart_size, xci_bytes.len() as u64);
        assert_eq!(xci.header.hfs0_header_hash, [0xAB; 0x20]);
    }

    #[test]
    fn reads_secure_partition_and_resolves_offsets() {
        let nested = build_hfs0(&[("abc.nca", b"hello")]);
        let xci_bytes = build_xci(&[("secure", nested)]);
        let xci = parse(&xci_bytes).unwrap();

        let secure = xci.secure().expect("partición secure");
        assert_eq!(secure.hfs0.entries.len(), 1);
        let nca = &secure.hfs0.entries[0];
        assert_eq!(nca.name, "abc.nca");
        assert_eq!(nca.size, 5);
        assert_eq!(&xci_bytes[nca.offset as usize..][..5], b"hello");
        // El offset debe caer después de la cabecera del XCI.
        assert!(nca.offset > 0x200);
    }

    #[test]
    fn reads_multiple_partitions() {
        let secure = build_hfs0(&[("game.nca", b"game")]);
        let update = build_hfs0(&[("patch.nca", b"patch")]);
        let xci_bytes = build_xci(&[("secure", secure), ("update", update)]);
        let xci = parse(&xci_bytes).unwrap();

        assert_eq!(xci.partitions.len(), 2);
        assert!(xci.partition("update").is_some());
        assert_eq!(
            xci.partition("update").unwrap().hfs0.entries[0].name,
            "patch.nca"
        );
        assert!(xci.partition("logo").is_none());
        assert!(xci.partition_errors.is_empty());
    }

    #[test]
    fn skips_zero_sized_partitions() {
        let secure = build_hfs0(&[("game.nca", b"game")]);
        let xci_bytes = build_xci(&[("secure", secure), ("normal", Vec::new())]);
        let xci = parse(&xci_bytes).unwrap();
        assert!(xci.secure().is_some());
        assert!(xci.partition("normal").is_none());
        assert!(xci.partition_errors.is_empty());
    }

    #[test]
    fn records_partition_parse_errors_without_losing_secure() {
        let secure = build_hfs0(&[("game.nca", b"game")]);
        let garbage = vec![0x5Au8; 0x40];
        let xci_bytes = build_xci(&[("secure", secure), ("update", garbage)]);
        let xci = parse(&xci_bytes).unwrap();
        assert!(xci.secure().is_some());
        assert!(xci.partition("update").is_none());
        assert_eq!(xci.partition_errors.len(), 1);
        assert_eq!(xci.partition_errors[0].0, "update");
    }

    #[test]
    fn rejects_bad_magic() {
        let mut xci_bytes = build_xci(&[]);
        xci_bytes[0x100] = b'X';
        assert!(parse(&xci_bytes).is_err());
    }

    #[test]
    fn rejects_truncated_file() {
        let xci_bytes = build_xci(&[]);
        let len = 0x100;
        assert!(Xci::read(&mut Cursor::new(&xci_bytes), len).is_err());
    }

    #[test]
    fn rejects_root_hfs0_beyond_file() {
        let mut xci_bytes = build_xci(&[]);
        xci_bytes[0x130..0x138].copy_from_slice(&0xFFFF_0000u64.to_le_bytes());
        assert!(parse(&xci_bytes).is_err());
    }
}
