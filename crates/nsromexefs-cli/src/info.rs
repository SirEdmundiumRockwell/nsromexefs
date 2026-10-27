//! Subcomando `info`: inspección de NSP/XCI sin extraer nada.

use std::fs::File;
use std::path::{Path, PathBuf};

use nsromexefs_core::{
    content_key, read_meta, EncryptionType, Error, KeySet, NcaHeader, PartitionType,
};

use crate::container::{open_table, read_display_version, read_tickets, ContainerKind, EntryTable};
use crate::util::{human_size, load_keys_lenient, short_id};

pub(crate) fn cmd_info(
    files: &[PathBuf],
    explicit_keys: &[PathBuf],
    json: bool,
) -> Result<(), Error> {
    let keys = load_keys_lenient(explicit_keys);

    let mut results = Vec::new();
    for path in files {
        results.push(inspect_container(path, keys.as_ref())?);
    }

    if json {
        let payload = serde_json::json!({
            "containers": results.iter().map(ContainerInfo::to_json).collect::<Vec<_>>(),
            "keys": {
                "sources": keys.as_ref().map(|k| k.sources().iter().map(|p| p.display().to_string()).collect::<Vec<_>>()).unwrap_or_default(),
                "max_revision": keys.as_ref().and_then(|k| k.max_revision()),
                "titlekeys": keys.as_ref().map(|k| k.titlekey_count()).unwrap_or(0),
                "warnings": keys.as_ref().map(|k| k.warnings().len()).unwrap_or(0),
            }
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return Ok(());
    }

    for info in &results {
        info.print_human();
        println!();
    }
    match &keys {
        Some(set) => {
            let rev = set
                .max_revision()
                .map(|r| format!("hasta revisión {r}"))
                .unwrap_or_else(|| "sin revisiones".to_string());
            println!(
                "claves: {} archivo(s) · {} claves · {} titlekeys · {rev}",
                set.sources().len(),
                set.key_count(),
                set.titlekey_count()
            );
            for warning in set.warnings().iter().take(5) {
                println!("  aviso: {warning}");
            }
        }
        None => println!("claves: no se encontró ningún archivo (usa -k prod.keys)"),
    }
    Ok(())
}

struct ContainerInfo {
    path: PathBuf,
    kind: &'static str,
    entries_label: &'static str,
    total_size: u64,
    partitions: Vec<PartitionLine>,
    entries: Vec<EntryLine>,
    titles: Vec<TitleInfo>,
    warnings: Vec<String>,
}

struct PartitionLine {
    name: String,
    size: u64,
    parsed: bool,
}

struct EntryLine {
    name: String,
    kind: &'static str,
    size: u64,
}

struct TitleInfo {
    title_id: String,
    version: String,
    version_raw: u32,
    /// Versión visible de `control.nacp` (`1.1.0`), si se pudo leer.
    display_version: Option<String>,
    meta_type: &'static str,
    application_id: Option<String>,
    crypto_type: u8,
    crypto_revision: u8,
    key_origin: &'static str,
    contents: Vec<ContentLine>,
}

struct ContentLine {
    name: String,
    size: u64,
    content_type: &'static str,
    nca_type: String,
    sections: Vec<String>,
    key: Option<&'static str>,
    error: Option<String>,
    /// La NCA está dentro del contenedor.
    present: bool,
}

impl ContainerInfo {
    fn print_human(&self) {
        println!("{}", self.path.display());
        println!(
            "  {} · {} entradas · {}",
            self.kind,
            self.entries.len(),
            human_size(self.total_size)
        );

        if !self.partitions.is_empty() {
            println!("  particiones ({})", self.partitions.len());
            for partition in &self.partitions {
                let state = if partition.parsed {
                    ""
                } else {
                    "  (vacía o no legible)"
                };
                println!(
                    "    {:<9} {:>10}{}",
                    partition.name,
                    human_size(partition.size),
                    state
                );
            }
        }

        for title in &self.titles {
            let suffix = title
                .application_id
                .as_ref()
                .map(|id| format!(" · base {id}"))
                .unwrap_or_default();
            let visible = title
                .display_version
                .as_deref()
                .map(|v| format!("v{v} · "))
                .unwrap_or_default();
            println!(
                "  título     {} · {}meta v{} · {}{}",
                title.title_id, visible, title.version_raw, title.meta_type, suffix
            );
            println!(
                "  claves     crypto_type {} → revisión {} · meta vía {}",
                title.crypto_type, title.crypto_revision, title.key_origin
            );
            println!("  contenidos del CNMT");
            for content in &title.contents {
                if !content.present {
                    // Los fragmentos delta viven en otro contenedor por diseño.
                    println!(
                        "    {:<10} {:>10}  {}  (fuera del contenedor)",
                        content.content_type,
                        human_size(content.size),
                        short_id(&content.name)
                    );
                    continue;
                }
                let key = match (&content.key, &content.error) {
                    (Some(origin), _) => format!("clave OK ({origin})"),
                    (None, Some(err)) => format!("clave: {err}"),
                    (None, None) => "clave: —".to_string(),
                };
                let sections = if content.sections.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", content.sections.join(", "))
                };
                println!(
                    "    {:<10} {:>10}  {}  [{}]{}",
                    content.content_type,
                    human_size(content.size),
                    short_id(&content.name),
                    key,
                    sections
                );
                println!("               {}", content.nca_type);
            }
        }

        for warning in &self.warnings {
            println!("  aviso      {warning}");
        }

        println!("  {} ({})", self.entries_label, self.entries.len());
        for entry in &self.entries {
            println!(
                "    {:<9} {:>10}  {}",
                entry.kind,
                human_size(entry.size),
                entry.name
            );
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "path": self.path.display().to_string(),
            "container": self.kind,
            "size": self.total_size,
            "partitions": self.partitions.iter().map(|p| serde_json::json!({
                "name": p.name,
                "size": p.size,
                "parsed": p.parsed,
            })).collect::<Vec<_>>(),
            "titles": self.titles.iter().map(|t| serde_json::json!({
                "title_id": t.title_id,
                "version": t.version,
                "version_raw": t.version_raw,
                "display_version": t.display_version,
                "meta_type": t.meta_type,
                "application_id": t.application_id,
                "crypto_type": t.crypto_type,
                "crypto_revision": t.crypto_revision,
                "key_origin": t.key_origin,
                "contents": t.contents.iter().map(|c| serde_json::json!({
                    "name": c.name,
                    "size": c.size,
                    "content_type": c.content_type,
                    "nca_type": c.nca_type,
                    "sections": c.sections,
                    "key_origin": c.key,
                    "error": c.error,
                    "present": c.present,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "entries": self.entries.iter().map(|e| serde_json::json!({
                "name": e.name,
                "kind": e.kind,
                "size": e.size,
            })).collect::<Vec<_>>(),
            "warnings": self.warnings,
        })
    }
}

fn inspect_container(path: &Path, keys: Option<&KeySet>) -> Result<ContainerInfo, Error> {
    let mut file = File::open(path)?;
    let total_size = file.metadata()?.len();
    let opened = open_table(&mut file, path)?;

    let entries_label = match opened.kind {
        ContainerKind::Nsp => "entradas del contenedor",
        ContainerKind::Xci => "entradas de la partición `secure`",
    };

    let mut info = ContainerInfo {
        path: path.to_path_buf(),
        kind: opened.kind.label(),
        entries_label,
        total_size,
        partitions: Vec::new(),
        entries: opened
            .table
            .entries
            .iter()
            .map(|e| EntryLine {
                name: e.name.clone(),
                kind: e.kind.label(),
                size: e.size,
            })
            .collect(),
        titles: Vec::new(),
        warnings: Vec::new(),
    };

    if let Some(xci) = &opened.xci {
        for entry in &xci.root.entries {
            info.partitions.push(PartitionLine {
                name: entry.name.clone(),
                size: entry.size,
                parsed: xci.partition(&entry.name).is_some(),
            });
        }
        for (name, err) in &xci.partition_errors {
            info.warnings
                .push(format!("partición `{name}` ilegible: {err}"));
        }
    }

    let Some(keys) = keys else {
        info.warnings.push(
            "sin claves: se muestran los archivos pero no el título (usa -k prod.keys)".into(),
        );
        return Ok(info);
    };

    if keys.header_key().is_none() {
        info.warnings
            .push("el keyset no incluye header_key: no se puede leer la cabecera NCA".into());
        return Ok(info);
    }

    inspect_table(&mut file, &opened.table, keys, &mut info);

    // En un XCI, la partición update es un NSP delta con su propio CNMT.
    if let Some(xci) = &opened.xci {
        for name in ["update", "normal", "logo"] {
            let Some(partition) = xci.partition(name) else {
                continue;
            };
            let table = EntryTable::from_hfs0(&partition.hfs0);
            if table.meta_entries().next().is_some() {
                inspect_table(&mut file, &table, keys, &mut info);
            }
        }
    }

    Ok(info)
}

fn inspect_table(file: &mut File, table: &EntryTable, keys: &KeySet, info: &mut ContainerInfo) {
    if table.meta_entries().next().is_none() {
        info.warnings
            .push("no hay ningún .cnmt.nca: no se puede identificar el título".into());
        return;
    }

    // Los tickets se leen una sola vez; son pequeños.
    let tickets = read_tickets(file, table);
    if let Some(err) = &tickets.err {
        info.warnings.push(err.clone());
    }

    for meta in table.meta_entries() {
        let header = NcaHeader::read_at(file, meta.offset, meta.size, keys.header_key().as_ref());
        let meta_rights = header
            .as_ref()
            .ok()
            .filter(|h| h.has_rights_id())
            .map(|h| h.rights_id);
        let ticket = meta_rights
            .as_ref()
            .and_then(|rid| tickets.list.iter().find(|t| &t.rights_id == rid));

        match read_meta(file, meta.offset, meta.size, keys, ticket) {
            Ok(meta_info) => {
                let mut contents = Vec::new();
                for content in &meta_info.cnmt.contents {
                    contents.push(inspect_content(
                        file,
                        table,
                        content.content_id,
                        content.size,
                        content.content_type,
                        keys,
                        &tickets.list,
                    ));
                }
                let display_version = read_display_version(
                    file,
                    table,
                    &meta_info.cnmt.contents,
                    keys,
                    &tickets.list,
                );
                info.titles.push(TitleInfo {
                    title_id: meta_info.cnmt.title_id_hex(),
                    version: meta_info.cnmt.version_string(),
                    version_raw: meta_info.cnmt.version,
                    display_version,
                    meta_type: meta_info.cnmt.meta_type.label(),
                    application_id: meta_info
                        .cnmt
                        .application_id()
                        .map(|id| format!("{id:016X}")),
                    crypto_type: meta_info
                        .header
                        .crypto_type
                        .max(meta_info.header.crypto_type2),
                    crypto_revision: meta_info.header.crypto_revision(),
                    key_origin: meta_info.key_origin.label(),
                    contents,
                });
            }
            Err(err) => info
                .warnings
                .push(format!("{}: {} [{}]", meta.name, err, err.code())),
        }
    }
}

fn inspect_content(
    file: &mut File,
    table: &EntryTable,
    content_id: [u8; 16],
    declared_size: u64,
    content_type: u8,
    keys: &KeySet,
    tickets: &[nsromexefs_core::Ticket],
) -> ContentLine {
    let id_hex: String = content_id.iter().map(|b| format!("{b:02x}")).collect();
    let Some(entry) = table.find_nca(&id_hex) else {
        return ContentLine {
            name: id_hex,
            size: declared_size,
            content_type: content_type_label(content_type),
            nca_type: String::new(),
            sections: Vec::new(),
            key: None,
            error: None,
            present: false,
        };
    };

    let header =
        match NcaHeader::read_at(file, entry.offset, entry.size, keys.header_key().as_ref()) {
            Ok(header) => header,
            Err(err) => {
                return ContentLine {
                    name: entry.name.clone(),
                    size: entry.size,
                    content_type: content_type_label(content_type),
                    nca_type: "cabecera ilegible".into(),
                    sections: Vec::new(),
                    key: None,
                    error: Some(format!("{} [{}]", err, err.code())),
                    present: true,
                }
            }
        };

    let ticket = tickets.iter().find(|t| t.rights_id == header.rights_id);
    let (key, error) = match content_key(&header, keys, ticket) {
        Ok((_, origin)) => (Some(origin.label()), None),
        Err(err) => (None, Some(format!("{} [{}]", err, err.code()))),
    };

    let mut sections = Vec::new();
    for section in header.sections.iter().flatten() {
        let magic_ok = match (section.partition_type, section.encryption) {
            (PartitionType::Pfs0, _) => section.pfs0_superblock().is_some_and(|sb| {
                sb.block_size.is_power_of_two() && sb.pfs0_offset + sb.pfs0_size <= section.size()
            }),
            (PartitionType::RomFs, EncryptionType::Bktr) => {
                // `bktr_superblock_t`: IVFC + 0x18 de relleno + dos
                // `bktr_header_t` consecutivas; el magic va en +0x10 de cada una.
                &section.hash_info[0..4] == b"IVFC"
                    && (&section.patch_info[0x10..0x14] == b"BKTR"
                        || &section.patch_info[0x30..0x34] == b"BKTR")
            }
            (PartitionType::RomFs, _) => &section.hash_info[0..4] == b"IVFC",
            _ => false,
        };
        let mark = if magic_ok { "" } else { " (?)" };
        sections.push(format!(
            "{} {}{}",
            section.partition_type.label(),
            section.encryption.label(),
            mark
        ));
    }

    ContentLine {
        name: entry.name.clone(),
        size: entry.size,
        content_type: content_type_label(content_type),
        nca_type: format!(
            "{} · {} · rights {}",
            header.format.label(),
            header.content_type.label(),
            if header.has_rights_id() {
                short_rights(&header)
            } else {
                "—".to_string()
            }
        ),
        sections,
        key,
        error,
        present: true,
    }
}

fn short_rights(header: &NcaHeader) -> String {
    let hex = header.rights_id_hex();
    format!("{}…{}", &hex[..8], &hex[hex.len() - 4..])
}

fn content_type_label(value: u8) -> &'static str {
    // `nn::ncm::ContentType`, no confundir con el `ContentType` de la NCA.
    match value {
        0 => "meta",
        1 => "programa",
        2 => "datos",
        3 => "control",
        4 => "html",
        5 => "legal",
        6 => "delta",
        _ => "?",
    }
}
