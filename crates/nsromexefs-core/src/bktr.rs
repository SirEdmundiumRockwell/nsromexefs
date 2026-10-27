//! Fusión de parches BKTR (RomFS de un update) contra la RomFS base.
//!
//! Una sección `BKTR` no contiene un RomFS completo: contiene tablas que dicen,
//! para cada bloque virtual, si sale de la sección del update (parcheado) o de
//! la RomFS de la NCA base. Los bloques parcheados usan **CTR-EX**: el contador
//! no solo avanza con el offset físico, también incorpora un `ctr_val` por
//! subsección.
//!
//! Portado de `hactool` (ISC, SciresM) `bktr.c` / `nca.c`. Se omiten a propósito
//! las validaciones de hactool que exigen que las tablas terminen exactamente en
//! el tamaño de sección: en los updates reales no siempre cuadran (ver Anexo B).
//!
//! ## Estructura en disco
//!
//! ```text
//! cabecera de relocalización (0x20)
//!   +0x00 offset u64   +0x08 size u64
//!   +0x10 "BKTR" u32   +0x14 version u32   +0x18 num_entries u32
//!
//! bloque de relocalización (size bytes)
//!   +0x00 reservado u32   +0x04 num_buckets u32   +0x08 total_size u64
//!   +0x10 bucket_virtual_offsets[num_buckets] u64
//!   buckets de 0x4000: u32 pad, u32 num, u64 virtual_offset_end,
//!                      entries[818] de { virt u64, phys u64, is_patch u32 }
//!
//! bloque de subsecciones (size bytes)
//!   +0x00 reservado u32   +0x04 num_buckets u32   +0x08 total_size u64
//!   +0x10 bucket_physical_offsets[num_buckets] u64
//!   buckets de 0x4000: u32 pad, u32 num, u64 physical_offset_end,
//!                      entries[1023] de { phys u64, rsv u32, ctr_val u32 }
//! ```

use crate::crypto;
use crate::error::{le32, le64};
use crate::ivfc::Ivfc;
use crate::section::SectionRead;
use crate::{Error, Result};

/// `"BKTR"` como u32 little-endian.
pub const BKTR_MAGIC: u32 = 0x5254_4B42;
const BUCKET_SIZE: usize = 0x4000;
/// Los `bucket_*_offsets` son un array **de tamaño fijo** de 0x3FF0 bytes, así
/// que los buckets empiezan siempre en 0x4000 (no detrás de `num_buckets`
/// entradas). hactool accede a `block->buckets` con esa misma premisa.
const BUCKETS_AT: usize = 0x4000;
const RELOC_BUCKET_HEADER: usize = 0x10;
const RELOC_ENTRY_LEN: usize = 0x14;
const RELOC_MAX_ENTRIES: usize = (BUCKET_SIZE - RELOC_BUCKET_HEADER) / RELOC_ENTRY_LEN; // 818
const SUBSEC_BUCKET_HEADER: usize = 0x10;
const SUBSEC_ENTRY_LEN: usize = 0x10;
const SUBSEC_MAX_ENTRIES: usize = (BUCKET_SIZE - SUBSEC_BUCKET_HEADER) / SUBSEC_ENTRY_LEN; // 1023
const MAX_BUCKETS: usize = 0x3FF0 / 8;
/// Las tablas BKTR reales ocupan unos pocos MB.
const MAX_TABLE: u64 = 256 * 1024 * 1024;

/// Cabecera de una de las dos tablas BKTR (`bktr_header_t`, 0x20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BktrHeader {
    pub offset: u64,
    pub size: u64,
    pub version: u32,
    pub num_entries: u32,
}

impl BktrHeader {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 0x20 {
            return Err(Error::Invalid(
                "cabecera BKTR truncada (se esperaban 0x20 bytes)".into(),
            ));
        }
        let header = Self {
            offset: le64(&data[0x00..]),
            size: le64(&data[0x08..]),
            version: le32(&data[0x14..]),
            num_entries: le32(&data[0x18..]),
        };
        if le32(&data[0x10..]) != BKTR_MAGIC {
            return Err(Error::Invalid(
                "se esperaba el magic \"BKTR\" en la cabecera de tabla".into(),
            ));
        }
        if header.size > MAX_TABLE {
            return Err(Error::Unsupported(format!(
                "tabla BKTR de {} bytes: demasiado grande",
                header.size
            )));
        }
        Ok(header)
    }
}

/// Entrada de la tabla de relocalización.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelocEntry {
    /// Offset **virtual** (el que ve el RomFS).
    pub virt: u64,
    /// Offset **físico** dentro de la sección que lo aporta.
    pub phys: u64,
    /// `true` si el bloque está en el update; `false` si sale de la base.
    pub is_patch: bool,
}

/// Entrada de la tabla de subsecciones: qué `ctr_val` usar para un rango físico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubsecEntry {
    pub phys: u64,
    pub ctr_val: u32,
}

/// Tablas ya normalizadas: entradas ordenadas y con centinelas.
#[derive(Debug, Clone)]
pub struct BktrTables {
    pub relocs: Vec<RelocEntry>,
    pub subsecs: Vec<SubsecEntry>,
    /// Tamaño virtual declarado por la tabla de relocalización.
    pub total_size: u64,
}

impl BktrTables {
    /// Parsea ambos bloques. `section_size` y `reloc_region` se usan para los
    /// centinelas de la tabla de subsecciones.
    pub fn parse(
        reloc: &[u8],
        subsec: &[u8],
        section_size: u64,
        section_ctr_low: u32,
        reloc_region: u64,
    ) -> Result<Self> {
        let (relocs, total_size) = parse_relocations(reloc)?;
        let mut subsecs = parse_subsections(subsec)?;

        // Centinelas: más allá de la última subsección real, hactool vuelve al
        // CTR normal de la sección y termina en el tamaño de sección.
        push_sentinel(&mut subsecs, reloc_region, section_ctr_low);
        push_sentinel(&mut subsecs, section_size, 0);

        Ok(Self {
            relocs,
            subsecs,
            total_size,
        })
    }

    /// Entrada de relocalización aplicable a `virt`, junto con el offset donde
    /// deja de ser válida.
    pub fn relocation_for(&self, virt: u64) -> Result<(&RelocEntry, u64)> {
        let idx = self.relocs.partition_point(|e| e.virt <= virt);
        if idx == 0 {
            return Err(Error::Invalid(format!(
                "offset virtual 0x{virt:X} no cubierto por la tabla BKTR"
            )));
        }
        let entry = &self.relocs[idx - 1];
        let end = self
            .relocs
            .get(idx)
            .map_or(self.total_size, |next| next.virt);
        Ok((entry, end))
    }

    /// Subsección aplicable a `phys`, junto con el offset donde termina.
    pub fn subsection_for(&self, phys: u64) -> Result<(&SubsecEntry, u64)> {
        let idx = self.subsecs.partition_point(|e| e.phys <= phys);
        if idx == 0 {
            return Err(Error::Invalid(format!(
                "offset físico 0x{phys:X} no cubierto por las subsecciones BKTR"
            )));
        }
        let entry = &self.subsecs[idx - 1];
        let end = self.subsecs.get(idx).map_or(u64::MAX, |next| next.phys);
        Ok((entry, end))
    }
}

fn parse_relocations(data: &[u8]) -> Result<(Vec<RelocEntry>, u64)> {
    let (num_buckets, total_size, buckets_at) = parse_block_header(data, "relocalización")?;
    let mut out = Vec::new();

    for b in 0..num_buckets {
        let bucket = bucket_slice(data, buckets_at, b, "relocalización")?;
        let count = le32(&bucket[0x04..]) as usize;
        if count > RELOC_MAX_ENTRIES {
            return Err(Error::Invalid(format!(
                "bucket BKTR {b} declara {count} entradas (máximo {RELOC_MAX_ENTRIES})"
            )));
        }
        for i in 0..count {
            let at = RELOC_BUCKET_HEADER + i * RELOC_ENTRY_LEN;
            let is_patch = le32(&bucket[at + 0x10..]);
            if is_patch > 1 {
                return Err(Error::Invalid(format!(
                    "entrada BKTR con is_patch={is_patch} (solo 0 o 1)"
                )));
            }
            out.push(RelocEntry {
                virt: le64(&bucket[at..]),
                phys: le64(&bucket[at + 0x08..]),
                is_patch: is_patch == 1,
            });
        }
    }

    out.sort_by_key(|e| e.virt);
    if out.is_empty() {
        return Err(Error::Invalid(
            "la tabla BKTR no tiene relocalizaciones".into(),
        ));
    }
    if out.iter().any(|e| e.virt >= total_size) {
        return Err(Error::Invalid(
            "la tabla BKTR apunta fuera de su tamaño virtual".into(),
        ));
    }
    // Centinela final: garantiza que `relocation_for` siempre encuentra entrada.
    out.push(RelocEntry {
        virt: total_size,
        phys: 0,
        is_patch: false,
    });

    Ok((out, total_size))
}

fn parse_subsections(data: &[u8]) -> Result<Vec<SubsecEntry>> {
    let (num_buckets, _total_size, buckets_at) = parse_block_header(data, "subsecciones")?;
    let mut out = Vec::new();

    for b in 0..num_buckets {
        let bucket = bucket_slice(data, buckets_at, b, "subsecciones")?;
        let count = le32(&bucket[0x04..]) as usize;
        if count > SUBSEC_MAX_ENTRIES {
            return Err(Error::Invalid(format!(
                "bucket BKTR {b} declara {count} subsecciones (máximo {SUBSEC_MAX_ENTRIES})"
            )));
        }
        for i in 0..count {
            let at = SUBSEC_BUCKET_HEADER + i * SUBSEC_ENTRY_LEN;
            out.push(SubsecEntry {
                phys: le64(&bucket[at..]),
                ctr_val: le32(&bucket[at + 0x0C..]),
            });
        }
    }

    if out.is_empty() {
        return Err(Error::Invalid(
            "la tabla de subsecciones BKTR está vacía".into(),
        ));
    }
    out.sort_by_key(|e| e.phys);
    Ok(out)
}

fn parse_block_header(data: &[u8], label: &str) -> Result<(usize, u64, usize)> {
    if data.len() < 0x10 {
        return Err(Error::Invalid(format!("bloque de {label} BKTR truncado")));
    }
    let num_buckets = le32(&data[0x04..]) as usize;
    if num_buckets == 0 || num_buckets > MAX_BUCKETS {
        return Err(Error::Invalid(format!(
            "bloque de {label} BKTR con {num_buckets} buckets"
        )));
    }
    let total_size = le64(&data[0x08..]);
    let buckets_at = BUCKETS_AT;
    let needed = buckets_at
        .checked_add(num_buckets * BUCKET_SIZE)
        .ok_or_else(|| Error::Invalid(format!("tamaño del bloque de {label} desbordado")))?;
    if needed > data.len() {
        return Err(Error::Invalid(format!(
            "bloque de {label} truncado: se necesitan 0x{needed:X} bytes y hay 0x{:X}",
            data.len()
        )));
    }
    Ok((num_buckets, total_size, buckets_at))
}

fn bucket_slice<'a>(
    data: &'a [u8],
    buckets_at: usize,
    index: usize,
    label: &str,
) -> Result<&'a [u8]> {
    let at = buckets_at + index * BUCKET_SIZE;
    data.get(at..at + BUCKET_SIZE)
        .ok_or_else(|| Error::Invalid(format!("bucket {index} de {label} BKTR fuera de la tabla")))
}

fn push_sentinel(entries: &mut Vec<SubsecEntry>, phys: u64, ctr_val: u32) {
    if entries.last().is_none_or(|last| last.phys < phys) {
        entries.push(SubsecEntry { phys, ctr_val });
    }
}

/// Sección virtual: fusiona la RomFS del update con la RomFS base.
///
/// `patch` debe ser la sección BKTR del update y `base` la sección RomFS de la
/// NCA base. Ambos se leen por separado; no se materializa ninguna NCA.
pub struct BktrSection<P: SectionRead, B: SectionRead> {
    patch: P,
    base: B,
    hash_info: [u8; 0xF8],
    patch_info: [u8; 0x40],
    ctr_iv: [u8; 16],
    patch_key: [u8; 16],
    virtual_size: u64,
    ctr_low: u32,
    tables: BktrTables,
}

impl<P: SectionRead, B: SectionRead> BktrSection<P, B> {
    pub fn new(mut patch: P, base: B) -> Result<Self> {
        let reloc_header = BktrHeader::parse(&patch.patch_info()[0x00..0x20])?;
        let subsec_header = BktrHeader::parse(&patch.patch_info()[0x20..0x40])?;

        let reloc_end = reloc_header
            .offset
            .checked_add(reloc_header.size)
            .ok_or_else(|| Error::Invalid("rango de la tabla BKTR desbordado".into()))?;
        let subsec_end = subsec_header
            .offset
            .checked_add(subsec_header.size)
            .ok_or_else(|| Error::Invalid("rango de la tabla BKTR desbordado".into()))?;
        if reloc_end > patch.size() || subsec_end > patch.size() {
            return Err(Error::Invalid(
                "las tablas BKTR caen fuera de la sección del update".into(),
            ));
        }

        let mut reloc_bytes = vec![0u8; reloc_header.size as usize];
        patch.read_at(reloc_header.offset, &mut reloc_bytes)?;
        let mut subsec_bytes = vec![0u8; subsec_header.size as usize];
        patch.read_at(subsec_header.offset, &mut subsec_bytes)?;

        let tables = BktrTables::parse(
            &reloc_bytes,
            &subsec_bytes,
            patch.size(),
            patch.section_ctr_low(),
            reloc_header.offset,
        )?;

        let ivfc = Ivfc::parse(patch.hash_info())?;
        let romfs_end = ivfc
            .fs_offset()
            .checked_add(ivfc.fs_size())
            .ok_or_else(|| Error::Invalid("rango virtual del RomFS BKTR desbordado".into()))?;
        // La tabla debería cubrir todo el espacio virtual; si no, se toma el
        // mayor de los dos para que las comprobaciones de RomFS no fallen antes
        // de leer.
        let virtual_size = tables.total_size.max(romfs_end);

        Ok(Self {
            hash_info: *patch.hash_info(),
            patch_info: [0u8; 0x40],
            ctr_iv: patch.ctr_iv(),
            patch_key: *patch.content_key(),
            ctr_low: patch.section_ctr_low(),
            patch,
            base,
            virtual_size,
            tables,
        })
    }

    pub fn tables(&self) -> &BktrTables {
        &self.tables
    }

    fn read_patch(&mut self, phys: u64, out: &mut [u8]) -> Result<()> {
        let mut done = 0usize;
        while done < out.len() {
            let p = phys + done as u64;
            let (subsec, end) = self.tables.subsection_for(p)?;
            let n = (out.len() - done).min((end.saturating_sub(p)) as usize);
            if n == 0 {
                return Err(Error::Invalid(format!(
                    "subsección BKTR sin avance en 0x{p:X}"
                )));
            }

            // CTR-EX: IV = primeros 4 bytes del section_ctr || ctr_val ||
            // BE64((inicio de sección + offset físico alineado) >> 4).
            let aligned = p & !0xF;
            let mut iv = self.ctr_iv;
            iv[4..8].copy_from_slice(&subsec.ctr_val.to_be_bytes());
            iv[8..16].copy_from_slice(&((self.patch.start() + aligned) >> 4).to_be_bytes());
            let skip = p - aligned;
            self.patch.read_raw_at(p, &mut out[done..done + n])?;
            crypto::ctr_apply_at(&self.patch_key, &iv, skip, &mut out[done..done + n]);
            done += n;
        }
        Ok(())
    }
}

impl<P: SectionRead, B: SectionRead> SectionRead for BktrSection<P, B> {
    fn index(&self) -> usize {
        self.patch.index()
    }

    fn hash_info(&self) -> &[u8; 0xF8] {
        &self.hash_info
    }

    fn patch_info(&self) -> &[u8; 0x40] {
        &self.patch_info
    }

    fn size(&self) -> u64 {
        self.virtual_size
    }

    fn start(&self) -> u64 {
        self.patch.start()
    }

    fn ctr_iv(&self) -> [u8; 16] {
        self.ctr_iv
    }

    fn section_ctr_low(&self) -> u32 {
        self.ctr_low
    }

    fn content_key(&self) -> &[u8; 16] {
        &self.patch_key
    }

    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> Result<()> {
        let end = offset
            .checked_add(out.len() as u64)
            .ok_or_else(|| Error::Invalid("lectura BKTR desbordada".into()))?;
        if end > self.virtual_size {
            return Err(Error::Invalid(format!(
                "lectura BKTR 0x{offset:X}..0x{end:X} fuera del espacio virtual (0x{:X})",
                self.virtual_size
            )));
        }
        let mut done = 0usize;
        while done < out.len() {
            let virt = offset + done as u64;
            let (reloc, reloc_end) = self.tables.relocation_for(virt)?;
            let n = (out.len() - done).min((reloc_end.saturating_sub(virt)) as usize);
            if n == 0 {
                return Err(Error::Invalid(format!(
                    "relocalización BKTR sin avance en 0x{virt:X}"
                )));
            }

            let phys = reloc
                .phys
                .checked_add(virt - reloc.virt)
                .ok_or_else(|| Error::Invalid("offset físico BKTR desbordado".into()))?;
            if reloc.is_patch {
                if phys + n as u64 > self.patch.size() {
                    return Err(Error::Invalid(format!(
                        "bloque BKTR 0x{phys:X}..0x{:X} fuera de la sección del update",
                        phys + n as u64
                    )));
                }
                self.read_patch(phys, &mut out[done..done + n])?;
            } else {
                self.base.read_at(phys, &mut out[done..done + n])?;
            }
            done += n;
        }
        Ok(())
    }

    fn read_raw_at(&mut self, offset: u64, out: &mut [u8]) -> Result<()> {
        // No tiene sentido en una sección virtual: los bloques parcheados se
        // leen en crudo desde el update dentro de `read_patch`.
        self.patch.read_raw_at(offset, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reloc_block(total: u64, entries: &[(u64, u64, u32)]) -> Vec<u8> {
        let mut data = vec![0u8; BUCKETS_AT + BUCKET_SIZE];
        data[0x04..0x08].copy_from_slice(&1u32.to_le_bytes());
        data[0x08..0x10].copy_from_slice(&total.to_le_bytes());
        data[0x10..0x18].copy_from_slice(&0u64.to_le_bytes()); // bucket 0 empieza en 0
        let bucket = BUCKETS_AT;
        data[bucket + 0x04..bucket + 0x08].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        data[bucket + 0x08..bucket + 0x10].copy_from_slice(&total.to_le_bytes());
        for (i, (virt, phys, patch)) in entries.iter().enumerate() {
            let at = bucket + 0x10 + i * RELOC_ENTRY_LEN;
            data[at..at + 8].copy_from_slice(&virt.to_le_bytes());
            data[at + 8..at + 16].copy_from_slice(&phys.to_le_bytes());
            data[at + 16..at + 20].copy_from_slice(&patch.to_le_bytes());
        }
        data
    }

    fn subsec_block(entries: &[(u64, u32)]) -> Vec<u8> {
        let mut data = vec![0u8; BUCKETS_AT + BUCKET_SIZE];
        data[0x04..0x08].copy_from_slice(&1u32.to_le_bytes());
        data[0x08..0x10].copy_from_slice(&0x8000u64.to_le_bytes());
        data[0x10..0x18].copy_from_slice(&0u64.to_le_bytes());
        let bucket = BUCKETS_AT;
        data[bucket + 0x04..bucket + 0x08].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        for (i, (phys, ctr)) in entries.iter().enumerate() {
            let at = bucket + 0x10 + i * SUBSEC_ENTRY_LEN;
            data[at..at + 8].copy_from_slice(&phys.to_le_bytes());
            data[at + 12..at + 16].copy_from_slice(&ctr.to_le_bytes());
        }
        data
    }

    #[test]
    fn parses_and_sorts_tables() {
        let reloc = reloc_block(0x1_0000, &[(0, 0, 0), (0x8000, 0x4000, 1)]);
        let subsec = subsec_block(&[(0x4000, 7), (0x8000, 8)]);
        let tables = BktrTables::parse(&reloc, &subsec, 0x2_0000, 0x1234, 0x1_8000).unwrap();

        assert_eq!(tables.total_size, 0x1_0000);
        assert_eq!(tables.relocs.len(), 3); // dos reales + centinela
        assert_eq!(
            tables.relocs[0],
            RelocEntry {
                virt: 0,
                phys: 0,
                is_patch: false
            }
        );
        assert_eq!(
            tables.relocs[1],
            RelocEntry {
                virt: 0x8000,
                phys: 0x4000,
                is_patch: true
            }
        );
        assert_eq!(tables.relocs[2].virt, 0x1_0000);

        let (entry, end) = tables.relocation_for(0x9000).unwrap();
        assert_eq!(entry.phys, 0x4000);
        assert_eq!(end, 0x1_0000);
    }

    #[test]
    fn subsection_sentinels_cover_the_section() {
        let reloc = reloc_block(0x1000, &[(0, 0, 1)]);
        let subsec = subsec_block(&[(0x100, 5)]);
        let tables = BktrTables::parse(&reloc, &subsec, 0x10000, 0xAA, 0x9000).unwrap();

        let (entry, end) = tables.subsection_for(0x100).unwrap();
        assert_eq!((entry.ctr_val, end), (5, 0x9000));
        let (entry, end) = tables.subsection_for(0x9000).unwrap();
        assert_eq!((entry.ctr_val, end), (0xAA, 0x10000));
        let (entry, end) = tables.subsection_for(0x10000).unwrap();
        assert_eq!((entry.ctr_val, end), (0, u64::MAX));
    }

    #[test]
    fn rejects_bad_magic_and_truncated_tables() {
        let mut header = [0u8; 0x20];
        header[0x10..0x14].copy_from_slice(b"XXXX");
        assert_eq!(
            BktrHeader::parse(&header).unwrap_err().code(),
            "E_LAYOUT_UNEXPECTED"
        );

        let reloc = reloc_block(0x1000, &[(0, 0, 0)]);
        assert!(BktrTables::parse(&reloc[..0x20], &subsec_block(&[(0, 1)]), 0x1000, 0, 0).is_err());
    }

    #[test]
    fn rejects_out_of_range_relocations() {
        let reloc = reloc_block(0x1000, &[(0x2000, 0, 0)]);
        let subsec = subsec_block(&[(0, 1)]);
        assert!(BktrTables::parse(&reloc, &subsec, 0x4000, 0, 0).is_err());
    }

    /// Sección en memoria con cabecera IVFC y `patch_info` inyectados: sirve
    /// tanto de base como de update.
    struct MockSection {
        data: Vec<u8>,
        info: [u8; 0xF8],
        patch: [u8; 0x40],
        key: [u8; 16],
        ctr_iv: [u8; 16],
        ctr_low: u32,
    }

    impl SectionRead for MockSection {
        fn index(&self) -> usize {
            1
        }
        fn hash_info(&self) -> &[u8; 0xF8] {
            &self.info
        }
        fn patch_info(&self) -> &[u8; 0x40] {
            &self.patch
        }
        fn size(&self) -> u64 {
            self.data.len() as u64
        }
        fn start(&self) -> u64 {
            0
        }
        fn ctr_iv(&self) -> [u8; 16] {
            self.ctr_iv
        }
        fn section_ctr_low(&self) -> u32 {
            self.ctr_low
        }
        fn content_key(&self) -> &[u8; 16] {
            &self.key
        }
        fn read_at(&mut self, offset: u64, out: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            if at + out.len() > self.data.len() {
                return Err(Error::Invalid("lectura fuera del mock".into()));
            }
            out.copy_from_slice(&self.data[at..at + out.len()]);
            Ok(())
        }
        fn read_raw_at(&mut self, offset: u64, out: &mut [u8]) -> Result<()> {
            self.read_at(offset, out)
        }
    }

    fn ivfc_header(fs_size: u64) -> [u8; 0xF8] {
        let mut info = [0u8; 0xF8];
        info[0..4].copy_from_slice(b"IVFC");
        info[4..8].copy_from_slice(&0x2_0000u32.to_le_bytes());
        info[8..12].copy_from_slice(&0x20u32.to_le_bytes());
        info[12..16].copy_from_slice(&7u32.to_le_bytes());
        for i in 0..6 {
            let at = 0x10 + i * 0x18;
            info[at + 0x10..at + 0x14].copy_from_slice(&12u32.to_le_bytes());
        }
        let level5 = 0x10 + 5 * 0x18;
        info[level5..level5 + 8].copy_from_slice(&0u64.to_le_bytes()); // fs_offset
        info[level5 + 8..level5 + 16].copy_from_slice(&fs_size.to_le_bytes());
        info
    }

    fn patch_info(reloc: (u64, u64), subsec: (u64, u64)) -> [u8; 0x40] {
        let mut p = [0u8; 0x40];
        p[0x00..0x08].copy_from_slice(&reloc.0.to_le_bytes());
        p[0x08..0x10].copy_from_slice(&reloc.1.to_le_bytes());
        p[0x10..0x14].copy_from_slice(b"BKTR");
        p[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        p[0x18..0x1C].copy_from_slice(&1u32.to_le_bytes());
        p[0x20..0x28].copy_from_slice(&subsec.0.to_le_bytes());
        p[0x28..0x30].copy_from_slice(&subsec.1.to_le_bytes());
        p[0x30..0x34].copy_from_slice(b"BKTR");
        p[0x34..0x38].copy_from_slice(&1u32.to_le_bytes());
        p[0x38..0x3C].copy_from_slice(&1u32.to_le_bytes());
        p
    }

    #[test]
    fn merges_patched_blocks_with_ctr_ex() {
        const RELOC_AT: u64 = 0x1_0000;
        const SUBSEC_AT: u64 = 0x1_8000;
        const CTR_VAL: u32 = 5;

        let key = [0x42u8; 16];
        let mut ctr_iv = [0u8; 16];
        ctr_iv[0..4].copy_from_slice(&2u32.to_be_bytes());
        let ctr_low = 2;

        let base = MockSection {
            data: vec![0x11; 0x1_0000],
            info: [0u8; 0xF8],
            patch: [0u8; 0x40],
            key,
            ctr_iv,
            ctr_low,
        };

        // 0x200 bytes de parche, cifrados con CTR-EX tal como estarían en disco.
        let mut cipher = vec![0xAAu8; 0x200];
        let mut iv = ctr_iv;
        iv[4..8].copy_from_slice(&CTR_VAL.to_be_bytes());
        iv[8..16].copy_from_slice(&0u64.to_be_bytes());
        crypto::ctr_apply(&key, &iv, &mut cipher);

        let reloc = reloc_block(0x1_0000, &[(0, 0, 0), (0x8000, 0, 1)]);
        let subsec = subsec_block(&[(0, CTR_VAL)]);
        let mut data = vec![0u8; 0x2_0000];
        data[..0x200].copy_from_slice(&cipher);
        data[RELOC_AT as usize..RELOC_AT as usize + reloc.len()].copy_from_slice(&reloc);
        data[SUBSEC_AT as usize..SUBSEC_AT as usize + subsec.len()].copy_from_slice(&subsec);

        let patch = MockSection {
            data,
            info: ivfc_header(0x1_0000),
            patch: patch_info(
                (RELOC_AT, reloc.len() as u64),
                (SUBSEC_AT, subsec.len() as u64),
            ),
            key,
            ctr_iv,
            ctr_low,
        };

        let mut merged = BktrSection::new(patch, base).unwrap();
        assert_eq!(merged.size(), 0x1_0000);

        let mut buf = [0u8; 8];
        merged.read_at(0x100, &mut buf).unwrap();
        assert_eq!(buf, [0x11; 8], "los bloques no parcheados salen de la base");

        let mut buf = [0u8; 0x20];
        merged.read_at(0x8000, &mut buf).unwrap();
        assert_eq!(
            buf, [0xAA; 0x20],
            "los bloques parcheados se descifran con CTR-EX"
        );

        // Tramo no alineado a 0x10: ejercita el `skip` del keystream.
        let mut buf = [0u8; 0x10];
        merged.read_at(0x8003, &mut buf).unwrap();
        assert_eq!(buf, [0xAA; 0x10]);

        assert!(merged.read_at(0x1_0000, &mut [0u8; 4]).is_err());
    }
}
