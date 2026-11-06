//! Lectura de secciones NCA a través de una abstracción común.
//!
//! Una sección puede ser:
//!
//! - **plana**: CTR o sin cifrar, tal cual está en la NCA (base, DLC, exefs de
//!   cualquier contenedor);
//! - **BKTR**: una RomFS virtual cuyos bloques se reparten entre la sección del
//!   update y la RomFS de la NCA base (ver `bktr.rs`).
//!
//! Extraer y recorrer RomFS no necesita saber cuál es cuál: pide bytes por
//! offset virtual y este módulo los resuelve.

use std::io::{Read, Seek};

use crate::nca::{self, Section};
use crate::Result;

/// Fuente de bytes de una sección, ya descifrada y (si hace falta) fusionada.
pub trait SectionRead {
    /// Número de sección dentro de la NCA (0..3).
    fn index(&self) -> usize;
    /// Superbloque específico (`hash_info`, 0xF8), p. ej. IVFC o PFS0.
    fn hash_info(&self) -> &[u8; 0xF8];
    /// Cabeceras de relocalización BKTR (`patch_info`, 0x40).
    fn patch_info(&self) -> &[u8; 0x40];
    /// Tamaño **virtual** de la sección (el que ven IVFC/RomFS).
    fn size(&self) -> u64;
    /// Offset de la sección dentro de la NCA; base para los IV de CTR.
    fn start(&self) -> u64;
    /// IV inicial de CTR: `section_ctr` invertido y offset de la sección.
    fn ctr_iv(&self) -> [u8; 16];
    /// `section_ctr` bajo (los 4 bytes de `generation`).
    fn section_ctr_low(&self) -> u32;
    /// Clave de contenido con la que se descifra la sección.
    fn content_key(&self) -> &[u8; 16];
    /// Lee `out.len()` bytes en `offset` (relativo a la sección).
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> Result<()>;
    /// Lee bytes **sin descifrar**. Solo lo usan los bloques BKTR: su cifrado
    /// es CTR-EX por subsección, no el CTR normal de la sección.
    fn read_raw_at(&mut self, offset: u64, out: &mut [u8]) -> Result<()>;
}

/// Sección sin fusionar: CTR (o texto plano) sobre un lector.
pub struct PlainSection<'a, R: Read + Seek> {
    reader: &'a mut R,
    section: &'a Section,
    content_key: [u8; 16],
}

impl<'a, R: Read + Seek> PlainSection<'a, R> {
    pub fn new(reader: &'a mut R, section: &'a Section, content_key: &[u8; 16]) -> Self {
        Self {
            reader,
            section,
            content_key: *content_key,
        }
    }

    pub fn section(&self) -> &Section {
        self.section
    }

    pub fn content_key(&self) -> &[u8; 16] {
        &self.content_key
    }
}

impl<R: Read + Seek> SectionRead for PlainSection<'_, R> {
    fn index(&self) -> usize {
        self.section.index
    }

    fn hash_info(&self) -> &[u8; 0xF8] {
        &self.section.hash_info
    }

    fn patch_info(&self) -> &[u8; 0x40] {
        &self.section.patch_info
    }

    fn size(&self) -> u64 {
        self.section.size()
    }

    fn start(&self) -> u64 {
        self.section.start
    }

    fn ctr_iv(&self) -> [u8; 16] {
        self.section.ctr_iv()
    }

    fn section_ctr_low(&self) -> u32 {
        self.section.section_ctr_low()
    }

    fn content_key(&self) -> &[u8; 16] {
        &self.content_key
    }

    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> Result<()> {
        nca::read_section(self.reader, self.section, &self.content_key, offset, out)
    }

    fn read_raw_at(&mut self, offset: u64, out: &mut [u8]) -> Result<()> {
        let end = offset
            .checked_add(out.len() as u64)
            .ok_or_else(|| crate::Error::Invalid("section read overflow".into()))?;
        if end > self.section.size() {
            return Err(crate::Error::Invalid(format!(
                "read 0x{offset:X}..0x{end:X} outside section {} (0x{:X} bytes)",
                self.section.index,
                self.section.size()
            )));
        }
        self.reader
            .seek(std::io::SeekFrom::Start(self.section.file_offset + offset))?;
        self.reader.read_exact(out)?;
        Ok(())
    }
}
