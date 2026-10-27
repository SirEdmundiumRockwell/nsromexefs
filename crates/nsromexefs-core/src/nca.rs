//! Cabecera NCA y acceso a sus secciones.
//!
//! La cabecera (`0xC00` en NCA3) va cifrada con AES-128-XTS y `header_key`.
//! Dentro de ella están las cuatro entradas de sección y las cuatro cabeceras
//! de sección (una por partición).
//!
//! Portado de `hactool` (ISC, SciresM) y contrastado con `libnintendo-hac`
//! (MIT, jakcron). Ver `THIRD-PARTY-NOTICES.md`.

use std::io::{Read, Seek, SeekFrom};

use crate::crypto;
use crate::error::{le32, le64};
use crate::keys::{KeyBank, KeySet};
use crate::ticket::{revision_from_generation, Ticket};
use crate::{Error, Result};

pub const NCA3_HEADER_LEN: usize = 0xC00;
pub const NCA0_HEADER_LEN: usize = 0xA00;
const SECTION_COUNT: usize = 4;
const SECTION_ENTRY_LEN: usize = 0x10;
const FS_HEADER_LEN: usize = 0x200;

/// `fs_header[0x03]`: tipo de sistema de archivos.
pub const FS_TYPE_PFS0: u8 = 2;
pub const FS_TYPE_ROMFS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NcaFormat {
    Nca3,
    Nca2,
    Nca0,
}

impl NcaFormat {
    pub fn header_len(self) -> usize {
        match self {
            NcaFormat::Nca3 | NcaFormat::Nca2 => NCA3_HEADER_LEN,
            NcaFormat::Nca0 => NCA0_HEADER_LEN,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            NcaFormat::Nca3 => "NCA3",
            NcaFormat::Nca2 => "NCA2",
            NcaFormat::Nca0 => "NCA0",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Program,
    Meta,
    Control,
    Manual,
    Data,
    PublicData,
    Other(u8),
}

impl ContentType {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0 => ContentType::Program,
            1 => ContentType::Meta,
            2 => ContentType::Control,
            3 => ContentType::Manual,
            4 => ContentType::Data,
            5 => ContentType::PublicData,
            other => ContentType::Other(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ContentType::Program => "programa",
            ContentType::Meta => "metadatos",
            ContentType::Control => "control",
            ContentType::Manual => "manual",
            ContentType::Data => "datos",
            ContentType::PublicData => "datos públicos",
            ContentType::Other(_) => "desconocido",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionType {
    RomFs,
    Pfs0,
    Other(u8),
}

impl PartitionType {
    fn from_byte(value: u8) -> Self {
        match value {
            0 => PartitionType::RomFs,
            1 => PartitionType::Pfs0,
            other => PartitionType::Other(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PartitionType::RomFs => "RomFS",
            PartitionType::Pfs0 => "PFS0",
            PartitionType::Other(_) => "desconocida",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionType {
    None,
    Xts,
    Ctr,
    Bktr,
    Other(u8),
}

impl EncryptionType {
    fn from_byte(value: u8) -> Self {
        match value {
            1 => EncryptionType::None,
            2 => EncryptionType::Xts,
            3 => EncryptionType::Ctr,
            4 => EncryptionType::Bktr,
            other => EncryptionType::Other(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EncryptionType::None => "sin cifrar",
            EncryptionType::Xts => "AES-XTS",
            EncryptionType::Ctr => "AES-CTR",
            EncryptionType::Bktr => "BKTR (parche)",
            EncryptionType::Other(_) => "desconocido",
        }
    }
}

/// Una partición de la NCA.
#[derive(Debug, Clone)]
pub struct Section {
    pub index: usize,
    /// Offset de la sección **relativo al inicio de la NCA** (unidades de 0x200).
    pub start: u64,
    /// Offset absoluto dentro del contenedor (NSP/XCI) que contiene la NCA.
    pub file_offset: u64,
    pub end: u64,
    pub partition_type: PartitionType,
    pub fs_type: u8,
    pub encryption: EncryptionType,
    /// `hash_info` (0xF8): superbloque PFS0/RomFS/BKTR.
    pub hash_info: [u8; 0xF8],
    /// `patch_info` (0x40): cabeceras de relocalización BKTR.
    pub patch_info: [u8; 0x40],
    /// Bytes crudos de `generation` + `secure_value`.
    sec_generation: u32,
    sec_secure_value: u32,
}

impl Section {
    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// IV de la sección: los 8 bytes de `generation`/`secure_value` invertidos,
    /// seguidos de ceros. El offset se añade al hacer `seek` en el flujo CTR.
    pub fn ctr_iv(&self) -> [u8; 16] {
        let mut raw = [0u8; 8];
        raw[0..4].copy_from_slice(&self.sec_generation.to_le_bytes());
        raw[4..8].copy_from_slice(&self.sec_secure_value.to_le_bytes());

        let mut iv = [0u8; 16];
        for i in 0..8 {
            iv[i] = raw[7 - i];
        }
        iv
    }

    /// IV equivalente al inicio absoluto de la sección.
    pub fn ctr_iv_at_start(&self) -> [u8; 16] {
        let mut iv = self.ctr_iv();
        iv[8..16].copy_from_slice(&(self.start >> 4).to_be_bytes());
        iv
    }

    /// Parte baja de `section_ctr` (`generation`).
    pub fn section_ctr_low(&self) -> u32 {
        self.sec_generation
    }

    /// Superbloque de una sección PFS0, si la sección lo es.
    ///
    /// Ojo: el PFS0 **no** empieza al inicio de la sección. Delante va la tabla
    /// de hashes, y el sistema de archivos arranca en `pfs0_offset` (por
    /// ejemplo, 0x4000 en el logo del programa base). Ver `hactool` `pfs0.h`.
    pub fn pfs0_superblock(&self) -> Option<Pfs0Superblock> {
        if self.partition_type == PartitionType::Pfs0 && self.fs_type == FS_TYPE_PFS0 {
            Some(Pfs0Superblock::parse(&self.hash_info))
        } else {
            None
        }
    }

    /// Offset del PFS0 dentro de la sección (0 si no es una sección PFS0).
    pub fn pfs0_offset(&self) -> u64 {
        self.pfs0_superblock().map_or(0, |sb| sb.pfs0_offset)
    }
}

/// `pfs0_superblock_t` de hactool: cabecera que precede al PFS0 de una NCA.
#[derive(Debug, Clone)]
pub struct Pfs0Superblock {
    /// SHA-256 de la tabla de hashes.
    pub master_hash: [u8; 0x20],
    pub block_size: u32,
    pub hash_table_offset: u64,
    pub hash_table_size: u64,
    pub pfs0_offset: u64,
    pub pfs0_size: u64,
}

impl Pfs0Superblock {
    pub fn parse(data: &[u8]) -> Self {
        let mut master_hash = [0u8; 0x20];
        master_hash.copy_from_slice(&data[0x00..0x20]);
        Self {
            master_hash,
            block_size: le32(&data[0x20..]),
            hash_table_offset: le64(&data[0x28..]),
            hash_table_size: le64(&data[0x30..]),
            pfs0_offset: le64(&data[0x38..]),
            pfs0_size: le64(&data[0x40..]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NcaHeader {
    pub format: NcaFormat,
    pub distribution: u8,
    pub content_type: ContentType,
    pub crypto_type: u8,
    pub crypto_type2: u8,
    pub kaek_index: u8,
    pub nca_size: u64,
    pub title_id: u64,
    pub sdk_version: u32,
    pub rights_id: [u8; 16],
    pub sections: [Option<Section>; SECTION_COUNT],
    pub key_area: [u8; 0x40],
}

impl NcaHeader {
    /// Lee y descifra la cabecera de una NCA que empieza en `base` dentro de
    /// `reader`. `available` es el tamaño de la NCA (entrada del contenedor).
    pub fn read_at<R: Read + Seek>(
        reader: &mut R,
        base: u64,
        available: u64,
        header_key: Option<&[u8; 32]>,
    ) -> Result<Self> {
        if available < NCA0_HEADER_LEN as u64 {
            return Err(Error::Invalid(format!(
                "NCA demasiado pequeña: {available} bytes"
            )));
        }

        let read_len = available.min(NCA3_HEADER_LEN as u64) as usize;
        let mut raw = vec![0u8; read_len];
        reader.seek(SeekFrom::Start(base))?;
        reader.read_exact(&mut raw)?;

        // Una NCA ya descifrada es válida para inspección.
        let magic = &raw[0x200..0x204];
        let plaintext = magic == b"NCA3" || magic == b"NCA2" || magic == b"NCA0";

        let header = if plaintext {
            raw
        } else {
            let key = header_key.ok_or(Error::MissingHeaderKey)?;
            if read_len < NCA0_HEADER_LEN {
                return Err(Error::Invalid(
                    "no hay bytes suficientes para descifrar la cabecera NCA".into(),
                ));
            }
            // NCA3/NCA2: 0xC00 en una sola pasada, sectores de 0x200 desde 0.
            crypto::xts_decrypt(key, &mut raw, 0, 0x200);
            raw
        };

        Self::parse(&header, base)
    }

    fn parse(header: &[u8], base: u64) -> Result<Self> {
        let magic = &header[0x200..0x204];
        let format = match magic {
            b"NCA3" => NcaFormat::Nca3,
            b"NCA2" => NcaFormat::Nca2,
            b"NCA0" => NcaFormat::Nca0,
            _ => {
                return Err(Error::Invalid(
                    "cabecera NCA ilegible tras el descifrado (¿header_key incorrecta?)".into(),
                ))
            }
        };

        if format == NcaFormat::Nca0 {
            return Err(Error::Unsupported(
                "NCA0 es anterior a Switch 1.0; se leerá en un hito posterior".into(),
            ));
        }

        let content_type = ContentType::from_byte(header[0x205]);
        let crypto_type = header[0x206];
        let kaek_index = header[0x207];
        let nca_size = le64(&header[0x208..]);
        let title_id = le64(&header[0x210..]);
        let sdk_version = le32(&header[0x21C..]);
        let crypto_type2 = header[0x220];

        let mut rights_id = [0u8; 16];
        rights_id.copy_from_slice(&header[0x230..0x240]);

        let mut key_area = [0u8; 0x40];
        key_area.copy_from_slice(&header[0x300..0x340]);

        let mut sections: [Option<Section>; SECTION_COUNT] = Default::default();

        for i in 0..SECTION_COUNT {
            let entry = &header[0x240 + i * SECTION_ENTRY_LEN..0x240 + (i + 1) * SECTION_ENTRY_LEN];
            let start_units = le32(&entry[0..4]);
            let end_units = le32(&entry[4..8]);
            if start_units == 0 && end_units == 0 {
                continue;
            }

            let fs = &header[0x400 + i * FS_HEADER_LEN..0x400 + (i + 1) * FS_HEADER_LEN];
            let mut hash_info = [0u8; 0xF8];
            hash_info.copy_from_slice(&fs[0x08..0x100]);
            let mut patch_info = [0u8; 0x40];
            patch_info.copy_from_slice(&fs[0x100..0x140]);

            sections[i] = Some(Section {
                index: i,
                start: u64::from(start_units) * 0x200,
                file_offset: base + u64::from(start_units) * 0x200,
                end: u64::from(end_units) * 0x200,
                partition_type: PartitionType::from_byte(fs[0x02]),
                fs_type: fs[0x03],
                encryption: EncryptionType::from_byte(fs[0x04]),
                hash_info,
                patch_info,
                sec_generation: le32(&fs[0x140..0x144]),
                sec_secure_value: le32(&fs[0x144..0x148]),
            });
        }

        Ok(Self {
            format,
            distribution: header[0x204],
            content_type,
            crypto_type,
            crypto_type2,
            kaek_index,
            nca_size,
            title_id,
            sdk_version,
            rights_id,
            sections,
            key_area,
        })
    }

    /// Revisión indexable en `titlekek_NN` / `master_key_NN`.
    pub fn crypto_revision(&self) -> u8 {
        revision_from_generation(self.crypto_type.max(self.crypto_type2))
    }

    pub fn has_rights_id(&self) -> bool {
        self.rights_id.iter().any(|&b| b != 0)
    }
    pub fn rights_id_hex(&self) -> String {
        self.rights_id.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn title_id_hex(&self) -> String {
        format!("{:016X}", self.title_id)
    }
}

/// De dónde salió la clave de contenido, para diagnóstico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOrigin {
    TitleKeys,
    Ticket,
    KeyArea,
}

impl KeyOrigin {
    pub fn label(self) -> &'static str {
        match self {
            KeyOrigin::TitleKeys => "title.keys",
            KeyOrigin::Ticket => "ticket",
            KeyOrigin::KeyArea => "key area",
        }
    }
}

/// Deriva la clave de contenido de una NCA (diseño §8.1).
pub fn content_key(
    header: &NcaHeader,
    keys: &KeySet,
    ticket: Option<&Ticket>,
) -> Result<([u8; 16], KeyOrigin)> {
    let revision = header.crypto_revision();

    if header.has_rights_id() {
        if let Some(encrypted) = keys.encrypted_titlekey(&header.rights_id) {
            let kek = keys.titlekek(revision).ok_or(Error::MissingTitlekek {
                revision,
                declared: header.crypto_type.max(header.crypto_type2),
            })?;
            let mut key = encrypted;
            crypto::ecb_decrypt_block(&kek, &mut key);
            return Ok((key, KeyOrigin::TitleKeys));
        }

        if let Some(ticket) = ticket.filter(|t| t.rights_id == header.rights_id) {
            return Ok((ticket.content_key(keys)?, KeyOrigin::Ticket));
        }

        return Err(Error::MissingTitlekey {
            rights_id: header.rights_id_hex(),
        });
    }

    let bank = KeyBank::from_index(header.kaek_index).ok_or_else(|| {
        Error::Unsupported(format!("índice de KAEK desconocido: {}", header.kaek_index))
    })?;
    let kaek = keys
        .key_area_key(revision, bank)
        .ok_or(Error::MissingKeyArea {
            bank: bank.label(),
            revision,
        })?;

    let mut area = header.key_area;
    crypto::ecb_decrypt(&kaek, &mut area)?;

    // El key area descifrado son 4 claves de 16 bytes. La de AES-CTR es la
    // tercera (índice 2), igual que en hactool y nstool.
    let mut key = [0u8; 16];
    key.copy_from_slice(&area[0x20..0x30]);
    Ok((key, KeyOrigin::KeyArea))
}

/// Lee un rango de una sección y lo descifra (solo CTR/plaintext).
pub fn read_section<R: Read + Seek>(
    reader: &mut R,
    section: &Section,
    content_key: &[u8; 16],
    relative_offset: u64,
    out: &mut [u8],
) -> Result<()> {
    if relative_offset + out.len() as u64 > section.size() {
        return Err(Error::Invalid(format!(
            "lectura fuera de la sección {}: offset 0x{relative_offset:X} + {} > 0x{:X}",
            section.index,
            out.len(),
            section.size()
        )));
    }

    reader.seek(SeekFrom::Start(section.file_offset + relative_offset))?;
    reader.read_exact(out)?;

    match section.encryption {
        EncryptionType::None => {}
        EncryptionType::Ctr | EncryptionType::Bktr => {
            // Las tablas de un BKTR se leen con el CTR normal de la sección; los
            // bloques parcheados usan CTR-EX y los resuelve `bktr.rs`. hactool
            // hace exactamente esto mientras no ha cargado las subsecciones.
            let iv = section.ctr_iv_at_start();
            crypto::ctr_apply_at(content_key, &iv, relative_offset, out);
        }
        EncryptionType::Xts => {
            return Err(Error::Unsupported(
                "sección XTS fuera de cabecera: no presente en NCA3".into(),
            ))
        }
        EncryptionType::Other(value) => {
            return Err(Error::Unsupported(format!(
                "tipo de cifrado de sección desconocido: {value}"
            )))
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_header(magic: &[u8; 4]) -> Vec<u8> {
        let mut h = vec![0u8; NCA3_HEADER_LEN];
        h[0x200..0x204].copy_from_slice(magic);
        h[0x204] = 0; // distribution
        h[0x205] = 1; // content type: Meta
        h[0x206] = 10; // crypto_type
        h[0x207] = 0; // kaek index
        h[0x208..0x210].copy_from_slice(&0xC00u64.to_le_bytes());
        h[0x210..0x218].copy_from_slice(&0x01006BB00C6F0000u64.to_le_bytes());
        h[0x220] = 10;
        // Sección 0: 0x4000..0x6000, CTR, RomFS.
        h[0x240..0x244].copy_from_slice(&0x20u32.to_le_bytes());
        h[0x244..0x248].copy_from_slice(&0x30u32.to_le_bytes());
        h[0x400 + 0x02] = 0;
        h[0x400 + 0x03] = 3;
        h[0x400 + 0x04] = 3;
        h[0x400 + 0x140..0x400 + 0x144].copy_from_slice(&2u32.to_le_bytes());
        h[0x400 + 0x144..0x400 + 0x148].copy_from_slice(&1u32.to_le_bytes());
        h
    }

    #[test]
    fn parses_plaintext_header() {
        let h = build_header(b"NCA3");
        let nca = NcaHeader::parse(&h, 0x4000).unwrap();
        assert_eq!(nca.format, NcaFormat::Nca3);
        assert_eq!(nca.content_type, ContentType::Meta);
        assert_eq!(nca.crypto_type, 10);
        assert_eq!(nca.crypto_revision(), 9);
        assert_eq!(nca.title_id_hex(), "01006BB00C6F0000");
        assert!(!nca.has_rights_id());

        let section = nca.sections[0].as_ref().unwrap();
        assert_eq!(section.start, 0x4000);
        assert_eq!(section.file_offset, 0x8000);
        assert_eq!(section.end, 0x6000);
        assert_eq!(section.size(), 0x2000);
        assert_eq!(section.encryption, EncryptionType::Ctr);
        assert_eq!(section.partition_type, PartitionType::RomFs);
    }

    #[test]
    fn iv_reverses_generation_and_secure_value() {
        let h = build_header(b"NCA3");
        let nca = NcaHeader::parse(&h, 0x4000).unwrap();
        let section = nca.sections[0].as_ref().unwrap();
        let iv = section.ctr_iv();
        assert_eq!(&iv[0..8], &[0, 0, 0, 1, 0, 0, 0, 2]);
        assert_eq!(&iv[8..16], &[0u8; 8]);

        let at_start = section.ctr_iv_at_start();
        assert_eq!(&at_start[8..16], &(0x4000u64 >> 4).to_be_bytes());
    }

    #[test]
    fn rejects_garbage_header() {
        let h = vec![0u8; NCA3_HEADER_LEN];
        let err = NcaHeader::parse(&h, 0x4000).unwrap_err();
        assert_eq!(err.code(), "E_LAYOUT_UNEXPECTED");
    }

    /// Superbloque PFS0 tal y como lo declara una NCA real: el PFS0 no empieza
    /// al inicio de la sección, sino en `pfs0_offset` (0x4000 en el logo del
    /// programa base de Link's Awakening).
    #[test]
    fn parses_pfs0_superblock() {
        let mut h = build_header(b"NCA3");
        // Sección 0 como PFS0 de NCA.
        h[0x400 + 0x02] = 1; // partition_type = PFS0
        h[0x400 + 0x03] = FS_TYPE_PFS0;
        h[0x400 + 0x04] = 1; // sin cifrar
        let fs = 0x400;
        h[fs + 0x08..fs + 0x28].copy_from_slice(&[0xAB; 0x20]); // master hash
        h[fs + 0x28..fs + 0x2C].copy_from_slice(&0x1000u32.to_le_bytes());
        h[fs + 0x2C..fs + 0x30].copy_from_slice(&2u32.to_le_bytes());
        h[fs + 0x30..fs + 0x38].copy_from_slice(&0u64.to_le_bytes());
        h[fs + 0x38..fs + 0x40].copy_from_slice(&0x240u64.to_le_bytes());
        h[fs + 0x40..fs + 0x48].copy_from_slice(&0x4000u64.to_le_bytes());
        h[fs + 0x48..fs + 0x50].copy_from_slice(&0x117D4u64.to_le_bytes());

        let nca = NcaHeader::parse(&h, 0x8E00).unwrap();
        let section = nca.sections[0].as_ref().unwrap();
        assert_eq!(section.partition_type, PartitionType::Pfs0);
        let sb = section.pfs0_superblock().unwrap();
        assert_eq!(sb.block_size, 0x1000);
        assert_eq!(sb.master_hash, [0xAB; 0x20]);
        assert_eq!(sb.hash_table_size, 0x240);
        assert_eq!(sb.pfs0_offset, 0x4000);
        assert_eq!(sb.pfs0_size, 0x117D4);
        assert_eq!(section.pfs0_offset(), 0x4000);
        assert_eq!(section.file_offset, 0x8E00 + 0x4000);
        assert_eq!(
            section.file_offset + sb.pfs0_offset,
            0x8E00 + 0x4000 + 0x4000
        );
    }

    #[test]
    fn romfs_section_has_no_pfs0_offset() {
        let h = build_header(b"NCA3");
        let nca = NcaHeader::parse(&h, 0x4000).unwrap();
        let section = nca.sections[0].as_ref().unwrap();
        assert!(section.pfs0_superblock().is_none());
        assert_eq!(section.pfs0_offset(), 0);
    }

    #[test]
    fn xts_roundtrip_produces_parseable_header() {
        let key = [0x5Au8; 32];
        let mut plain = build_header(b"NCA3");
        plain.truncate(NCA3_HEADER_LEN);
        let mut enc = plain.clone();
        crypto::xts_encrypt(&key, &mut enc, 0, 0x200);

        let mut cursor = std::io::Cursor::new(enc);
        let nca = NcaHeader::read_at(&mut cursor, 0, NCA3_HEADER_LEN as u64, Some(&key)).unwrap();
        assert_eq!(nca.format, NcaFormat::Nca3);
        assert_eq!(nca.title_id_hex(), "01006BB00C6F0000");
    }

    #[test]
    fn missing_header_key_is_reported() {
        let enc = vec![0u8; NCA3_HEADER_LEN];
        let mut cursor = std::io::Cursor::new(enc);
        let err = NcaHeader::read_at(&mut cursor, 0, NCA3_HEADER_LEN as u64, None).unwrap_err();
        assert_eq!(err.code(), "E_KEY_HEADER_MISSING");
    }
}
