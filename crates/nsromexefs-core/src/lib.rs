//! `nsromexefs-core` — motor de extracción de NSP/XCI a `exefs`/`romfs`.
//!
//! Este crate no imprime nada ni conoce la línea de comandos: recibe datos y
//! devuelve resultados. La CLI y (en v2) la GUI son adaptadores sobre él.

pub mod bktr;
pub mod cnmt;
pub mod content;
pub mod crypto;
pub mod error;
pub mod extract;
pub mod hfs0;
pub mod ivfc;
pub mod keys;
pub mod nca;
pub mod pfs0;
pub mod romfs;
pub mod section;
pub mod ticket;
pub mod xci;

pub use bktr::{BktrHeader, BktrSection, BktrTables, RelocEntry, SubsecEntry};
pub use cnmt::{Cnmt, ContentInfo, MetaType};
pub use content::{read_meta, MetaInfo};
pub use error::{Error, Result};
pub use extract::{
    exefs_index, extract_exefs, extract_romfs, romfs_index, romfs_read_file, ExtractedFile,
    RomFsIndex, WriteOptions,
};
pub use hfs0::{Hfs0, Hfs0Entry};
pub use ivfc::Ivfc;
pub use keys::{KeyBank, KeySet, KeyWarning};
pub use nca::{
    content_key, ContentType, EncryptionType, KeyOrigin, NcaFormat, NcaHeader, PartitionType,
    Section,
};
pub use pfs0::{EntryKind, Pfs0, Pfs0Entry};
pub use romfs::{RomFs, RomFsEntry, RomFsHeader};
pub use section::{PlainSection, SectionRead};
pub use ticket::Ticket;
pub use xci::{Xci, XciHeader, XciPartition};

/// Nombre y versión del motor, visibles desde `--about`.
pub const ENGINE_NAME: &str = "nsromexefs";
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
