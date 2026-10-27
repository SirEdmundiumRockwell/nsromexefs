//! Cabecera IVFC: el índice de niveles que precede a un RomFS.
//!
//! En una NCA, el superbloque de una sección RomFS empieza en
//! `hash_info[0x00]` (es decir, `fs_header + 0x08`) y **es** una cabecera IVFC
//! de `0xE0` bytes. El nivel 6 (índice 5) describe el propio RomFS: su
//! `logical_offset` es el offset del RomFS dentro de la sección.
//!
//! **El campo `num_levels` no es de fiar.** Las NCA reales declaran `7`, pero
//! la cabecera solo tiene espacio para seis `ivfc_level_hdr_t` (`0x10 + 6 ×
//! 0x18 = 0xA0`, y en `0xA0` empieza el relleno). hactool ignora el campo y
//! recorre siempre `IVFC_MAX_LEVEL = 6`. Hacer lo mismo es lo correcto: se
//! valida que el valor declarado sea plausible (1..=7) y se leen **siempre**
//! seis niveles.
//!
//! Portado de `hactool` (ISC, SciresM) `ivfc.h` / `nca.c`.

use crate::error::le32;
use crate::{Error, Result};

pub const IVFC_MAGIC: &[u8; 4] = b"IVFC";
pub const IVFC_HEADER_LEN: usize = 0xE0;
/// Niveles que se leen siempre (y que la cabecera puede contener).
pub const IVFC_MAX_LEVELS: usize = 6;
/// Valor máximo que puede declarar `num_levels` sin ser absurdo.
pub const IVFC_DECLARED_MAX_LEVELS: u32 = 7;

/// Cabecera de un nivel IVFC (`ivfc_level_hdr_t`, 0x18 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IvfcLevel {
    /// Offset de los datos del nivel **relativo al inicio de la sección**.
    pub logical_offset: u64,
    /// Tamaño de los datos del nivel.
    pub hash_data_size: u64,
    /// `log2` del tamaño de bloque.
    pub block_size_log2: u32,
}

impl IvfcLevel {
    pub fn block_size(&self) -> u64 {
        1u64 << self.block_size_log2
    }
}

/// Cabecera IVFC completa.
#[derive(Debug, Clone)]
pub struct Ivfc {
    pub version: u32,
    pub master_hash_size: u32,
    /// Valor tal cual venía en la cabecera (las NCA reales ponen `7`).
    /// `levels` tiene siempre [`IVFC_MAX_LEVELS`] entradas.
    pub declared_levels: u32,
    pub levels: Vec<IvfcLevel>,
    pub master_hash: [u8; 0x20],
}

impl Ivfc {
    /// Parsea `0xE0` bytes (o más; se ignoran los extra).
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < IVFC_HEADER_LEN {
            return Err(Error::Invalid(format!(
                "cabecera IVFC truncada: {} bytes (se esperaban 0x{IVFC_HEADER_LEN:X})",
                data.len()
            )));
        }
        if &data[0..4] != IVFC_MAGIC {
            return Err(Error::Invalid(
                "se esperaba magic IVFC en el superbloque de la sección".into(),
            ));
        }

        let version = le32(&data[0x04..]);
        let master_hash_size = le32(&data[0x08..]);
        let declared_levels = le32(&data[0x0C..]);

        if declared_levels == 0 || declared_levels > IVFC_DECLARED_MAX_LEVELS {
            return Err(Error::Invalid(format!(
                "IVFC declara {declared_levels} niveles (rango conocido 1..={IVFC_DECLARED_MAX_LEVELS})"
            )));
        }
        if master_hash_size != 0x20 {
            return Err(Error::Invalid(format!(
                "IVFC declara un hash maestro de {master_hash_size} bytes (se esperaban 32)"
            )));
        }

        // Se leen siempre seis: `num_levels` no cabe en la cabecera si es 7.
        let mut levels = Vec::with_capacity(IVFC_MAX_LEVELS);
        for i in 0..IVFC_MAX_LEVELS {
            let at = 0x10 + i * 0x18;
            let level = IvfcLevel {
                logical_offset: crate::error::le64(&data[at..]),
                hash_data_size: crate::error::le64(&data[at + 0x08..]),
                block_size_log2: le32(&data[at + 0x10..]),
            };
            if level.block_size_log2 > 32 {
                return Err(Error::Invalid(format!(
                    "nivel IVFC {i}: tamaño de bloque 2^{} fuera de rango",
                    level.block_size_log2
                )));
            }
            levels.push(level);
        }

        let mut master_hash = [0u8; 0x20];
        master_hash.copy_from_slice(&data[0xC0..0xE0]);

        Ok(Self {
            version,
            master_hash_size,
            declared_levels,
            levels,
            master_hash,
        })
    }

    /// Nivel que describe el sistema de archivos (índice 5, como hactool).
    pub fn data_level(&self) -> &IvfcLevel {
        // `parse` siempre deja exactamente `IVFC_MAX_LEVELS` entradas.
        &self.levels[IVFC_MAX_LEVELS - 1]
    }

    /// Offset del RomFS **relativo al inicio de la sección**.
    pub fn fs_offset(&self) -> u64 {
        self.data_level().logical_offset
    }

    /// Tamaño total del RomFS (cabecera, tablas y datos).
    pub fn fs_size(&self) -> u64 {
        self.data_level().hash_data_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> Vec<u8> {
        let mut h = vec![0u8; IVFC_HEADER_LEN];
        h[0..4].copy_from_slice(b"IVFC");
        h[0x04..0x08].copy_from_slice(&0x0002_0000u32.to_le_bytes());
        h[0x08..0x0C].copy_from_slice(&0x20u32.to_le_bytes());
        // Igual que las NCA reales: declara 7 aunque solo quepan 6.
        h[0x0C..0x10].copy_from_slice(&7u32.to_le_bytes());
        for i in 0..6 {
            let at = 0x10 + i * 0x18;
            h[at..at + 8].copy_from_slice(&((i as u64) * 0x1000).to_le_bytes());
            h[at + 8..at + 16].copy_from_slice(&((6 - i as u64) * 0x1000).to_le_bytes());
            h[at + 16..at + 20].copy_from_slice(&12u32.to_le_bytes());
        }
        h[0xC0..0xE0].copy_from_slice(&[0xAB; 0x20]);
        h
    }

    #[test]
    fn parses_levels_and_romfs_offset() {
        let ivfc = Ivfc::parse(&build()).unwrap();
        assert_eq!(ivfc.version, 0x0002_0000);
        assert_eq!(ivfc.master_hash_size, 0x20);
        assert_eq!(ivfc.declared_levels, 7);
        // Siempre seis, aunque declare siete.
        assert_eq!(ivfc.levels.len(), IVFC_MAX_LEVELS);
        assert_eq!(ivfc.levels[5].logical_offset, 0x5000);
        assert_eq!(ivfc.levels[5].block_size(), 0x1000);
        assert_eq!(ivfc.fs_offset(), 0x5000);
        assert_eq!(ivfc.fs_size(), 0x1000);
        assert_eq!(ivfc.master_hash, [0xAB; 0x20]);
    }

    #[test]
    fn declares_six_levels_also_works() {
        let mut h = build();
        h[0x0C..0x10].copy_from_slice(&6u32.to_le_bytes());
        let ivfc = Ivfc::parse(&h).unwrap();
        assert_eq!(ivfc.declared_levels, 6);
        assert_eq!(ivfc.levels.len(), IVFC_MAX_LEVELS);
        assert_eq!(ivfc.fs_offset(), 0x5000);
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut h = build();
        h[0..4].copy_from_slice(b"ROMF");
        assert_eq!(Ivfc::parse(&h).unwrap_err().code(), "E_LAYOUT_UNEXPECTED");
    }

    #[test]
    fn rejects_absurd_level_count() {
        let mut h = build();
        h[0x0C..0x10].copy_from_slice(&8u32.to_le_bytes());
        assert!(Ivfc::parse(&h).is_err());

        h[0x0C..0x10].copy_from_slice(&0u32.to_le_bytes());
        assert!(Ivfc::parse(&h).is_err());
    }

    #[test]
    fn rejects_truncated_header() {
        let h = build();
        assert!(Ivfc::parse(&h[..0x80]).is_err());
    }
}
