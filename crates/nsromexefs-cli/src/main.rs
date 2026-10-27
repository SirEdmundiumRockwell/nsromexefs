//! CLI de `nsromexefs`.
//!
//! `info` inspecciona contenedores; `dump` extrae `exefs`/`romfs` de un base,
//! DLC o base+update (fusionando BKTR); `keys` valida el keyset.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nsromexefs_core::{
    content_key, exefs_index, extract_exefs, extract_romfs, pfs0::EntryKind, read_meta,
    romfs_index, romfs_read_file, BktrSection, ContentInfo, EncryptionType, Error, ExtractedFile,
    KeyOrigin, KeySet, MetaInfo, MetaType, NcaHeader, PartitionType, Pfs0, PlainSection, Section,
    SectionRead, Ticket, WriteOptions,
};

#[derive(Parser)]
#[command(
    name = "nsromexefs",
    version,
    about = "Extrae exefs y romfs desde NSP/XCI de Nintendo Switch",
    long_about = None,
    disable_help_subcommand = true
)]
struct Cli {
    /// Archivos de claves (repetible). Si se omite, se busca prod.keys/title.keys.
    #[arg(short = 'k', long = "keys", global = true, value_name = "FILE")]
    keys: Vec<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Muestra el contenido de un NSP/XCI sin extraer nada
    Info {
        /// Archivos .nsp o .xci
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Salida legible por máquina
        #[arg(long)]
        json: bool,
    },
    /// Extrae exefs y romfs de un NSP base, DLC o base + update
    Dump {
        /// Archivos .nsp: la base y, opcionalmente, su update (en cualquier orden)
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Directorio de salida (por defecto ./<title_id>)
        #[arg(short = 'o', long = "out", value_name = "DIR")]
        out: Option<PathBuf>,
        /// Extrae solo exefs
        #[arg(long, conflicts_with = "romfs_only")]
        exefs_only: bool,
        /// Extrae solo romfs
        #[arg(long)]
        romfs_only: bool,
        /// Analiza y muestra el plan sin escribir nada
        #[arg(long)]
        dry_run: bool,
        /// Sobrescribe una extracción anterior
        #[arg(long)]
        force: bool,
        /// Lista cada archivo extraído
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Salida legible por máquina
        #[arg(long)]
        json: bool,
    },
    /// Valida y sanea archivos de claves
    Keys {
        /// (Compatibilidad) valida el keyset; es el comportamiento por defecto
        #[arg(long)]
        check: bool,
        /// Archivos prod.keys / title.keys (si se omiten, se usan los de -k)
        #[arg(value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Salida legible por máquina
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error [{}]: {err}", err.code());
            if let Some(hint) = err.hint() {
                eprintln!("  pista: {hint}");
            }
            ExitCode::from(exit_code(&err))
        }
    }
}

fn exit_code(err: &Error) -> u8 {
    match err {
        Error::MissingHeaderKey
        | Error::MissingTitlekek { .. }
        | Error::MissingTitlekey { .. }
        | Error::MissingKeyArea { .. }
        | Error::KeysetInvalid { .. } => 2,
        Error::MissingBase | Error::BaseMismatch { .. } => 3,
        Error::Unsupported(_) => 4,
        _ => 1,
    }
}

fn run(cli: &Cli) -> Result<(), Error> {
    match &cli.command {
        Command::Info { files, json } => cmd_info(files, &cli.keys, *json),
        Command::Dump {
            files,
            out,
            exefs_only,
            romfs_only,
            dry_run,
            force,
            verbose,
            json,
        } => cmd_dump(
            files,
            &cli.keys,
            out.as_deref(),
            DumpOptions {
                exefs_only: *exefs_only,
                romfs_only: *romfs_only,
                dry_run: *dry_run,
                force: *force,
                verbose: *verbose,
                json: *json,
            },
        ),
        Command::Keys { files, json, .. } => cmd_keys(files, &cli.keys, *json),
    }
}

// ---------------------------------------------------------------------------
// keys
// ---------------------------------------------------------------------------

fn cmd_keys(files: &[PathBuf], global: &[PathBuf], json: bool) -> Result<(), Error> {
    let paths = merge_paths(files, global);
    if paths.is_empty() {
        return Err(Error::KeysetInvalid {
            path: PathBuf::from("(ninguno)"),
            reason: "no se indicó ningún archivo de claves; usa -k prod.keys".into(),
        });
    }
    let set = KeySet::load(&paths)?;

    if json {
        let families: serde_json::Map<String, serde_json::Value> = set
            .families()
            .into_iter()
            .map(|(name, present)| (name.to_string(), serde_json::Value::Bool(present)))
            .collect();
        let out = serde_json::json!({
            "files": set.sources().iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "max_revision": set.max_revision(),
            "titlekeys": set.titlekey_count(),
            "keys": set.key_count(),
            "families": families,
            "warnings": set.warnings().iter().map(|w| w.message.clone()).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return Ok(());
    }

    for source in set.sources() {
        println!("{}", source.display());
    }
    let max = set.max_revision();
    for (family, present) in set.families() {
        let status = if present { "OK" } else { "falta" };
        let extra = match (family, max) {
            ("titlekek" | "master_key" | "key_area_key_application", Some(rev)) if present => {
                format!("  hasta revisión {rev}")
            }
            _ => String::new(),
        };
        println!("  {:<28} {:<6}{}", family, status, extra);
    }
    println!("  {:<28} {} entradas", "titlekeys", set.titlekey_count());
    println!("  {:<28} {}", "claves totales", set.key_count());

    if set.warnings().is_empty() {
        println!("  avisos: ninguno");
    } else {
        println!("  avisos:");
        for warning in set.warnings() {
            println!("    {warning}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// info
// ---------------------------------------------------------------------------

fn cmd_info(files: &[PathBuf], explicit_keys: &[PathBuf], json: bool) -> Result<(), Error> {
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

struct DumpOptions {
    exefs_only: bool,
    romfs_only: bool,
    dry_run: bool,
    force: bool,
    verbose: bool,
    json: bool,
}

fn cmd_dump(
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

    // Clasificar por CNMT, nunca por el nombre del archivo.
    let mut base: Option<Container> = None;
    let mut update: Option<Container> = None;
    for path in files {
        let opened = open_container(path, &keys)?;
        match opened.meta_info.cnmt.meta_type {
            MetaType::Application | MetaType::AddOnContent => {
                if base.is_some() {
                    return Err(Error::Invalid(
                        "se indicaron dos contenedores base; dump admite uno como máximo".into(),
                    ));
                }
                base = Some(opened);
            }
            MetaType::Patch => {
                if update.is_some() {
                    return Err(Error::Invalid(
                        "se indicaron dos updates; dump admite uno como máximo".into(),
                    ));
                }
                update = Some(opened);
            }
            other => {
                return Err(Error::Unsupported(format!(
                    "tipo de contenido no soportado: {other:?}"
                )))
            }
        }
    }

    let mut base = base.ok_or(Error::MissingBase)?;

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
    let display_version = title
        .display_version
        .clone()
        .or_else(|| base.display_version.clone());
    let version_raw = title.meta_info.cnmt.version;
    let meta_label = title.meta_info.cnmt.meta_type.label();
    let base_title_id = base.title_id();
    let base_display_version = base.display_version.clone();
    let base_version_raw = base.meta_info.cnmt.version;

    let out_dir = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(&title_id));
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
                let mut source = PlainSection::new(file, section, content_key);
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
        let mut source = PlainSection::new(file, exefs.as_ref().unwrap(), content_key);
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

    if opts.json {
        let payload = serde_json::json!({
            "inputs": files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "merged": merged,
            "title_id": title_id,
            "base_title_id": base_title_id,
            "meta_type": meta_label,
            "version_raw": version_raw,
            "display_version": &display_version,
            "base_version_raw": base_version_raw,
            "base_display_version": &base_display_version,
            "key_origin": key_origin.label(),
            "output": out_dir.display().to_string(),
            "dry_run": opts.dry_run,
            "exefs": exefs_plan.as_ref().map(|entries| serde_json::json!({
                "files": entries.len(),
                "bytes": exefs_bytes,
                "entries": entries.iter().map(|(name, size)| serde_json::json!({
                    "name": name,
                    "size": size,
                })).collect::<Vec<_>>(),
            })),
            "romfs": romfs_plan.as_ref().map(|index| serde_json::json!({
                "files": index.file_count(),
                "directories": index.dirs,
                "bytes": index.bytes,
                "fs_size": index.fs_size,
            })),
            "checksums": checksums.then_some(
                exefs_files.len() + romfs_files.len()
            ),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return Ok(());
    }

    for path in files {
        println!("{}", path.display());
    }
    let visible = display_version
        .as_deref()
        .map(|v| format!("v{v} · "))
        .unwrap_or_default();
    println!("  título     {title_id} · {visible}meta v{version_raw} · {meta_label}");
    if merged {
        let base_visible = base_display_version
            .as_deref()
            .map(|v| format!("v{v} · "))
            .unwrap_or_default();
        println!("  base       {base_title_id} · {base_visible}meta v{base_version_raw}");
    }
    println!(
        "  programa   {}  [clave OK ({})]",
        short_id(&program_name),
        key_origin.label()
    );

    if let Some(entries) = &exefs_plan {
        println!(
            "  exefs      {} archivos · {}",
            entries.len(),
            human_size(exefs_bytes)
        );
    }
    if let Some(index) = &romfs_plan {
        let origin = if merged {
            " · fusionado con la base"
        } else {
            ""
        };
        println!(
            "  romfs      {} archivos · {} · {} directorios{origin}",
            index.file_count(),
            human_size(index.bytes),
            index.dirs
        );
    }
    if opts.dry_run {
        println!("  plan       sin escribir (--dry-run)");
    } else {
        println!("  salida     {}", out_dir.display());
        if checksums {
            println!(
                "  SHA256SUMS {} entradas",
                exefs_files.len() + romfs_files.len()
            );
        }
    }

    Ok(())
}

/// Contenedor abierto y resuelto: lo necesario para planificar y extraer sin
/// volver a leer cabeceras ni tickets.
struct Container {
    file: File,
    meta_info: MetaInfo,
    content_key: [u8; 16],
    key_origin: KeyOrigin,
    program_name: String,
    exefs: Option<Section>,
    romfs: Option<Section>,
    display_version: Option<String>,
}

impl Container {
    fn title_id(&self) -> String {
        self.meta_info.cnmt.title_id_hex()
    }
}

/// Abre un NSP y resuelve su CNMT, la NCA de programa, la clave de contenido y
/// las secciones de `exefs`/`romfs`.
fn open_container(path: &Path, keys: &KeySet) -> Result<Container, Error> {
    let mut file = File::open(path)?;
    let total_size = file.metadata()?.len();
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"PFS0" {
        if looks_like_xci(&mut file)? {
            return Err(Error::Unsupported(
                "XCI detectado: la lectura de HFS0 llega en un hito posterior (M4); usa el .nsp"
                    .into(),
            ));
        }
        return Err(Error::Unsupported(format!(
            "{}: no es un NSP reconocible",
            path.display()
        )));
    }
    let pfs0 = Pfs0::read(&mut file, total_size)?;

    let meta_entry = pfs0
        .entries
        .iter()
        .find(|e| e.kind() == EntryKind::Meta)
        .ok_or_else(|| {
            Error::Invalid(format!(
                "{}: no hay ningún .cnmt.nca: no se puede identificar el título",
                path.display()
            ))
        })?;

    let tickets = read_tickets(&mut file, &pfs0);
    let meta_header = NcaHeader::read_at(
        &mut file,
        meta_entry.offset,
        meta_entry.size,
        keys.header_key().as_ref(),
    )?;
    let meta_ticket = tickets
        .list
        .iter()
        .find(|t| t.rights_id == meta_header.rights_id);
    let meta_info = read_meta(
        &mut file,
        meta_entry.offset,
        meta_entry.size,
        keys,
        meta_ticket,
    )?;

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
    let nca_name = format!("{id_hex}.nca");
    let nca_entry = pfs0
        .entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(&nca_name))
        .ok_or_else(|| {
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
        &pfs0,
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

/// RomFS sin fusionar (base, DLC o un update sin BKTR).
fn plain_source(container: &mut Container) -> Result<Box<dyn SectionRead + '_>, Error> {
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
fn merged_source<'a>(
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

struct ContainerInfo {
    path: PathBuf,
    kind: &'static str,
    total_size: u64,
    entries: Vec<EntryLine>,
    titles: Vec<TitleInfo>,
    warnings: Vec<String>,
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

        println!("  entradas del contenedor ({})", self.entries.len());
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

fn short_id(name: &str) -> String {
    let stem = name.split('.').next().unwrap_or(name);
    if stem.len() > 18 {
        format!("{}…", &stem[..16])
    } else {
        stem.to_string()
    }
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

fn inspect_container(path: &Path, keys: Option<&KeySet>) -> Result<ContainerInfo, Error> {
    let mut file = File::open(path)?;
    let total_size = file.metadata()?.len();

    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;

    if &magic != b"PFS0" {
        if looks_like_xci(&mut file)? {
            return Err(Error::Unsupported(format!(
                "{}: XCI detectado; llega en un hito posterior (M4)",
                path.display()
            )));
        }
        return Err(Error::Unsupported(format!(
            "{}: no es un NSP ni un XCI reconocible",
            path.display()
        )));
    }

    let pfs0 = Pfs0::read(&mut file, total_size)?;

    let entries = pfs0
        .entries
        .iter()
        .map(|e| EntryLine {
            name: e.name.clone(),
            kind: e.kind().label(),
            size: e.size,
        })
        .collect();

    let mut info = ContainerInfo {
        path: path.to_path_buf(),
        kind: "NSP (PFS0)",
        total_size,
        entries,
        titles: Vec::new(),
        warnings: Vec::new(),
    };

    let meta_entries: Vec<_> = pfs0
        .entries
        .iter()
        .filter(|e| e.kind() == EntryKind::Meta)
        .collect();

    if meta_entries.is_empty() {
        info.warnings
            .push("no hay ningún .cnmt.nca: no se puede identificar el título".into());
        return Ok(info);
    }

    // Los tickets se leen una sola vez; son pequeños.
    let tickets = read_tickets(&mut file, &pfs0);
    if let Some(err) = &tickets.err {
        info.warnings.push(err.clone());
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

    for meta in meta_entries {
        let header = NcaHeader::read_at(
            &mut file,
            meta.offset,
            meta.size,
            keys.header_key().as_ref(),
        );
        let meta_rights = header
            .as_ref()
            .ok()
            .filter(|h| h.has_rights_id())
            .map(|h| h.rights_id);
        let ticket = meta_rights
            .as_ref()
            .and_then(|rid| tickets.list.iter().find(|t| &t.rights_id == rid));

        match read_meta(&mut file, meta.offset, meta.size, keys, ticket) {
            Ok(meta_info) => {
                let mut contents = Vec::new();
                for content in &meta_info.cnmt.contents {
                    contents.push(inspect_content(
                        &mut file,
                        &pfs0,
                        content.content_id,
                        content.size,
                        content.content_type,
                        keys,
                        &tickets.list,
                    ));
                }
                let display_version = read_display_version(
                    &mut file,
                    &pfs0,
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

    Ok(info)
}

struct TicketList {
    list: Vec<Ticket>,
    err: Option<String>,
}

/// Lee la versión visible (`DisplayVersion`) de `control.nacp`.
///
/// La NCA de control es un RomFS normal; `control.nacp` está en su raíz y la
/// cadena vive en `0x3060` (16 bytes, terminada en NUL). Devuelve `None` si el
/// contenido no está en el contenedor, no hay clave o el campo no parece una
/// versión.
fn read_display_version(
    file: &mut File,
    pfs0: &Pfs0,
    contents: &[ContentInfo],
    keys: &KeySet,
    tickets: &[Ticket],
) -> Option<String> {
    let control = contents.iter().find(|c| c.content_type == 3)?;
    let id_hex: String = control
        .content_id
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let nca_name = format!("{id_hex}.nca");
    let entry = pfs0
        .entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(&nca_name))?;

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
        romfs_read_file(&mut source, "control.nacp").ok()??
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

fn read_tickets(file: &mut File, pfs0: &Pfs0) -> TicketList {
    let mut list = Vec::new();
    for entry in &pfs0.entries {
        if entry.kind() != EntryKind::Ticket {
            continue;
        }
        if entry.size > 0x10000 {
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

fn inspect_content(
    file: &mut File,
    pfs0: &Pfs0,
    content_id: [u8; 16],
    declared_size: u64,
    content_type: u8,
    keys: &KeySet,
    tickets: &[Ticket],
) -> ContentLine {
    let id_hex: String = content_id.iter().map(|b| format!("{b:02x}")).collect();
    let Some(entry) = pfs0
        .entries
        .iter()
        .find(|e| e.name.to_ascii_lowercase() == format!("{id_hex}.nca"))
    else {
        return ContentLine {
            name: id_hex.clone(),
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
    let (key, error) = match nsromexefs_core::content_key(&header, keys, ticket) {
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

fn looks_like_xci(file: &mut File) -> Result<bool, Error> {
    if file.metadata()?.len() < 0x104 {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(0x100))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    Ok(&magic == b"HEAD")
}

// ---------------------------------------------------------------------------
// Descubrimiento de claves
// ---------------------------------------------------------------------------

/// Une dos listas de rutas sin duplicados, conservando el orden.
fn merge_paths(a: &[PathBuf], b: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in a.iter().chain(b) {
        if !out.contains(path) {
            out.push(path.clone());
        }
    }
    out
}

/// Carga el keyset sin abortar si no hay archivos (para `info`).
fn load_keys_lenient(explicit: &[PathBuf]) -> Option<KeySet> {
    let paths = if explicit.is_empty() {
        discover_keys()
    } else {
        explicit.to_vec()
    };
    if paths.is_empty() {
        return None;
    }
    KeySet::load(&paths).ok().filter(|set| set.key_count() > 0)
}

fn discover_keys() -> Vec<PathBuf> {
    let mut found = Vec::new();

    if let Ok(env) = std::env::var("NSROMEXEFS_KEYS") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        found.extend(
            env.split(sep)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        );
    }

    let mut candidates: Vec<PathBuf> =
        vec![PathBuf::from("prod.keys"), PathBuf::from("title.keys")];
    if let Some(home) = home_dir() {
        candidates.push(home.join(".switch").join("prod.keys"));
        candidates.push(home.join(".switch").join("title.keys"));
    }
    found.extend(candidates.into_iter().filter(|p| p.is_file()));

    found
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// Formato
// ---------------------------------------------------------------------------

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}
