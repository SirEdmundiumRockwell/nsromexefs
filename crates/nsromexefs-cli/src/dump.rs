//! Subcomando `dump`: clasificación de contenedores, plan y extracción.

use std::path::{Path, PathBuf};

use nsromexefs_core::{
    exefs_index, extract_exefs, extract_romfs, format_version, prepare_output, romfs_index,
    EncryptionType, Error, ExtractedFile, KeyOrigin, KeySet, MetaType, ResumeFile, SectionRead,
    WriteOptions,
};

use crate::container::{merged_source, open_containers, plain_source, probe_titles, Container};
use crate::state::{atomic_json, plan_hash, source_fingerprint, timestamp_utc, OutputSession};
use crate::util::{human_size, load_keys_lenient, short_id};

#[derive(Debug, Clone)]
pub(crate) struct DumpOptions {
    pub exefs_only: bool,
    pub romfs_only: bool,
    pub dry_run: bool,
    pub force: bool,
    pub verbose: bool,
    pub json: bool,
    pub tree: bool,
    /// Fuerza una versión concreta (visible, del CNMT o entera).
    pub version: Option<String>,
    /// Reanuda una extracción desde el journal por archivo.
    pub resume: bool,
}

pub(crate) fn cmd_dump(
    files: &[PathBuf],
    explicit_keys: &[PathBuf],
    out: Option<&Path>,
    opts: DumpOptions,
) -> Result<(), Error> {
    if files.is_empty() || files.len() > 2 {
        return Err(Error::Invalid(
            "dump accepts one or two containers: the base and, optionally, its update".into(),
        ));
    }
    let keys = load_keys_lenient(explicit_keys).ok_or(Error::MissingHeaderKey)?;
    if keys.header_key().is_none() {
        return Err(Error::MissingHeaderKey);
    }

    // Un archivo puede combinar base + DLC. Se detectan todos los títulos y se
    // extrae cada uno por separado, compartiendo la raíz de salida.
    let titles = discover_titles(files, &keys)?;
    if titles.is_empty() {
        return Err(Error::MissingBase);
    }
    if titles.len() > 1 && !opts.tree {
        return Err(Error::Invalid(format!(
            "the container combines {} titles (base and DLC); add --tree to extract them all",
            titles.len()
        )));
    }

    let mut reports = Vec::new();
    for (title_id, _) in &titles {
        reports.push(dump_paths(files, &keys, out, opts.clone(), Some(title_id))?);
    }

    if opts.json {
        let payload = serde_json::json!({
            "count": reports.len(),
            "titles": reports.iter().map(DumpReport::to_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
    } else {
        for (index, report) in reports.iter().enumerate() {
            if index > 0 {
                println!();
            }
            report.print_human();
        }
    }
    Ok(())
}

/// Títulos base/DLC del contenedor, con la base primero. Los updates no se
/// extraen por sí solos: acompañan a su base.
fn discover_titles(files: &[PathBuf], keys: &KeySet) -> Result<Vec<(String, MetaType)>, Error> {
    let mut titles: Vec<(String, MetaType)> = Vec::new();
    for path in files {
        for probe in probe_titles(path, keys)? {
            if !matches!(
                probe.meta_type,
                MetaType::Application | MetaType::AddOnContent
            ) {
                continue;
            }
            if titles.iter().any(|(id, _)| id == &probe.title_id) {
                continue;
            }
            titles.push((probe.title_id, probe.meta_type));
        }
    }
    titles.sort_by(|a, b| {
        a.1.short_label()
            .cmp(b.1.short_label())
            .then_with(|| a.0.cmp(&b.0))
    });
    Ok(titles)
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
    pub crypto_revision: u8,
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
    pub elapsed_ms: u128,
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
            "crypto_revision": self.crypto_revision,
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
            "manifest": (!self.dry_run)
                .then(|| self.out_dir.join("manifest.json").display().to_string()),
            "timings": {"total_ms": self.elapsed_ms},
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
            "  title      {} · {}meta v{} · {}",
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
            "  program    {}  [key OK ({})]",
            short_id(&self.program_name),
            self.key_origin.label()
        );

        if let Some(exefs) = &self.exefs {
            println!(
                "  exefs      {} files · {}",
                exefs.entries.len(),
                human_size(exefs.bytes)
            );
        }
        if let Some(romfs) = &self.romfs {
            let origin = if self.merged {
                " · merged with the base"
            } else {
                ""
            };
            println!(
                "  romfs      {} files · {} · {} directories{origin}",
                romfs.files,
                human_size(romfs.bytes),
                romfs.dirs
            );
        }
        if self.dry_run {
            println!("  plan       no writes (--dry-run)");
        } else {
            println!("  output     {}", self.out_dir.display());
            if let Some(count) = self.checksums {
                println!("  SHA256SUMS {count} entries");
            }
            if self.elapsed_ms > 0 {
                println!("  duration   {} ms", self.elapsed_ms);
            }
        }
        for warning in &self.warnings {
            println!("  warning    {warning}");
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
    target: Option<&str>,
) -> Result<DumpReport, Error> {
    let started = std::time::Instant::now();
    // Clasificar por CNMT, nunca por el nombre del archivo. `target` limita la
    // apertura al título pedido (y a sus parches), de modo que un archivo
    // combinado base + DLC no exige las claves de los títulos que no se extraen.
    let mut bases: Vec<Container> = Vec::new();
    let mut updates: Vec<Container> = Vec::new();
    let mut warnings = Vec::new();

    for path in files {
        for container in open_containers(path, keys, target)? {
            match container.meta_info.cnmt.meta_type {
                MetaType::Application | MetaType::AddOnContent => bases.push(container),
                MetaType::Patch => updates.push(container),
                other => {
                    return Err(Error::Unsupported(format!(
                        "unsupported content type: {other:?}"
                    )))
                }
            }
        }
    }

    let mut base = match target {
        Some(id) => bases.into_iter().next().ok_or_else(|| {
            Error::Invalid(format!("title {id} was not found in the given containers"))
        })?,
        None => {
            if bases.len() > 1 {
                return Err(Error::Invalid(
                    "the container combines multiple titles (base + DLC); \
                     use --tree to extract them all"
                        .into(),
                ));
            }
            bases.into_iter().next().ok_or(Error::MissingBase)?
        }
    };
    let mut update = select_update(updates, &base, opts.version.as_deref(), &mut warnings)?;

    let merged = update.is_some();
    let want_exefs = !opts.romfs_only;
    let want_romfs = !opts.exefs_only;

    let romfs_available =
        update.as_ref().is_some_and(|u| u.romfs.is_some()) || base.romfs.is_some();
    if opts.romfs_only && !romfs_available {
        return Err(Error::Invalid("this content has no romfs".into()));
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
    // propio id ya es el de la aplicación. Un DLC se agrupa bajo el id del
    // juego aunque lo que se extraiga sea su parche.
    let app_id = match base.meta_info.cnmt.meta_type {
        MetaType::AddOnContent => base
            .meta_info
            .cnmt
            .application_id()
            .map(|id| format!("{id:016X}"))
            .unwrap_or_else(|| base.title_id()),
        _ => base_title_id.clone(),
    };
    let version_text = display_version
        .clone()
        .unwrap_or_else(|| version_raw.to_string());

    // El layout de un DLC (con o sin parche) siempre va en `dlc/<dlc_id>`.
    let (layout_type, layout_title_id) = match base.meta_info.cnmt.meta_type {
        MetaType::AddOnContent => (MetaType::AddOnContent, base.title_id()),
        _ => (meta_type, title_id.clone()),
    };

    let out_dir = if opts.tree {
        let root = out
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        root.join(tree_relative(
            layout_type,
            &app_id,
            &layout_title_id,
            &version_text,
        ))
    } else {
        out.map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(&title_id))
    };

    // exefs: del update si lo trae (código parcheado); si no, de la base.
    let exefs_in_update = update.as_ref().is_some_and(|u| u.exefs.is_some());
    let (program_name, key_origin, crypto_revision) = {
        let container = if exefs_in_update {
            update.as_ref().unwrap()
        } else {
            &base
        };
        if opts.exefs_only && container.exefs.is_none() {
            return Err(Error::Invalid(
                "this content has no exefs (is it a DLC?)".into(),
            ));
        }
        (
            container.program_name.clone(),
            container.key_origin,
            container.crypto_revision,
        )
    };

    // Índices primero: permiten un `--dry-run` real y detectan colisiones antes
    // de escribir un solo byte. La RomFS se planifica antes de extraer exefs
    // para que el preflight conozca el tamaño total de la salida.
    let exefs_plan = if want_exefs {
        let Container {
            file,
            exefs,
            content_key,
            ..
        } = exefs_container(&mut base, update.as_mut());
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

    // romfs: la RomFS virtual del update (BKTR) se fusiona bloque a bloque con
    // la RomFS de la base; no se materializa ninguna NCA intermedia.
    let romfs_plan = if want_romfs {
        let mut source = open_romfs_source(&mut base, update.as_mut())?;
        Some(romfs_index(source.as_mut())?)
    } else {
        None
    };

    let total_bytes = exefs_bytes + romfs_plan.as_ref().map_or(0, |index| index.bytes);
    prepare_output(
        &out_dir,
        opts.force || opts.resume,
        total_bytes,
        opts.dry_run,
    )?;

    let fingerprint = source_fingerprint(files)?;
    let identity = serde_json::json!({
        "sources": fingerprint,
        "title_id": title_id,
        "base_title_id": base_title_id,
        "version_raw": version_raw,
        "meta_type": meta_type.short_label(),
        "merged": merged,
        "exefs_only": opts.exefs_only,
        "romfs_only": opts.romfs_only,
        "exefs": exefs_plan.as_ref().map(|entries| entries.iter().map(|(path, size)| (path, size)).collect::<Vec<_>>()),
        "romfs": romfs_plan.as_ref().map(|index| index.files.iter().map(|entry| (&entry.path, entry.size)).collect::<Vec<_>>()),
    });
    let current_plan_hash = plan_hash(&identity);
    let mut session = if opts.dry_run {
        None
    } else {
        Some(OutputSession::begin(
            &out_dir,
            opts.resume,
            opts.force,
            fingerprint,
            current_plan_hash.clone(),
        )?)
    };
    let resume_files: std::collections::BTreeMap<String, ResumeFile> = session
        .as_ref()
        .map(OutputSession::resume_files)
        .unwrap_or_default();

    let mut exefs_files = Vec::new();
    if !opts.dry_run && exefs_plan.is_some() {
        let Container {
            file,
            exefs,
            content_key,
            ..
        } = exefs_container(&mut base, update.as_mut());
        let mut source =
            nsromexefs_core::PlainSection::new(file, exefs.as_ref().unwrap(), content_key);
        let exefs_options = WriteOptions {
            force: opts.force || opts.resume,
            resume: resume_files.clone().into_iter().collect(),
            resume_prefix: "exefs/".into(),
        };
        let mut on_exefs = |file: &ExtractedFile| {
            if opts.verbose {
                println!("    exefs/{}", file.path);
            }
            if let Some(session) = session.as_mut() {
                session.checkpoint(
                    &format!("exefs/{}", file.path),
                    file.size,
                    &file.sha256_hex(),
                )?;
            }
            Ok(())
        };
        exefs_files = extract_exefs(
            &mut source,
            &out_dir.join("exefs"),
            &exefs_options,
            &mut on_exefs,
        )?;
    }

    let mut written = 0u64;
    let mut next_report = 1u64 << 30;
    let mut on_file = |file: &ExtractedFile| {
        if opts.verbose {
            println!("    romfs/{}", file.path);
        }
        written += file.size;
        if written >= next_report {
            eprintln!(
                "    … {} of {}",
                human_size(written),
                human_size(total_bytes)
            );
            next_report = written + (1 << 30);
        }
        if let Some(session) = session.as_mut() {
            session.checkpoint(
                &format!("romfs/{}", file.path),
                file.size,
                &file.sha256_hex(),
            )?;
        }
        Ok(())
    };

    let mut romfs_files = Vec::new();
    if !opts.dry_run {
        if let Some(index) = romfs_plan.as_ref() {
            let mut source = open_romfs_source(&mut base, update.as_mut())?;
            let romfs_options = WriteOptions {
                force: opts.force || opts.resume,
                resume: resume_files.clone().into_iter().collect(),
                resume_prefix: "romfs/".into(),
            };
            romfs_files = extract_romfs(
                source.as_mut(),
                index,
                &out_dir.join("romfs"),
                &romfs_options,
                &mut on_file,
            )?;
        }
    }

    let checksums = !opts.dry_run && (!exefs_files.is_empty() || !romfs_files.is_empty());
    if checksums {
        write_sha256sums(&out_dir, &exefs_files, &romfs_files)?;
        let manifest = build_manifest(
            files,
            keys,
            &title_id,
            &base_title_id,
            &version_text,
            version_raw,
            meta_type,
            merged,
            &program_name,
            crypto_revision,
            exefs_plan.is_some(),
            &exefs_files,
            romfs_plan.as_ref(),
            &current_plan_hash,
        );
        atomic_json(&out_dir.join("manifest.json"), &manifest)?;
    }
    if let Some(session) = session.take() {
        session.finish()?;
    }

    Ok(DumpReport {
        inputs: files.to_vec(),
        merged,
        title_id,
        base_title_id,
        meta_type,
        program_name,
        key_origin,
        crypto_revision,
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
        elapsed_ms: started.elapsed().as_millis(),
    })
}

/// Contenedor del que sale `exefs`: el update si trae código parcheado.
fn exefs_container<'a>(
    base: &'a mut Container,
    update: Option<&'a mut Container>,
) -> &'a mut Container {
    match update {
        Some(up) if up.exefs.is_some() => up,
        _ => base,
    }
}

/// Fuente de la RomFS: la del update fusionada con BKTR, la del update si es
/// plana, o la de la base.
fn open_romfs_source<'a>(
    base: &'a mut Container,
    update: Option<&'a mut Container>,
) -> Result<Box<dyn SectionRead + 'a>, Error> {
    match update {
        Some(up)
            if up
                .romfs
                .as_ref()
                .is_some_and(|section| section.encryption == EncryptionType::Bktr) =>
        {
            merged_source(base, up)
        }
        Some(up) if up.romfs.is_some() => plain_source(up),
        _ => plain_source(base),
    }
}

/// Elige el update: el más alto por defecto, o el que pida `--version`.
///
/// Acepta la versión visible (`1.1.0`), la del CNMT (`2.0.0`) o el entero
/// interno (`131072`). Si la versión pedida es la de la base, se extrae la base
/// sin update. Un update que no pertenece a la base es `E_BASE_MISMATCH`.
fn select_update(
    updates: Vec<Container>,
    base: &Container,
    requested: Option<&str>,
    warnings: &mut Vec<String>,
) -> Result<Option<Container>, Error> {
    let base_id = base.meta_info.cnmt.title_id;
    let mut matching: Vec<Container> = Vec::new();
    let mut mismatched: Option<Container> = None;

    for update in updates {
        let belongs = update.meta_info.cnmt.application_id() == Some(base_id)
            || update.meta_info.cnmt.title_id == base_id.wrapping_add(0x800);
        if belongs {
            matching.push(update);
        } else if mismatched.is_none() {
            mismatched = Some(update);
        }
    }

    let available = available_versions(base, &matching);

    if let Some(requested) = requested {
        let mut selected: Vec<Container> = matching
            .into_iter()
            .filter(|update| version_matches(update, requested))
            .collect();
        if selected.is_empty() {
            if version_matches(base, requested) {
                warnings.push(format!(
                    "--version {requested}: extracting base v{} without an update",
                    version_label(base)
                ));
                return Ok(None);
            }
            return Err(Error::VersionNotFound {
                requested: requested.to_string(),
                available,
            });
        }
        selected.sort_by_key(|update| update.meta_info.cnmt.version);
        let best = selected.pop().unwrap();
        for other in selected {
            warnings.push(format!(
                "--version {requested}: ignoring update v{}",
                version_label(&other)
            ));
        }
        return Ok(Some(best));
    }

    let mut best: Option<Container> = None;
    for candidate in matching {
        let version = candidate.meta_info.cnmt.version;
        match &best {
            None => best = Some(candidate),
            Some(current) if version > current.meta_info.cnmt.version => {
                warnings.push(format!(
                    "multiple updates: using v{} instead of v{}",
                    version_label(&candidate),
                    version_label(current)
                ));
                best = Some(candidate);
            }
            Some(current) => warnings.push(format!(
                "multiple updates: ignoring v{} in favor of v{}",
                version_label(&candidate),
                version_label(current)
            )),
        }
    }

    if best.is_none() {
        if let Some(other) = mismatched {
            let expected = other
                .meta_info
                .cnmt
                .application_id()
                .map(|id| format!("{id:016X}"))
                .unwrap_or_else(|| "unknown".into());
            return Err(Error::BaseMismatch {
                expected,
                actual: base.title_id(),
            });
        }
    }
    Ok(best)
}

/// Etiqueta de versión: la visible si existe; si no, la del CNMT.
fn version_label(container: &Container) -> String {
    container
        .display_version
        .clone()
        .unwrap_or_else(|| format_version(container.meta_info.cnmt.version))
}

/// Compara la versión pedida con las tres representaciones admitidas.
fn version_matches(container: &Container, requested: &str) -> bool {
    version_matches_parts(
        container.display_version.as_deref(),
        container.meta_info.cnmt.version,
        requested,
    )
}

fn version_matches_parts(display: Option<&str>, raw: u32, requested: &str) -> bool {
    let requested = requested.trim();
    let bare = requested
        .strip_prefix('v')
        .or_else(|| requested.strip_prefix('V'))
        .unwrap_or(requested);
    bare == raw.to_string()
        || bare == format_version(raw)
        || display.is_some_and(|display| display.eq_ignore_ascii_case(bare))
}

fn available_versions(base: &Container, updates: &[Container]) -> String {
    let mut labels = vec![format!("{} (base)", version_label(base))];
    for update in updates {
        let label = format!("{} (update)", version_label(update));
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels.join(", ")
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

#[allow(clippy::too_many_arguments)]
fn build_manifest(
    files: &[PathBuf],
    keys: &KeySet,
    title_id: &str,
    base_title_id: &str,
    version: &str,
    version_raw: u32,
    meta_type: MetaType,
    merged: bool,
    program_name: &str,
    crypto_revision: u8,
    include_exefs: bool,
    exefs_files: &[ExtractedFile],
    romfs: Option<&nsromexefs_core::RomFsIndex>,
    plan_hash: &str,
) -> serde_json::Value {
    let main = exefs_files.iter().find(|file| file.path == "main");
    let key_sources: Vec<String> = keys
        .sources()
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    serde_json::json!({
        "schema": 1,
        "title_id": title_id,
        "base_title_id": base_title_id,
        "meta_type": meta_type.label(),
        "version": version,
        "version_raw": version_raw,
        "sources": files.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "merged": merged,
        "crypto_revision": crypto_revision,
        "keys_source": key_sources,
        "program": program_name,
        "exefs": include_exefs.then(|| serde_json::json!({
            "files": exefs_files.len(),
            "main_sha256": main.map(ExtractedFile::sha256_hex),
            "main_size": main.map(|file| file.size),
        })),
        "romfs": romfs.map(|index| serde_json::json!({
            "files": index.file_count(),
            "bytes": index.bytes,
            "directories": index.dirs,
            "changed_files": serde_json::Value::Null,
        })),
        "sanitized_paths": [],
        "skipped_entries": [],
        "plan_hash": plan_hash,
        "timestamp": timestamp_utc(),
    })
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
    fn version_matches_display_cnmt_and_raw() {
        let raw = 131_072;
        for requested in ["1.1.0", "2.0.0", "131072", "v1.1.0", " 1.1.0 "] {
            assert!(
                version_matches_parts(Some("1.1.0"), raw, requested),
                "{requested}"
            );
        }
        for requested in ["1.0.0", "131073", "3.0.0", ""] {
            assert!(
                !version_matches_parts(Some("1.1.0"), raw, requested),
                "{requested}"
            );
        }
        assert!(version_matches_parts(None, 0, "0"));
        assert!(version_matches_parts(None, 0, "0.0.0"));
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
