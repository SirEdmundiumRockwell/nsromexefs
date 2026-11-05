//! Lectura de alto nivel: de una NCA de metadatos a su CNMT.
//!
//! Une las piezas de M1: cabecera NCA (XTS), clave de contenido (title.keys,
//! ticket o key area), sección PFS0 (CTR) y parser del CNMT.

use std::io::{Cursor, Read, Seek};

use crate::cnmt::Cnmt;
use crate::nca::{self, KeyOrigin, NcaHeader};
use crate::pfs0::Pfs0;
use crate::{Error, KeySet, Result};

/// Límite de seguridad: una NCA de metadatos real ronda los 4-20 KB.
const MAX_META_SECTION: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct MetaInfo {
    pub header: NcaHeader,
    pub cnmt: Cnmt,
    pub key_origin: KeyOrigin,
    /// True si el CNMT venía en una sección cifrada.
    pub encrypted: bool,
}

/// Lee la cabecera de la NCA de metadatos, descifra su PFS0 y devuelve el CNMT.
pub fn read_meta<R: Read + Seek>(
    reader: &mut R,
    base: u64,
    nca_size: u64,
    keys: &KeySet,
    ticket: Option<&crate::ticket::Ticket>,
) -> Result<MetaInfo> {
    let header = NcaHeader::read_at(reader, base, nca_size, keys.header_key().as_ref())?;

    if header.content_type != crate::nca::ContentType::Meta {
        return Err(Error::Invalid(format!(
            "expected a metadata NCA but received type {}",
            header.content_type.label()
        )));
    }

    let section = header
        .sections
        .iter()
        .flatten()
        .find(|s| s.partition_type == crate::nca::PartitionType::Pfs0)
        .ok_or_else(|| Error::Invalid("the metadata NCA has no PFS0 section".into()))?;

    if section.size() > MAX_META_SECTION {
        return Err(Error::Invalid(format!(
            "abnormally large metadata section: {} bytes",
            section.size()
        )));
    }

    let (content_key, key_origin) = nca::content_key(&header, keys, ticket)?;

    let mut section_data = vec![0u8; section.size() as usize];
    nca::read_section(reader, section, &content_key, 0, &mut section_data)?;

    let pfs0_offset = section.pfs0_offset();
    if pfs0_offset as usize >= section_data.len() {
        return Err(Error::Invalid(format!(
            "the declared PFS0 (0x{pfs0_offset:X}) falls outside the section (0x{:X} bytes)",
            section_data.len()
        )));
    }
    let partition = &section_data[pfs0_offset as usize..];

    let pfs0 = Pfs0::read(&mut Cursor::new(partition), partition.len() as u64)?;

    let entry = pfs0
        .entries
        .iter()
        .find(|e| e.name.to_ascii_lowercase().ends_with(".cnmt"))
        .ok_or_else(|| Error::Invalid("the metadata PFS0 contains no .cnmt".into()))?;

    let start = pfs0_offset as usize + entry.offset as usize;
    let end = start
        .checked_add(entry.size as usize)
        .ok_or_else(|| Error::Invalid("CNMT size overflow".into()))?;
    if end > section_data.len() {
        return Err(Error::Invalid(
            "the declared CNMT falls outside the section".into(),
        ));
    }

    let cnmt = Cnmt::parse(&section_data[start..end])?;

    Ok(MetaInfo {
        encrypted: section.encryption != crate::nca::EncryptionType::None,
        header,
        cnmt,
        key_origin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use crate::nca::NCA3_HEADER_LEN;
    use std::path::Path;

    const KAEK: [u8; 16] = [0x5A; 16];
    const CONTENT_KEY: [u8; 16] = [0x11; 16];
    const TITLE_ID: u64 = 0x0100_6BB0_0C6F_0800;

    fn keyset() -> KeySet {
        let mut set = KeySet::default();
        let hex: String = KAEK.iter().map(|b| format!("{b:02x}")).collect();
        set.merge_str(
            &format!("key_area_key_application_00 = {hex}\n"),
            Path::new("test.keys"),
        );
        set
    }

    fn cnmt_bytes() -> Vec<u8> {
        let mut out = vec![0u8; 0x20];
        out[0x00..0x08].copy_from_slice(&TITLE_ID.to_le_bytes());
        out[0x08..0x0C].copy_from_slice(&0x0002_0000u32.to_le_bytes());
        out[0x0C] = 0x81; // Patch
        out
    }

    fn pfs0(name: &str, data: &[u8]) -> Vec<u8> {
        let mut names = Vec::new();
        names.extend_from_slice(name.as_bytes());
        names.push(0);

        let mut out = Vec::new();
        out.extend_from_slice(b"PFS0");
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes()); // offset relativo a los datos
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&names);
        out.extend_from_slice(data);
        out
    }

    /// NCA de metadatos sintética con la misma forma que las reales: la sección
    /// empieza por la tabla de hashes y el PFS0 va en `pfs0_offset` (0x20).
    fn build_nca() -> Vec<u8> {
        let pfs = pfs0("meta.cnmt", &cnmt_bytes());

        let mut section = vec![0u8; 0x20];
        section.extend_from_slice(&pfs);
        while section.len() % 0x200 != 0 {
            section.push(0);
        }

        let section_start = 0xC00u64;
        let section_end = section_start + section.len() as u64;

        let mut nca = vec![0u8; NCA3_HEADER_LEN];
        nca[0x200..0x204].copy_from_slice(b"NCA3");
        nca[0x205] = 1; // Meta
        nca[0x208..0x210].copy_from_slice(&section_end.to_le_bytes());
        nca[0x210..0x218].copy_from_slice(&TITLE_ID.to_le_bytes());
        nca[0x240..0x244].copy_from_slice(&((section_start / 0x200) as u32).to_le_bytes());
        nca[0x244..0x248].copy_from_slice(&((section_end / 0x200) as u32).to_le_bytes());

        let mut area = [0u8; 0x40];
        area[0x20..0x30].copy_from_slice(&CONTENT_KEY);
        for block in area.chunks_exact_mut(0x10) {
            let mut b = [0u8; 0x10];
            b.copy_from_slice(block);
            crypto::ecb_encrypt_block(&KAEK, &mut b);
            block.copy_from_slice(&b);
        }
        nca[0x300..0x340].copy_from_slice(&area);

        let fs = 0x400;
        nca[fs + 0x02] = 1; // PARTITION_PFS0
        nca[fs + 0x03] = crate::nca::FS_TYPE_PFS0;
        nca[fs + 0x04] = 1; // sin cifrar
        nca[fs + 0x28..fs + 0x2C].copy_from_slice(&0x1000u32.to_le_bytes());
        nca[fs + 0x2C..fs + 0x30].copy_from_slice(&2u32.to_le_bytes());
        nca[fs + 0x38..fs + 0x40].copy_from_slice(&0x20u64.to_le_bytes());
        nca[fs + 0x40..fs + 0x48].copy_from_slice(&0x20u64.to_le_bytes());
        nca[fs + 0x48..fs + 0x50].copy_from_slice(&(pfs.len() as u64).to_le_bytes());

        nca.extend_from_slice(&section);
        nca
    }

    #[test]
    fn reads_cnmt_located_after_the_hash_table() {
        let nca = build_nca();
        let len = nca.len() as u64;
        let info = read_meta(&mut Cursor::new(nca), 0, len, &keyset(), None).unwrap();
        assert_eq!(info.cnmt.title_id_hex(), "01006BB00C6F0800");
        assert_eq!(info.cnmt.version, 0x0002_0000);
        assert_eq!(info.cnmt.meta_type, crate::cnmt::MetaType::Patch);
        assert_eq!(info.key_origin, KeyOrigin::KeyArea);
        assert!(!info.encrypted);
    }

    #[test]
    fn rejects_non_meta_nca() {
        let mut nca = build_nca();
        nca[0x205] = 0; // Program
        let len = nca.len() as u64;
        let err = read_meta(&mut Cursor::new(nca), 0, len, &keyset(), None).unwrap_err();
        assert_eq!(err.code(), "E_LAYOUT_UNEXPECTED");
    }
}
