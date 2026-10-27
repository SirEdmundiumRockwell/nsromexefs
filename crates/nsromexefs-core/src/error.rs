//! Errores del motor.
//!
//! Cada variante expone un código estable (`E_*`) pensado para `--json` y para
//! que la GUI pueda reaccionar sin parsear mensajes.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("formato no soportado: {0}")]
    Unsupported(String),

    #[error("archivo inválido: {0}")]
    Invalid(String),

    #[error("falta `header_key` en el keyset (se necesitan 32 bytes)")]
    MissingHeaderKey,

    #[error("falta `titlekek_{revision:02}` (generación de clave declarada: {declared})")]
    MissingTitlekek { revision: u8, declared: u8 },

    #[error("falta el titlekey del rights id {rights_id}")]
    MissingTitlekey { rights_id: String },

    #[error("falta `key_area_key_{bank}_{revision:02}` en el keyset")]
    MissingKeyArea { bank: &'static str, revision: u8 },

    #[error("ticket personalizado: el titlekey está cifrado con la clave RSA de una consola")]
    TicketPersonalized,

    #[error("archivo de claves inválido ({path}): {reason}")]
    KeysetInvalid { path: PathBuf, reason: String },

    #[error("ya existe {path}; usa --force para sobrescribir")]
    OutputExists { path: PathBuf },

    #[error("espacio insuficiente en {path}: se necesitan {needed} bytes y hay {available}")]
    NoSpace {
        path: PathBuf,
        needed: u64,
        available: u64,
    },

    #[error("la verificación falló: {failed} problema(s) en {checked} archivo(s)")]
    VerifyFailed { checked: usize, failed: usize },

    #[error("no está disponible la versión {requested} (disponibles: {available})")]
    VersionNotFound {
        requested: String,
        available: String,
    },

    #[error("colisión de nombres en el destino: {path}")]
    NameCollision { path: String },

    #[error("falta la base: para un update hay que indicar también su contenedor base")]
    MissingBase,

    #[error("la base no corresponde al update: el update pertenece a {expected}, pero la base es {actual}")]
    BaseMismatch { expected: String, actual: String },

    #[error("el lote terminó con {failed} error(es) de {total} título(s)")]
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
                Some("tu prod.keys no incluye header_key; sin ella no se puede leer la cabecera NCA")
            }
            Error::MissingTitlekek { .. } => {
                Some("actualiza tu keyset: falta una generación de claves que esta NCA necesita")
            }
            Error::MissingTitlekey { .. } => Some(
                "aporta el ticket del contenido o una entrada en title.keys para ese rights id",
            ),
            Error::MissingKeyArea { .. } => Some(
                "tu prod.keys no incluye key_area_key_* para esa revisión; actualízalo",
            ),
            Error::TicketPersonalized => {
                Some("los tickets personalizados requieren la clave RSA de la consola que los emitió")
            }
            Error::KeysetInvalid { .. } => {
                Some("revisa el formato: nombre = hex, una clave por línea")
            }
            Error::OutputExists { .. } => {
                Some("añade --force si quieres reemplazar la extracción anterior")
            }
            Error::NoSpace { .. } => {
                Some("libera espacio en el volumen de destino o extrae en otro disco")
            }
            Error::VerifyFailed { .. } => {
                Some("vuelve a extraer el título o elimina los archivos dañados")
            }
            Error::VersionNotFound { .. } => {
                Some("usa la versión visible (1.1.0), la del CNMT (2.0.0) o el entero (131072)")
            }
            Error::NameCollision { .. } => Some(
                "el RomFS trae dos rutas que Windows no distingue; extrae en otro sistema de archivos",
            ),
            Error::MissingBase => Some(
                "un update solo contiene parches: indica también el .nsp/.xci base del mismo juego",
            ),
            Error::BaseMismatch { .. } => Some(
                "comprueba que el update y la base son del mismo juego y región",
            ),
            Error::BatchFailed { .. } => {
                Some("cada título se procesó por separado; revisa las líneas marcadas como error")
            }
            Error::Unsupported(_) => Some("consulta el alcance soportado en el diseño §2"),
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
