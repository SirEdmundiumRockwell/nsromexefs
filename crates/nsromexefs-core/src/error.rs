//! Errores del motor.
//!
//! Cada variante expone un código estable (`E_*`) pensado para `--json` y para
//! que la GUI pueda reaccionar sin parsear mensajes.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported format: {0}")]
    Unsupported(String),

    #[error("invalid file: {0}")]
    Invalid(String),

    #[error("missing `header_key` in the keyset (32 bytes are required)")]
    MissingHeaderKey,

    #[error("missing `titlekek_{revision:02}` (declared key generation: {declared})")]
    MissingTitlekek { revision: u8, declared: u8 },

    #[error("missing the title key for rights ID {rights_id}")]
    MissingTitlekey { rights_id: String },

    #[error("missing `key_area_key_{bank}_{revision:02}` in the keyset")]
    MissingKeyArea { bank: &'static str, revision: u8 },

    #[error("personalized ticket: the title key is encrypted with a console RSA key")]
    TicketPersonalized,

    #[error("invalid key file ({path}): {reason}")]
    KeysetInvalid { path: PathBuf, reason: String },

    #[error("{path} already exists; use --force to overwrite")]
    OutputExists { path: PathBuf },

    #[error("not enough space at {path}: {needed} bytes required, {available} available")]
    NoSpace {
        path: PathBuf,
        needed: u64,
        available: u64,
    },

    #[error("verification failed: {failed} issue(s) across {checked} file(s)")]
    VerifyFailed { checked: usize, failed: usize },

    #[error("version {requested} is not available (available: {available})")]
    VersionNotFound {
        requested: String,
        available: String,
    },

    #[error("name collision in the destination: {path}")]
    NameCollision { path: String },

    #[error("missing base: an update requires its base container")]
    MissingBase,

    #[error("the base does not match the update: the update belongs to {expected}, but the base is {actual}")]
    BaseMismatch { expected: String, actual: String },

    #[error("the batch finished with {failed} error(s) out of {total} title(s)")]
    BatchFailed {
        failed: usize,
        total: usize,
        /// Código de salida agregado (mismo si todos los fallos coinciden).
        exit: u8,
    },
}

impl Error {
    /// Código estable, el mismo que documenta el diseño (§22).
    pub fn code(&self) -> &'static str {
        match self {
            Error::Io(_) => "E_IO",
            Error::Unsupported(_) => "E_FORMAT_UNSUPPORTED",
            Error::Invalid(_) => "E_LAYOUT_UNEXPECTED",
            Error::MissingHeaderKey => "E_KEY_HEADER_MISSING",
            Error::MissingTitlekek { .. } => "E_KEY_TITLEKEK_MISSING",
            Error::MissingTitlekey { .. } => "E_KEY_TITLEKEY_MISSING",
            Error::MissingKeyArea { .. } => "E_KEY_AREA_MISSING",
            Error::TicketPersonalized => "E_TICKET_PERSONALIZED",
            Error::KeysetInvalid { .. } => "E_KEYSET_INVALID",
            Error::OutputExists { .. } => "E_OUTPUT_EXISTS",
            Error::NoSpace { .. } => "E_NO_SPACE",
            Error::VerifyFailed { .. } => "E_VERIFY_FAILED",
            Error::VersionNotFound { .. } => "E_VERSION_NOT_FOUND",
            Error::NameCollision { .. } => "E_NAME_COLLISION",
            Error::MissingBase => "E_BASE_MISSING",
            Error::BaseMismatch { .. } => "E_BASE_MISMATCH",
            Error::BatchFailed { .. } => "E_BATCH_FAILED",
        }
    }

    /// Pista accionable para el usuario final.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Error::MissingHeaderKey => {
                Some("your prod.keys does not include header_key; the NCA header cannot be read without it")
            }
            Error::MissingTitlekek { .. } => {
                Some("update your keyset: this NCA requires a key generation that is missing")
            }
            Error::MissingTitlekey { .. } => Some(
                "provide the content ticket or a title.keys entry for that rights ID",
            ),
            Error::MissingKeyArea { .. } => Some(
                "your prod.keys does not include key_area_key_* for that revision; update it",
            ),
            Error::TicketPersonalized => {
                Some("personalized tickets require the RSA key of the console that issued them")
            }
            Error::KeysetInvalid { .. } => {
                Some("check the format: name = hex, one key per line")
            }
            Error::OutputExists { .. } => {
                Some("add --force to replace the previous extraction")
            }
            Error::NoSpace { .. } => {
                Some("free space on the destination volume or extract to another drive")
            }
            Error::VerifyFailed { .. } => {
                Some("extract the title again or remove the damaged files")
            }
            Error::VersionNotFound { .. } => {
                Some("use the display version (1.1.0), the CNMT version (2.0.0), or the raw integer (131072)")
            }
            Error::NameCollision { .. } => Some(
                "the RomFS contains two paths that Windows cannot distinguish; extract to another file system",
            ),
            Error::MissingBase => Some(
                "an update only contains patches: also provide the base .nsp/.xci for the same game",
            ),
            Error::BaseMismatch { .. } => Some(
                "check that the update and the base belong to the same game and region",
            ),
            Error::BatchFailed { .. } => {
                Some("each title was processed separately; review the lines marked as errors")
            }
            Error::Unsupported(_) => Some("see the supported scope in the design document §2"),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn le16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

pub(crate) fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub(crate) fn le64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable() {
        assert_eq!(Error::Io(std::io::Error::other("x")).code(), "E_IO");
        assert_eq!(
            Error::Unsupported("nsz".into()).code(),
            "E_FORMAT_UNSUPPORTED"
        );
        assert_eq!(Error::Invalid("x".into()).code(), "E_LAYOUT_UNEXPECTED");
        assert_eq!(
            Error::MissingTitlekek {
                revision: 9,
                declared: 10
            }
            .code(),
            "E_KEY_TITLEKEK_MISSING"
        );
        assert_eq!(
            Error::MissingTitlekey {
                rights_id: "0100".into()
            }
            .code(),
            "E_KEY_TITLEKEY_MISSING"
        );
        assert_eq!(Error::MissingHeaderKey.code(), "E_KEY_HEADER_MISSING");
        assert_eq!(Error::TicketPersonalized.code(), "E_TICKET_PERSONALIZED");
        assert_eq!(Error::MissingBase.code(), "E_BASE_MISSING");
        assert_eq!(
            Error::NoSpace {
                path: PathBuf::from("x"),
                needed: 1,
                available: 0
            }
            .code(),
            "E_NO_SPACE"
        );
        assert_eq!(
            Error::VerifyFailed {
                checked: 2,
                failed: 1
            }
            .code(),
            "E_VERIFY_FAILED"
        );
        assert_eq!(
            Error::VersionNotFound {
                requested: "1.2.0".into(),
                available: "1.1.0".into()
            }
            .code(),
            "E_VERSION_NOT_FOUND"
        );
        assert_eq!(
            Error::BaseMismatch {
                expected: "01006BB00C6F0000".into(),
                actual: "01006BB00C6F0800".into()
            }
            .code(),
            "E_BASE_MISMATCH"
        );
    }

    #[test]
    fn every_error_with_key_context_has_a_hint() {
        assert!(Error::MissingHeaderKey.hint().is_some());
        assert!(Error::TicketPersonalized.hint().is_some());
        assert!(Error::MissingBase.hint().is_some());
        assert!(Error::BaseMismatch {
            expected: "x".into(),
            actual: "y".into()
        }
        .hint()
        .is_some());
    }
}
