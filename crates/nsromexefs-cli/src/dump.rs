//! Subcomando `dump`: clasificación de contenedores, plan y extracción.

use std::path::{Path, PathBuf};

use nsromexefs_core::{
    exefs_index, extract_exefs, extract_romfs, romfs_index, EncryptionType, Error, ExtractedFile,
    KeyOrigin, KeySet, MetaType, SectionRead, WriteOptions,
};

use crate::container::{merged_source, open_containers, plain_source, Container};
use crate::util::{human_size, load_keys_lenient, short_id};

#[derive(Debug, Clone, Copy)]
pub(crate) struct DumpOptions {
    pub exefs_only: bool,
    pub romfs_only: bool,
    pub dry_run: bool,
    pub force: bool,
    pub verbose: bool,
    pub json: bool,
    pub tree: bool,
}

pub(crate) fn cmd_dump(
    files: &[PathBuf],
    explicit_keys: &[PathBuf],
    out: Option<&Path>,
    opts: DumpOptions,
) -> Result<(), Error> {
    if files.is_empty() || files.len() > 2 {
        return Err(Error::Invalid(
            "dump acepta uno o dos contenedores: la base y, opcionalmente, su update".into(),
        ));
    }
    let keys = load_keys_lenient(explicit_keys).ok_or(Error::MissingHeaderKey)?;
    if keys.header_key().is_none() {
        return Err(Error::MissingHeaderKey);
    }

    let report = dump_paths(files, &keys, out, opts)?;
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report.to_json()).unwrap()
        );
    } else {
        report.print_human();
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct ExefsReport {
    pub entries: Vec<(String, u64)>,
    pub bytes: u64,
}

#[derive(Debug)]
pub(crate) struct RomfsReport {
    pub files: usize,
    pub dirs: usize,
    pub bytes: u64,
    pub fs_size: u64,
}

#[derive(Debug)]
pub(crate) struct DumpReport {
    pub inputs: Vec<PathBuf>,
    pub merged: bool,
    pub title_id: String,
    pub base_title_id: String,
    pub meta_type: MetaType,
    pub program_name: String,
    pub key_origin: KeyOrigin,
    pub version_raw: u32,
    pub display_version: Option<String>,
    pub base_version_raw: u32,
    pub base_display_version: Option<String>,
    pub out_dir: PathBuf,
    pub dry_run: bool,
    pub exefs: Option<ExefsReport>,
    pub romfs: Option<RomfsReport>,
    pub checksums: Option<usize>,
    pub warnings: Vec<String>,
}

impl DumpReport {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "inputs": self.inputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "merged": self.merged,
            "title_id": self.title_id,
            "base_title_id": self.base_title_id,
            "meta_type": self.meta_type.label(),
            "meta_short": self.meta_type.short_label(),
            "version_raw": self.version_raw,
            "display_version": &self.display_version,
            "base_version_raw": self.base_version_raw,
            "base_display_version": &self.base_display_version,
            "program": self.program_name,
            "key_origin": self.key_origin.label(),
            "output": self.out_dir.display().to_string(),
            "dry_run": self.dry_run,
            "exefs": self.exefs.as_ref().map(|exefs| serde_json::json!({
                "files": exefs.entries.len(),
                "bytes": exefs.bytes,
                "entries": exefs.entries.iter().map(|(name, size)| serde_json::json!({
                    "name": name,
                    "size": size,
                })).collect::<Vec<_>>(),
            })),
            "romfs": self.romfs.as_ref().map(|romfs| serde_json::json!({
                "files": romfs.files,
                "directories": romfs.dirs,
                "bytes": romfs.bytes,
                "fs_size": romfs.fs_size,
            })),
            "checksums": self.checksums,
            "warnings": self.warnings,
        })
    }

    pub fn print_human(&self) {
        for path in &self.inputs {
            println!("{}", path.display());
        }
        let visible = self
            .display_version
            .as_deref()
            .map(|v| format!("v{v} · "))
            .unwrap_or_default();
        println!(
            "  título     {} · {}meta v{} · {}",
            self.title_id,
            visible,
            self.version_raw,
            self.meta_type.label()
        );
        if self.merged {
            let base_visible = self
                .base_display_version
                .as_deref()
                .map(|v| format!("v{v} · "))
                .unwrap_or_default();
            println!(
                "  base       {} · {}meta v{}",
                self.base_title_id, base_visible, self.base_version_raw
            );
        }
        println!(
            "  programa   {}  [clave OK ({})]",
            short_id(&self.program_name),
            self.key_origin.label()
        );

        if let Some(exefs) = &self.exefs {
            println!(
                "  exefs      {} archivos · {}",
                exefs.entries.len(),
                human_size(exefs.bytes)
            );
        }
        if let Some(romfs) = &self.romfs {
            let origin = if self.merged {
                " · fusionado con la base"
            } else {
                ""
            };
            println!(
                "  romfs      {} archivos · {} · {} directorios{origin}",
                romfs.files,
                human_size(romfs.bytes),
                romfs.dirs
            );
        }
        if self.dry_run {
            println!("  plan       sin escribir (--dry-run)");
        } else {
            println!("  salida     {}", self.out_dir.display());
            if let Some(count) = self.checksums {
                println!("  SHA256SUMS {count} entradas");
            }
        }
        for warning in &self.warnings {
            println!("  aviso      {warning}");
        }
    }
}

/// Abre los contenedores, decide qué se extrae y lo escribe. No imprime el
/// resumen: eso lo hacen `dump` y `batch` según su formato.
pub(crate) fn dump_paths(
    files: &[PathBuf],
    keys: &KeySet,
    out: Option<&Path>,
    opts: DumpOptions,
) -> Result<DumpReport, Error> {
    // Clasificar por CNMT, nunca por el nombre del archivo. Un XCI puede
    // aportar su base (secure) y su update en una sola pasada.
    let mut base: Option<Container> = None;
    let mut updates: Vec<Container> = Vec::new();
    let mut warnings = Vec::new();

    for path in files {
        for container in open_containers(path, keys)? {
            match container.meta_info.cnmt.meta_type {
                MetaType::Application | MetaType::AddOnContent => {
                    if base.is_some() {
                        return Err(Error::Invalid(
                            "hay más de un título base o DLC en los contenedores indicados; \
                             dump extrae un título por invocación"
                                .into(),
                        ));
                    }
                    base = Some(container);
                }
                MetaType::Patch => updates.push(container),
                other => {
                    return Err(Error::Unsupported(format!(
                        "tipo de contenido no soportado: {other:?}"
                    )))
                }
            }
        }
    }

    let mut base = base.ok_or(Error::MissingBase)?;
    let mut update = select_highest_update(updates, &mut warnings);

    if let Some(up) = &update {
        match up.meta_info.cnmt.application_id() {
            Some(app) if app == base.meta_info.cnmt.title_id => {}
            Some(app) => {
                return Err(Error::BaseMismatch {
                    expected: format!("{app:016X}"),
                    actual: base.title_id(),
                })
            }
            None => {
                return Err(Error::Invalid(
                    "el CNMT del update no declara a qué aplicación pertenece".into(),
                ))
            }
        }
    }

    let merged = update.is_some();
    let want_exefs = !opts.romfs_only;
    let want_romfs = !opts.exefs_only;

    let romfs_available =
        update.as_ref().is_some_and(|u| u.romfs.is_some()) || base.romfs.is_some();
    if opts.romfs_only && !romfs_available {
        return Err(Error::Invalid("este contenido no tiene romfs".into()));
    }

    // Datos para la salida. Se copian antes de que los lectores de sección
    // tomen prestados los ficheros.
    let title = update.as_ref().unwrap_or(&base);
    let title_id = title.title_id();
    let meta_type = title.meta_info.cnmt.meta_type;
    let display_version = title
        .display_version
        .clone()
        .or_else(|| base.display_version.clone());
    let version_raw = title.meta_info.cnmt.version;
    let base_title_id = base.title_id();
    let base_display_version = base.display_version.clone();
    let base_version_raw = base.meta_info.cnmt.version;

    // En un XCI el CNMT del update apunta a la aplicación; para el base, su
    // propio id ya es el de la aplicación.
    let app_id = match meta_type {
        MetaType::AddOnContent => title
            .meta_info
            .cnmt
            .application_id()
            .map(|id| format!("{id:016X}"))
            .unwrap_or_else(|| title_id.clone()),
        _ => base_title_id.clone(),
    };
    let version_label = display_version
        .clone()
        .unwrap_or_else(|| version_raw.to_string());

    let out_dir = if opts.tree {
        let root = out
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        root.join(tree_relative(meta_type, &app_id, &title_id, &version_label))
    } else {
        out.map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(&title_id))
    };
    let write_opts = WriteOptions { force: opts.force };

    // exefs: del update si lo trae (código parcheado); si no, de la base.
    let exefs_in_update = update.as_ref().is_some_and(|u| u.exefs.is_some());
    let exefs_container: &mut Container = if exefs_in_update {
        update.as_mut().unwrap()
    } else {
        &mut base
    };
    if opts.exefs_only && exefs_container.exefs.is_none() {
        return Err(Error::Invalid(
            "este contenido no tiene exefs (¿es un DLC?)".into(),
        ));
    }
    let program_name = exefs_container.program_name.clone();
    let key_origin = exefs_container.key_origin;

    // Índices primero: permiten un `--dry-run` real y detectan colisiones antes
    // de escribir un solo byte.
    let exefs_plan = if want_exefs {
        let Container {
            file,
            exefs,
            content_key,
            ..
        } = &mut *exefs_container;
        match exefs.as_ref() {
            Some(section) => {
                let mut source = nsromexefs_core::PlainSection::new(file, section, content_key);
                Some(exefs_index(&mut source)?)
            }
            None => None,
        }
    } else {
        None
    };
    let exefs_bytes: u64 = exefs_plan
        .as_ref()
        .map(|entries| entries.iter().map(|(_, size)| size).sum())
        .unwrap_or(0);

    let mut exefs_files = Vec::new();
    if !opts.dry_run && exefs_plan.is_some() {
        let Container {
            file,
            exefs,
            content_key,
            ..
        } = &mut *exefs_container;
        let mut source =
            nsromexefs_core::PlainSection::new(file, exefs.as_ref().unwrap(), content_key);
        let mut on_exefs = |path: &str, _size: u64| {
            if opts.verbose {
                println!("    {path}");
            }
        };
        exefs_files = extract_exefs(
            &mut source,
            &out_dir.join("exefs"),
            &write_opts,
            &mut on_exefs,
        )?;
    }

    // romfs: la RomFS virtual del update (BKTR) se fusiona bloque a bloque con
    // la RomFS de la base; no se materializa ninguna NCA intermedia.
    let mut romfs_source: Option<Box<dyn SectionRead + '_>> = None;
    let romfs_plan = if want_romfs {
        let mut source: Box<dyn SectionRead + '_> = match update.as_mut() {
            Some(up)
                if up
                    .romfs
                    .as_ref()
                    .is_some_and(|section| section.encryption == EncryptionType::Bktr) =>
            {
                merged_source(&mut base, up)?
            }
            Some(up) if up.romfs.is_some() => plain_source(up)?,
            _ => plain_source(&mut base)?,
        };
        let plan = romfs_index(source.as_mut())?;
        romfs_source = Some(source);
        Some(plan)
    } else {
        None
    };

    let total_bytes = exefs_bytes + romfs_plan.as_ref().map_or(0, |index| index.bytes);
    let mut written = 0u64;
    let mut next_report = 1u64 << 30;
    let mut on_file = |path: &str, size: u64| {
        if opts.verbose {
            println!("    {path}");
        }
        written += size;
        if written >= next_report {
            eprintln!(
                "    … {} de {}",
                human_size(written),
                human_size(total_bytes)
            );
            next_report = written + (1 << 30);
        }
    };

    let mut romfs_files = Vec::new();
    if !opts.dry_run {
        if let (Some(index), Some(source)) = (romfs_plan.as_ref(), romfs_source.as_mut()) {
            romfs_files = extract_romfs(
                source.as_mut(),
                index,
                &out_dir.join("romfs"),
                &write_opts,
                &mut on_file,
            )?;
        }
    }

    let checksums = !opts.dry_run && (!exefs_files.is_empty() || !romfs_files.is_empty());
    if checksums {
        write_sha256sums(&out_dir, &exefs_files, &romfs_files)?;
    }

    Ok(DumpReport {
        inputs: files.to_vec(),
        merged,
        title_id,
        base_title_id,
        meta_type,
        program_name,
        key_origin,
        version_raw,
        display_version,
        base_version_raw,
        base_display_version,
        out_dir,
        dry_run: opts.dry_run,
        exefs: exefs_plan.map(|entries| ExefsReport {
            bytes: entries.iter().map(|(_, size)| size).sum(),
            entries,
        }),
        romfs: romfs_plan.as_ref().map(|index| RomfsReport {
            files: index.file_count(),
            dirs: index.dirs,
            bytes: index.bytes,
            fs_size: index.fs_size,
        }),
        checksums: checksums.then_some(exefs_files.len() + romfs_files.len()),
        warnings,
    })
}

/// Si hay varios updates (por ejemplo el de un XCI y un NSP suelto), se usa el
/// de versión más alta y se avisa de los demás.
fn select_highest_update(updates: Vec<Container>, warnings: &mut Vec<String>) -> Option<Container> {
    let mut best: Option<Container> = None;
    for candidate in updates {
        let version = candidate.meta_info.cnmt.version;
        match &best {
            None => best = Some(candidate),
            Some(current) if version > current.meta_info.cnmt.version => {
                warnings.push(format!(
                    "varios updates: se usa v{version} en lugar de v{}",
                    current.meta_info.cnmt.version
                ));
                best = Some(candidate);
            }
            Some(current) => warnings.push(format!(
                "varios updates: se ignora v{version} en favor de v{}",
                current.meta_info.cnmt.version
            )),
        }
    }
    best
}

/// Ruta relativa del layout `--tree` (§9.2 del diseño).
pub(crate) fn tree_relative(
    meta_type: MetaType,
    app_id: &str,
    title_id: &str,
    version: &str,
) -> PathBuf {
    let version = format!("v{}", safe_component(version));
    match meta_type {
        MetaType::AddOnContent => PathBuf::from(app_id)
            .join("dlc")
            .join(title_id)
            .join(version),
        _ => PathBuf::from(app_id).join(version),
    }
}

/// Un nombre de directorio seguro: sin separadores ni componentes especiales.
fn safe_component(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "0".to_string()
    } else {
        cleaned
    }
}

/// Escribe `SHA256SUMS` con rutas relativas al directorio de salida.
fn write_sha256sums(
    out_dir: &Path,
    exefs: &[ExtractedFile],
    romfs: &[ExtractedFile],
) -> Result<(), Error> {
    let mut lines: Vec<(String, String)> = exefs
        .iter()
        .map(|f| (format!("exefs/{}", f.path), f.sha256_hex()))
        .chain(
            romfs
                .iter()
                .map(|f| (format!("romfs/{}", f.path), f.sha256_hex())),
        )
        .collect();
    lines.sort_by(|a, b| a.0.cmp(&b.0));

    let mut text = String::new();
    for (path, hash) in lines {
        text.push_str(&hash);
        text.push_str("  ");
        text.push_str(&path);
        text.push('\n');
    }
    std::fs::write(out_dir.join("SHA256SUMS"), text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_layout_for_base_and_update() {
        assert_eq!(
            tree_relative(
                MetaType::Application,
                "01006BB00C6F0000",
                "01006BB00C6F0000",
                "1.1.0"
            ),
            PathBuf::from("01006BB00C6F0000").join("v1.1.0")
        );
        assert_eq!(
            tree_relative(
                MetaType::Patch,
                "01006BB00C6F0000",
                "01006BB00C6F0800",
                "1.2.0"
            ),
            PathBuf::from("01006BB00C6F0000").join("v1.2.0")
        );
    }

    #[test]
    fn tree_layout_for_dlc() {
        assert_eq!(
            tree_relative(
                MetaType::AddOnContent,
                "01006BB00C6F0000",
                "01006BB00C6F1001",
                "0"
            ),
            PathBuf::from("01006BB00C6F0000")
                .join("dlc")
                .join("01006BB00C6F1001")
                .join("v0")
        );
    }

    #[test]
    fn tree_version_is_sanitized() {
        let path = tree_relative(MetaType::Application, "APP", "APP", "../x");
        // `..` no debe sobrevivir como componente: el separador ya se sustituye
        // por `_`, así que la ruta queda `APP/v_.._x` y no escapa de la raíz.
        assert_eq!(path.components().count(), 2);
        assert_eq!(path.components().next().unwrap().as_os_str(), "APP");
        assert!(!path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)));
    }
}
