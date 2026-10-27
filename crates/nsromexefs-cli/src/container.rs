//! Apertura de contenedores: NSP (PFS0) y XCI (HFS0).
//!
//! Los dos formatos terminan en la misma abstracción: una lista de entradas
//! con offsets **absolutos** dentro del archivo abierto. A partir de ahí todo
//! el motor (cabeceras NCA, tickets, CNMT, RomFS) funciona igual.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use nsromexefs_core::pfs0::{classify, EntryKind};
use nsromexefs_core::{
    content_key, read_meta, BktrSection, Cnmt, ContentInfo, EncryptionType, Error, Hfs0, KeyOrigin,
    KeySet, MetaInfo, MetaType, NcaHeader, PartitionType, Pfs0, PlainSection, Section, SectionRead,
    Ticket, Xci,
};

/// Entrada ya resuelta a offsets absolutos del archivo.
#[derive(Debug, Clone)]
pub(crate) struct TableEntry {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    pub kind: EntryKind,
}

/// Entradas de un contenedor: el PFS0 de un NSP o el HFS0 de `secure`.
#[derive(Debug, Clone)]
pub(crate) struct EntryTable {
    pub entries: Vec<TableEntry>,
}

impl EntryTable {
    fn from_pfs0(pfs0: &Pfs0) -> Self {
        Self {
            entries: pfs0
                .entries
                .iter()
                .map(|e| TableEntry {
                    name: e.name.clone(),
                    offset: e.offset,
                    size: e.size,
                    kind: e.kind(),
                })
                .collect(),
        }
    }

    pub(crate) fn from_hfs0(hfs0: &Hfs0) -> Self {
        Self {
            entries: hfs0
                .entries
                .iter()
                .map(|e| TableEntry {
                    name: e.name.clone(),
                    offset: e.offset,
                    size: e.size,
                    kind: classify(&e.name),
                })
                .collect(),
        }
    }

    pub fn meta_entries(&self) -> impl Iterator<Item = &TableEntry> {
        self.entries.iter().filter(|e| e.kind == EntryKind::Meta)
    }

    /// Busca `<id>.nca` por el id de contenido del CNMT.
    pub fn find_nca(&self, id_hex: &str) -> Option<&TableEntry> {
        let name = format!("{id_hex}.nca");
        self.entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(&name))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerKind {
    Nsp,
    Xci,
}

impl ContainerKind {
    pub fn label(self) -> &'static str {
        match self {
            ContainerKind::Nsp => "NSP (PFS0)",
            ContainerKind::Xci => "XCI (HFS0)",
        }
    }
}

pub(crate) struct OpenedTable {
    pub kind: ContainerKind,
    pub table: EntryTable,
    pub xci: Option<Xci>,
}

/// Contenedor abierto y resuelto: lo necesario para planificar y extraer sin
/// volver a leer cabeceras ni tickets.
pub(crate) struct Container {
    pub file: File,
    pub meta_info: MetaInfo,
    pub content_key: [u8; 16],
    pub key_origin: KeyOrigin,
    pub program_name: String,
    pub exefs: Option<Section>,
    pub romfs: Option<Section>,
    pub display_version: Option<String>,
}

impl Container {
    pub fn title_id(&self) -> String {
        self.meta_info.cnmt.title_id_hex()
    }
}

/// Información mínima para agrupar títulos en modo lote (no toca la NCA de
/// programa ni necesita su titlekey).
#[derive(Debug, Clone)]
pub(crate) struct ProbeInfo {
    pub meta_type: MetaType,
    pub title_id: String,
    /// Id de la aplicación a la que pertenece (para DLC y updates).
    pub app_id: String,
    pub version: u32,
}

/// Detecta el tipo de contenedor y extrae su tabla de entradas principal.
pub(crate) fn open_table(file: &mut File, path: &Path) -> Result<OpenedTable, Error> {
    let total_size = file.metadata()?.len();
    if total_size < 4 {
        return Err(Error::Unsupported(format!(
            "{}: archivo demasiado pequeño para ser un contenedor",
            path.display()
        )));
    }

    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;

    if &magic == b"PFS0" {
        file.seek(SeekFrom::Start(0))?;
        let pfs0 = Pfs0::read(file, total_size)?;
        return Ok(OpenedTable {
            kind: ContainerKind::Nsp,
            table: EntryTable::from_pfs0(&pfs0),
            xci: None,
        });
    }

    if !looks_like_xci(file)? {
        return Err(Error::Unsupported(format!(
            "{}: no es un NSP ni un XCI reconocible",
            path.display()
        )));
    }

    let xci = Xci::read(file, total_size)?;
    let secure = xci.secure().ok_or_else(|| {
        Error::Invalid(format!(
            "{}: el XCI no tiene una partición `secure` legible",
            path.display()
        ))
    })?;

    Ok(OpenedTable {
        kind: ContainerKind::Xci,
        table: EntryTable::from_hfs0(&secure.hfs0),
        xci: Some(xci),
    })
}

/// Abre un archivo y devuelve **todos** los títulos que contiene: un NSP puede
/// traer base + DLC, y un XCI puede traer base + update dentro de `secure` o en
/// su partición `update`.
pub(crate) fn open_containers(
    path: &Path,
    keys: &KeySet,
    target: Option<&str>,
) -> Result<Vec<Container>, Error> {
    let mut file = File::open(path)?;
    let opened = open_table(&mut file, path)?;
    let OpenedTable { table, xci, .. } = opened;

    if table.meta_entries().next().is_none() {
        return Err(no_meta_error(path));
    }

    let target_raw = target.and_then(|id| u64::from_str_radix(id, 16).ok());
    let mut containers = Vec::new();
    for meta in table.meta_entries() {
        let mut file = File::open(path)?;
        let (meta_info, tickets) = read_meta_info(&mut file, meta, &table, keys)?;
        if !wanted_title(&meta_info.cnmt, target, target_raw) {
            // No se resuelve la NCA de programa de un título que no se va a
            // extraer: así no se exige su titlekey ni se pierde tiempo.
            continue;
        }
        containers.push(resolve_container(
            file, meta_info, tickets, &table, keys, path,
        )?);
    }

    if let Some(xci) = xci {
        if let Some(update) = xci.partition("update") {
            let update_table = EntryTable::from_hfs0(&update.hfs0);
            // Una partición update vacía o sin CNMT no aporta nada.
            for meta in update_table.meta_entries() {
                let mut file = File::open(path)?;
                let (meta_info, tickets) = read_meta_info(&mut file, meta, &update_table, keys)?;
                if !wanted_title(&meta_info.cnmt, target, target_raw) {
                    continue;
                }
                containers.push(resolve_container(
                    file,
                    meta_info,
                    tickets,
                    &update_table,
                    keys,
                    path,
                )?);
            }
        }
    }

    Ok(containers)
}

/// El título pedido y sus parches. Un `--tree` con base + DLC abre solo el
/// título que toca en cada pasada; el update (Patch) acompaña siempre a su base.
fn wanted_title(cnmt: &Cnmt, target: Option<&str>, target_raw: Option<u64>) -> bool {
    let Some(target) = target else {
        return true;
    };
    if cnmt.title_id_hex().eq_ignore_ascii_case(target) {
        return true;
    }
    let Some(target_raw) = target_raw else {
        return false;
    };
    cnmt.meta_type == MetaType::Patch
        && (cnmt.application_id() == Some(target_raw)
            || cnmt.title_id == target_raw.wrapping_add(0x800))
}

/// Lee solo el CNMT de cada título presente en el archivo (base, update, DLC y
/// la partición `update` del XCI). Sirve para agrupar en modo lote sin gastar
/// tiempo en claves de contenido que aún no hacen falta.
pub(crate) fn probe_titles(path: &Path, keys: &KeySet) -> Result<Vec<ProbeInfo>, Error> {
    let mut file = File::open(path)?;
    let opened = open_table(&mut file, path)?;
    let OpenedTable { table, xci, .. } = opened;

    if table.meta_entries().next().is_none() {
        return Err(no_meta_error(path));
    }

    let mut probes = Vec::new();
    probe_table(&mut file, &table, keys, &mut probes)?;

    if let Some(xci) = xci {
        if let Some(update) = xci.partition("update") {
            let table = EntryTable::from_hfs0(&update.hfs0);
            if table.meta_entries().next().is_some() {
                probe_table(&mut file, &table, keys, &mut probes)?;
            }
        }
    }

    Ok(probes)
}

/// Lee todos los CNMT de una tabla. Un mismo contenedor puede llevar varios
/// títulos (base + update en un XCI, o base + DLC en un NSP).
fn probe_table(
    file: &mut File,
    table: &EntryTable,
    keys: &KeySet,
    probes: &mut Vec<ProbeInfo>,
) -> Result<(), Error> {
    for meta in table.meta_entries() {
        let (meta_info, _) = read_meta_info(file, meta, table, keys)?;
        probes.push(ProbeInfo::from_cnmt(&meta_info));
    }
    Ok(())
}

fn no_meta_error(path: &Path) -> Error {
    Error::Invalid(format!(
        "{}: no hay ningún .cnmt.nca: no se puede identificar el título",
        path.display()
    ))
}

impl ProbeInfo {
    fn from_cnmt(meta_info: &MetaInfo) -> Self {
        let cnmt = &meta_info.cnmt;
        let title_id = cnmt.title_id_hex();
        let app_id = match cnmt.meta_type {
            MetaType::Application => title_id.clone(),
            _ => cnmt
                .application_id()
                .map(|id| format!("{id:016X}"))
                .unwrap_or_else(|| title_id.clone()),
        };
        Self {
            meta_type: cnmt.meta_type,
            title_id,
            app_id,
            version: cnmt.version,
        }
    }
}

/// Resuelve CNMT, NCA de programa, clave de contenido y secciones de
/// `exefs`/`romfs` sobre una tabla de entradas ya abierta.
fn resolve_container(
    mut file: File,
    meta_info: MetaInfo,
    tickets: TicketList,
    table: &EntryTable,
    keys: &KeySet,
    path: &Path,
) -> Result<Container, Error> {
    let program = meta_info
        .cnmt
        .contents
        .iter()
        .find(|c| c.content_type == 1)
        .ok_or_else(|| {
            Error::Invalid(format!(
                "{}: el CNMT no lista ninguna NCA de programa",
                path.display()
            ))
        })?;
    let id_hex = program.content_id_hex();
    let nca_entry = table.find_nca(&id_hex).ok_or_else(|| {
        Error::Invalid(format!(
            "{}: la NCA de programa {id_hex} no está en el contenedor",
            path.display()
        ))
    })?;

    let header = NcaHeader::read_at(
        &mut file,
        nca_entry.offset,
        nca_entry.size,
        keys.header_key().as_ref(),
    )?;
    let program_ticket = tickets
        .list
        .iter()
        .find(|t| t.rights_id == header.rights_id);
    let (content_key, key_origin) = content_key(&header, keys, program_ticket)?;

    let exefs = header
        .sections
        .iter()
        .flatten()
        .find(|s| s.partition_type == PartitionType::Pfs0)
        .cloned();
    let romfs = header
        .sections
        .iter()
        .flatten()
        .find(|s| s.partition_type == PartitionType::RomFs)
        .cloned();
    let display_version = read_display_version(
        &mut file,
        table,
        &meta_info.cnmt.contents,
        keys,
        &tickets.list,
    );

    Ok(Container {
        file,
        meta_info,
        content_key,
        key_origin,
        program_name: nca_entry.name.clone(),
        exefs,
        romfs,
        display_version,
    })
}

/// Lee la NCA de metadatos y su CNMT, junto con los tickets del contenedor.
fn read_meta_info(
    file: &mut File,
    meta_entry: &TableEntry,
    table: &EntryTable,
    keys: &KeySet,
) -> Result<(MetaInfo, TicketList), Error> {
    let tickets = read_tickets(file, table);
    let meta_header = NcaHeader::read_at(
        file,
        meta_entry.offset,
        meta_entry.size,
        keys.header_key().as_ref(),
    )?;
    let meta_ticket = tickets
        .list
        .iter()
        .find(|t| t.rights_id == meta_header.rights_id);
    let meta_info = read_meta(file, meta_entry.offset, meta_entry.size, keys, meta_ticket)?;

    Ok((meta_info, tickets))
}

pub(crate) struct TicketList {
    pub list: Vec<Ticket>,
    pub err: Option<String>,
}

/// Los tickets son pequeños y opcionales: en un cartucho no existen, así que
/// la clave debe venir de `title.keys`.
pub(crate) fn read_tickets(file: &mut File, table: &EntryTable) -> TicketList {
    let mut list = Vec::new();
    for entry in &table.entries {
        if entry.kind != EntryKind::Ticket || entry.size > 0x10000 {
            continue;
        }
        let mut data = vec![0u8; entry.size as usize];
        if file.seek(SeekFrom::Start(entry.offset)).is_err() || file.read_exact(&mut data).is_err()
        {
            continue;
        }
        match Ticket::parse(&data) {
            Ok(ticket) => list.push(ticket),
            Err(err) => {
                return TicketList {
                    list,
                    err: Some(format!("ticket {} ilegible: {err}", entry.name)),
                }
            }
        }
    }
    TicketList { list, err: None }
}

/// Lee la versión visible (`DisplayVersion`) de `control.nacp`.
///
/// La NCA de control es un RomFS normal; `control.nacp` está en su raíz y la
/// cadena vive en `0x3060` (16 bytes, terminada en NUL). Devuelve `None` si el
/// contenido no está en el contenedor, no hay clave o el campo no parece una
/// versión.
pub(crate) fn read_display_version(
    file: &mut File,
    table: &EntryTable,
    contents: &[ContentInfo],
    keys: &KeySet,
    tickets: &[Ticket],
) -> Option<String> {
    let control = contents.iter().find(|c| c.content_type == 3)?;
    let id_hex = control.content_id_hex();
    let entry = table.find_nca(&id_hex)?;

    let header =
        NcaHeader::read_at(file, entry.offset, entry.size, keys.header_key().as_ref()).ok()?;
    let ticket = tickets.iter().find(|t| t.rights_id == header.rights_id);
    let (content_key, _) = content_key(&header, keys, ticket).ok()?;

    let section = header
        .sections
        .iter()
        .flatten()
        .find(|s| s.partition_type == PartitionType::RomFs)?;
    if section.encryption == EncryptionType::Bktr {
        // El control de un update no debería ser BKTR; si lo fuera, haría falta
        // la base para fusionarlo y aquí solo tenemos este contenedor.
        return None;
    }

    let nacp = {
        let mut source = PlainSection::new(file, section, &content_key);
        nsromexefs_core::romfs_read_file(&mut source, "control.nacp").ok()??
    };
    parse_display_version(&nacp)
}

fn parse_display_version(nacp: &[u8]) -> Option<String> {
    const DISPLAY_VERSION_OFFSET: usize = 0x3060;
    let raw = nacp.get(DISPLAY_VERSION_OFFSET..DISPLAY_VERSION_OFFSET + 0x10)?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let text = std::str::from_utf8(&raw[..end]).ok()?.trim();
    if text.is_empty()
        || !text
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
    {
        return None;
    }
    Some(text.to_string())
}

/// RomFS sin fusionar (base, DLC o un update sin BKTR).
pub(crate) fn plain_source(container: &mut Container) -> Result<Box<dyn SectionRead + '_>, Error> {
    let Container {
        file,
        romfs,
        content_key,
        ..
    } = container;
    let section = romfs
        .as_ref()
        .ok_or_else(|| Error::Invalid("el contenedor no tiene RomFS".into()))?;
    Ok(Box::new(PlainSection::new(file, section, content_key)))
}

/// RomFS virtual del update fusionada con la RomFS de la base.
pub(crate) fn merged_source<'a>(
    base: &'a mut Container,
    update: &'a mut Container,
) -> Result<Box<dyn SectionRead + 'a>, Error> {
    let patch = {
        let Container {
            file,
            romfs,
            content_key,
            ..
        } = &mut *update;
        let section = romfs
            .as_ref()
            .ok_or_else(|| Error::Invalid("el update no tiene RomFS".into()))?;
        PlainSection::new(file, section, content_key)
    };
    let origin = {
        let Container {
            file,
            romfs,
            content_key,
            ..
        } = &mut *base;
        let section = romfs
            .as_ref()
            .ok_or_else(|| Error::Invalid("la base no tiene RomFS".into()))?;
        PlainSection::new(file, section, content_key)
    };
    Ok(Box::new(BktrSection::new(patch, origin)?))
}

pub(crate) fn looks_like_xci(file: &mut File) -> Result<bool, Error> {
    if file.metadata()?.len() < 0x104 {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(0x100))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    Ok(&magic == b"HEAD")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cnmt(title_id: u64, meta_type: MetaType, app: Option<u64>) -> Cnmt {
        let mut extended = Vec::new();
        if let Some(app) = app {
            extended.extend_from_slice(&app.to_le_bytes());
        }
        Cnmt {
            title_id,
            version: 0,
            meta_type,
            attributes: 0,
            required_download_version: 0,
            extended,
            contents: Vec::new(),
        }
    }

    const BASE: u64 = 0x0100_6BB0_0C6F_0000;
    const DLC: u64 = BASE + 0x1000;

    #[test]
    fn target_filter_opens_only_the_title_and_its_patches() {
        let target = format!("{BASE:016X}");
        assert!(wanted_title(
            &cnmt(BASE, MetaType::Application, None),
            Some(&target),
            Some(BASE)
        ));
        assert!(wanted_title(
            &cnmt(BASE + 0x800, MetaType::Patch, Some(BASE)),
            Some(&target),
            Some(BASE)
        ));

        // Un DLC ajeno no se abre al extraer la base.
        assert!(!wanted_title(
            &cnmt(DLC, MetaType::AddOnContent, Some(BASE)),
            Some(&target),
            Some(BASE)
        ));

        // El parche de un DLC puede identificarse por aplicación o por title id.
        let dlc_target = format!("{DLC:016X}");
        assert!(wanted_title(
            &cnmt(DLC, MetaType::AddOnContent, Some(BASE)),
            Some(&dlc_target),
            Some(DLC)
        ));
        assert!(wanted_title(
            &cnmt(DLC + 0x800, MetaType::Patch, Some(BASE)),
            Some(&dlc_target),
            Some(DLC)
        ));

        // Sin filtro se abre todo.
        assert!(wanted_title(
            &cnmt(DLC, MetaType::AddOnContent, Some(BASE)),
            None,
            None
        ));
    }
}
