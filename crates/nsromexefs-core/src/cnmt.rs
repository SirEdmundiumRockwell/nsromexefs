//! CNMT (`PackagedContentMeta`): título, versión, tipo y lista de contenidos.
//!
//! El archivo **no tiene magic**: empieza directamente por el title id. Se
//! encuentra dentro de la NCA de metadatos, en su PFS0 interno.
//!
//! Estructuras verificadas contra `switchbrew` y `libnintendo-hac` (§21.1 del
//! diseño).

use crate::error::{le16, le32, le64};
use crate::{Error, Result};

const HEADER_LEN: usize = 0x20;
const CONTENT_INFO_LEN: usize = 0x38;
const CONTENT_META_INFO_LEN: usize = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaType {
    Application,
    Patch,
    AddOnContent,
    Delta,
    DataPatch,
    Other(u8),
}

impl MetaType {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0x80 => MetaType::Application,
            0x81 => MetaType::Patch,
            0x82 => MetaType::AddOnContent,
            0x83 => MetaType::Delta,
            0x84 => MetaType::DataPatch,
            other => MetaType::Other(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MetaType::Application => "base (Application)",
            MetaType::Patch => "update (Patch)",
            MetaType::AddOnContent => "DLC (AddOnContent)",
            MetaType::Delta => "delta",
            MetaType::DataPatch => "data patch",
            MetaType::Other(_) => "unknown",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            MetaType::Application => "base",
            MetaType::Patch => "update",
            MetaType::AddOnContent => "dlc",
            MetaType::Delta => "delta",
            MetaType::DataPatch => "data-patch",
            MetaType::Other(_) => "?",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentInfo {
    pub hash: [u8; 0x20],
    pub content_id: [u8; 0x10],
    /// `size_higher << 32 | size_lower`.
    pub size: u64,
    pub content_type: u8,
    pub id_offset: u8,
}

impl ContentInfo {
    pub fn content_id_hex(&self) -> String {
        self.content_id.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[derive(Debug, Clone)]
pub struct Cnmt {
    pub title_id: u64,
    pub version: u32,
    pub meta_type: MetaType,
    pub attributes: u8,
    pub required_download_version: u32,
    /// Cabecera extendida cruda (su primer u64 identifica la aplicación en
    /// updates y DLC).
    pub extended: Vec<u8>,
    pub contents: Vec<ContentInfo>,
}

impl Cnmt {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_LEN {
            return Err(Error::Invalid(format!(
                "truncated CNMT: {} bytes",
                data.len()
            )));
        }

        let title_id = le64(&data[0x00..]);
        let version = le32(&data[0x08..]);
        let meta_type = MetaType::from_byte(data[0x0C]);
        let extended_len = usize::from(le16(&data[0x0E..]));
        let content_count = usize::from(le16(&data[0x10..]));
        let content_meta_count = usize::from(le16(&data[0x12..]));
        let attributes = data[0x14];
        let required_download_version = le32(&data[0x18..]);

        let contents_start = HEADER_LEN
            .checked_add(extended_len)
            .ok_or_else(|| Error::Invalid("extended header size overflow".into()))?;
        let contents_end = contents_start
            .checked_add(content_count * CONTENT_INFO_LEN)
            .ok_or_else(|| Error::Invalid("content list overflow".into()))?;
        let meta_end = contents_end
            .checked_add(content_meta_count * CONTENT_META_INFO_LEN)
            .ok_or_else(|| Error::Invalid("metadata list overflow".into()))?;

        if meta_end > data.len() {
            return Err(Error::Invalid(format!(
                "CNMT declares 0x{meta_end:X} bytes but only 0x{:X} are available",
                data.len()
            )));
        }

        let extended = data[HEADER_LEN..contents_start].to_vec();

        let mut contents = Vec::with_capacity(content_count);
        for i in 0..content_count {
            let base = contents_start + i * CONTENT_INFO_LEN;
            let raw = &data[base..base + CONTENT_INFO_LEN];

            let mut hash = [0u8; 0x20];
            hash.copy_from_slice(&raw[0..0x20]);
            let mut content_id = [0u8; 0x10];
            content_id.copy_from_slice(&raw[0x20..0x30]);

            let size_lower = u64::from(le32(&raw[0x30..]));
            let size_higher = u64::from(le16(&raw[0x34..]));

            contents.push(ContentInfo {
                hash,
                content_id,
                size: (size_higher << 32) | size_lower,
                content_type: raw[0x36],
                id_offset: raw[0x37],
            });
        }

        Ok(Self {
            title_id,
            version,
            meta_type,
            attributes,
            required_download_version,
            extended,
            contents,
        })
    }

    pub fn title_id_hex(&self) -> String {
        format!("{:016X}", self.title_id)
    }

    /// `major.minor.micro`.
    pub fn version_string(&self) -> String {
        format_version(self.version)
    }

    /// Para updates y DLC, el title id de la aplicación a la que pertenecen.
    pub fn application_id(&self) -> Option<u64> {
        match self.meta_type {
            MetaType::Patch | MetaType::AddOnContent | MetaType::DataPatch | MetaType::Delta => {
                if self.extended.len() >= 8 {
                    Some(le64(&self.extended[0..8]))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Identificador del parche asociado a una aplicación base.
    pub fn patch_id(&self) -> Option<u64> {
        match self.meta_type {
            MetaType::Application => {
                if self.extended.len() >= 8 {
                    Some(le64(&self.extended[0..8]))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

pub fn format_version(version: u32) -> String {
    format!(
        "{}.{}.{}",
        version >> 16,
        (version >> 8) & 0xFF,
        version & 0xFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(meta_type: u8, ex: &[u8], contents: &[(u8, u64)]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_LEN];
        out[0x00..0x08].copy_from_slice(&0x01006BB00C6F0800u64.to_le_bytes());
        out[0x08..0x0C].copy_from_slice(&65792u32.to_le_bytes());
        out[0x0C] = meta_type;
        out[0x0E..0x10].copy_from_slice(&(ex.len() as u16).to_le_bytes());
        out[0x10..0x12].copy_from_slice(&(contents.len() as u16).to_le_bytes());
        out[0x14] = 0;
        out.extend_from_slice(ex);

        for (i, (ctype, size)) in contents.iter().enumerate() {
            let mut c = vec![0u8; CONTENT_INFO_LEN];
            c[0x20] = i as u8;
            c[0x30..0x34].copy_from_slice(&((*size as u32).to_le_bytes()));
            c[0x34..0x36].copy_from_slice(&((*size >> 32) as u16).to_le_bytes());
            c[0x36] = *ctype;
            out.extend_from_slice(&c);
        }
        out
    }

    #[test]
    fn parses_patch_meta() {
        let mut ex = vec![0u8; 0x10];
        ex[0..8].copy_from_slice(&0x01006BB00C6F0000u64.to_le_bytes());
        let data = build(0x81, &ex, &[(0, 0x1234_5678_9ABC), (3, 0x2000)]);
        let cnmt = Cnmt::parse(&data).unwrap();

        assert_eq!(cnmt.title_id_hex(), "01006BB00C6F0800");
        assert_eq!(cnmt.version_string(), "1.1.0");
        assert_eq!(cnmt.meta_type, MetaType::Patch);
        assert_eq!(cnmt.application_id(), Some(0x01006BB00C6F0000));
        assert_eq!(cnmt.contents.len(), 2);
        assert_eq!(cnmt.contents[0].size, 0x1234_5678_9ABC);
        assert_eq!(cnmt.contents[1].content_type, 3);
    }

    #[test]
    fn parses_application_meta() {
        let mut ex = vec![0u8; 0x10];
        ex[0..8].copy_from_slice(&0x01006BB00C6F0800u64.to_le_bytes());
        let data = build(0x80, &ex, &[]);
        let cnmt = Cnmt::parse(&data).unwrap();
        assert_eq!(cnmt.meta_type, MetaType::Application);
        assert_eq!(cnmt.patch_id(), Some(0x01006BB00C6F0800));
        assert_eq!(cnmt.application_id(), None);
    }

    #[test]
    fn formats_versions() {
        assert_eq!(format_version(0), "0.0.0");
        assert_eq!(format_version(65536), "1.0.0");
        assert_eq!(format_version(65792), "1.1.0");
        assert_eq!(format_version(0x0001_0203), "1.2.3");
    }

    #[test]
    fn rejects_truncated_cnmt() {
        let data = build(0x81, &[0u8; 0x10], &[(0, 0x10)]);
        assert!(Cnmt::parse(&data[..0x30]).is_err());
    }

    #[test]
    fn rejects_lying_counts() {
        let mut data = build(0x81, &[], &[]);
        data[0x10..0x12].copy_from_slice(&100u16.to_le_bytes());
        assert!(Cnmt::parse(&data).is_err());
    }

    #[test]
    fn rejects_empty_input() {
        assert!(Cnmt::parse(&[]).is_err());
    }
}
