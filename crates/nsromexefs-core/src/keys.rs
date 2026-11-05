//! Parser tolerante de `prod.keys` y `title.keys`.
//!
//! Reglas (diseño §18):
//! - `nombre = hex`, comentarios `#`/`;`, CRLF, BOM.
//! - Valores de 34 hex (bug de Lockpick) se truncan a 32 y se avisa.
//! - Claves desconocidas se ignoran sin romper.
//! - Un nombre de 32 hex con valor de 16 bytes es una entrada de `title.keys`.
//!
//! Las claves nunca se escriben en logs ni se formatean con `Debug`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Tamaño máximo aceptado para un valor. Los keysets reales incluyen material
/// RSA de hasta 528 bytes (`eticket_rsa_keypair`), aunque no lo usemos.
const MAX_HEX_LEN: usize = 2048;

/// Aviso no bloqueante emitido al parsear un archivo de claves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyWarning {
    /// Número de línea (1-based) dentro de su archivo.
    pub line: usize,
    /// Archivo de origen.
    pub origin: PathBuf,
    /// Texto para el usuario.
    pub message: String,
}

impl fmt::Display for KeyWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}",
            self.origin.display(),
            self.line,
            self.message
        )
    }
}

/// Banco de claves del *key area* al que apunta `kaek_index` en la NCA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyBank {
    Application,
    Ocean,
    System,
}

impl KeyBank {
    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(KeyBank::Application),
            1 => Some(KeyBank::Ocean),
            2 => Some(KeyBank::System),
            _ => None,
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            KeyBank::Application => "key_area_key_application",
            KeyBank::Ocean => "key_area_key_ocean",
            KeyBank::System => "key_area_key_system",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KeyBank::Application => "application",
            KeyBank::Ocean => "ocean",
            KeyBank::System => "system",
        }
    }
}

/// Conjunto de claves cargado desde uno o varios archivos.
#[derive(Default)]
pub struct KeySet {
    raw: BTreeMap<String, Vec<u8>>,
    /// `title.keys`: rights id → titlekey **cifrado** (raw del ticket).
    enc_titlekeys: HashMap<[u8; 16], [u8; 16]>,
    warnings: Vec<KeyWarning>,
    sources: Vec<PathBuf>,
}

impl fmt::Debug for KeySet {
    /// Nunca imprimir el material de claves.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeySet")
            .field("keys", &self.raw.len())
            .field("titlekeys", &self.enc_titlekeys.len())
            .field("warnings", &self.warnings.len())
            .field("sources", &self.sources)
            .finish()
    }
}

impl KeySet {
    /// Carga y combina varios archivos. El orden importa: la última definición gana.
    pub fn load<P: AsRef<Path>>(paths: &[P]) -> Result<Self> {
        let mut set = Self::default();
        for path in paths {
            let path = path.as_ref();
            let text = std::fs::read_to_string(path).map_err(|e| Error::KeysetInvalid {
                path: path.to_path_buf(),
                reason: e.to_string(),
            })?;
            set.merge_str(&text, path);
        }
        Ok(set)
    }

    /// Incorpora el contenido de un archivo ya leído en memoria.
    pub fn merge_str(&mut self, text: &str, origin: &Path) {
        self.sources.push(origin.to_path_buf());

        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for (idx, line) in text.lines().enumerate() {
            let line_no = idx + 1;
            self.parse_line(line, line_no, origin);
        }
    }

    fn parse_line(&mut self, line: &str, line_no: usize, origin: &Path) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            return;
        }

        let Some((name, value)) = line.split_once('=') else {
            self.warn(line_no, origin, "line without '='; skipped");
            return;
        };
        let name = name.trim();
        let value = value.trim();

        if name.is_empty() || value.is_empty() {
            self.warn(line_no, origin, "empty name or value; skipped");
            return;
        }

        let mut value = value;
        if let Some(rest) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            value = rest;
        }
        let value: String = value.chars().filter(|c| !c.is_whitespace()).collect();

        if !is_hex(&value) {
            self.warn(line_no, origin, "non-hexadecimal value; skipped");
            return;
        }

        let value = if value.len() == 34 {
            // Bug conocido de Lockpick: 17 bytes donde deben ser 16.
            self.warn(
                line_no,
                origin,
                &format!("'{}' has 34 hex digits; using 32 (Lockpick bug)", name),
            );
            &value[..32]
        } else {
            value.as_str()
        };

        if value.len() % 2 != 0 {
            self.warn(line_no, origin, "odd hex length; skipped");
            return;
        }
        if value.len() > MAX_HEX_LEN {
            self.warn(
                line_no,
                origin,
                &format!(
                    "value of {} bytes (maximum {}); skipped",
                    value.len() / 2,
                    MAX_HEX_LEN / 2
                ),
            );
            return;
        }

        let Some(bytes) = hex_to_vec(value) else {
            self.warn(line_no, origin, "invalid hex; skipped");
            return;
        };

        // title.keys: rights id (32 hex) = titlekey cifrado (16 bytes).
        // El valor NO es la clave de contenido: se descifra con titlekek.
        if name.len() == 32 && is_hex(name) && bytes.len() == 16 {
            if let Some(rid) = hex_to_array16(name) {
                let mut tk = [0u8; 16];
                tk.copy_from_slice(&bytes);
                self.enc_titlekeys.insert(rid, tk);
                return;
            }
        }

        let canonical = canonical_name(name);
        self.raw.insert(canonical, bytes);
    }

    fn warn(&mut self, line: usize, origin: &Path, message: &str) {
        self.warnings.push(KeyWarning {
            line,
            origin: origin.to_path_buf(),
            message: message.to_string(),
        });
    }

    // ------------------------------------------------------------------
    // Consulta
    // ------------------------------------------------------------------

    /// Clave por nombre canónico (por ejemplo `header_key`).
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.raw.get(name).map(Vec::as_slice)
    }

    pub fn has(&self, name: &str) -> bool {
        self.raw.contains_key(name)
    }

    /// `header_key` (32 bytes: 16 de datos + 16 de tweak).
    pub fn header_key(&self) -> Option<[u8; 32]> {
        to_array32(self.get("header_key")?)
    }

    /// `titlekek_NN`. Recuerda: el índice es `max(crypto_type, crypto_type2) - 1`.
    pub fn titlekek(&self, revision: u8) -> Option<[u8; 16]> {
        to_array16(self.get(&format!("titlekek_{revision:02}"))?)
    }

    /// `master_key_NN`.
    pub fn master_key(&self, revision: u8) -> Option<[u8; 16]> {
        to_array16(self.get(&format!("master_key_{revision:02}"))?)
    }

    /// `key_area_key_application_NN` (KAEK de aplicación).
    pub fn key_area_key_application(&self, revision: u8) -> Option<[u8; 16]> {
        self.key_area_key(revision, KeyBank::Application)
    }

    /// KAEK de cualquier banco (`kaek_index` de la cabecera NCA).
    pub fn key_area_key(&self, revision: u8, bank: KeyBank) -> Option<[u8; 16]> {
        to_array16(self.get(&format!("{}_{revision:02}", bank.prefix()))?)
    }

    /// Titlekey **cifrado** desde `title.keys`. Hay que descifrarlo con `titlekek`.
    pub fn encrypted_titlekey(&self, rights_id: &[u8; 16]) -> Option<[u8; 16]> {
        self.enc_titlekeys.get(rights_id).copied()
    }

    pub fn titlekey_count(&self) -> usize {
        self.enc_titlekeys.len()
    }

    pub fn key_count(&self) -> usize {
        self.raw.len()
    }

    pub fn warnings(&self) -> &[KeyWarning] {
        &self.warnings
    }

    pub fn sources(&self) -> &[PathBuf] {
        &self.sources
    }

    /// Revisión más alta cubierta por `titlekek_NN` o `master_key_NN`.
    pub fn max_revision(&self) -> Option<u8> {
        self.raw
            .keys()
            .filter_map(|name| {
                suffix_revision(name, "titlekek_").or_else(|| suffix_revision(name, "master_key_"))
            })
            .max()
    }

    /// Nombres de las familias presentes, para `keys --check`.
    pub fn families(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("header_key", self.has("header_key")),
            (
                "titlekek",
                self.raw.keys().any(|k| k.starts_with("titlekek_")),
            ),
            (
                "master_key",
                self.raw.keys().any(|k| k.starts_with("master_key_")),
            ),
            (
                "key_area_key_application",
                self.raw
                    .keys()
                    .any(|k| k.starts_with("key_area_key_application_")),
            ),
        ]
    }
}

fn canonical_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("title_key_") {
        return format!("titlekek_{rest}");
    }
    if let Some(rest) = lower.strip_prefix("kaek_app_") {
        return format!("key_area_key_application_{rest}");
    }
    lower
}

fn suffix_revision(name: &str, prefix: &str) -> Option<u8> {
    let rest = name.strip_prefix(prefix)?;
    rest.parse::<u8>().ok()
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex_to_vec(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        out.push(hex_val(chunk[0])? << 4 | hex_val(chunk[1])?);
    }
    Some(out)
}

fn hex_to_array16(s: &str) -> Option<[u8; 16]> {
    if s.len() != 32 {
        return None;
    }
    let v = hex_to_vec(s)?;
    let mut out = [0u8; 16];
    out.copy_from_slice(&v);
    Some(out)
}

fn to_array16(slice: &[u8]) -> Option<[u8; 16]> {
    slice.try_into().ok()
}

fn to_array32(slice: &[u8]) -> Option<[u8; 32]> {
    slice.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> KeySet {
        let mut set = KeySet::default();
        set.merge_str(text, Path::new("test.keys"));
        set
    }

    const HEADER: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
    const KEY16: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";

    #[test]
    fn parses_header_key() {
        let set = parse(&format!("header_key = {HEADER}\n"));
        assert!(set.header_key().is_some());
        assert_eq!(set.warnings().len(), 0);
    }

    #[test]
    fn truncates_34_hex_and_warns() {
        // 17 bytes en hex = 34 caracteres: el bug de Lockpick.
        let value = "ab".repeat(17);
        assert_eq!(value.len(), 34);
        let set = parse(&format!("master_kek_source_06 = {value}\n"));
        assert_eq!(set.warnings().len(), 1);
        assert!(set.warnings()[0].message.contains("34 hex"));
        assert_eq!(set.get("master_kek_source_06").unwrap().len(), 16);
    }

    #[test]
    fn parses_titlekeys_entry() {
        let rid = "01006bb00c6f00000000000000000008";
        let tk = "00112233445566778899aabbccddeeff";
        let set = parse(&format!("{rid} = {tk}\n"));
        assert_eq!(set.titlekey_count(), 1);
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hex_to_vec(rid).unwrap());
        assert_eq!(set.encrypted_titlekey(&bytes).unwrap()[0], 0x00);
        assert_eq!(set.encrypted_titlekey(&bytes).unwrap()[15], 0xff);
        assert_eq!(set.key_count(), 0);
    }

    #[test]
    fn canonicalizes_aliases() {
        let set = parse(&format!("title_key_09 = {KEY16}\nkaek_app_07 = {KEY16}\n"));
        assert!(set.titlekek(9).is_some());
        assert!(set.key_area_key_application(7).is_some());
    }

    #[test]
    fn tolerates_bom_crlf_comments_and_noise() {
        let text = format!(
            "\u{feff}# comentario\r\n\r\nheader_key={HEADER}\r\nbasura sin igual\r\n  \r\n; otro comentario\r\n"
        );
        let set = parse(&text);
        assert!(set.header_key().is_some());
        assert_eq!(set.warnings().len(), 1);
        assert!(set.warnings()[0].message.contains("without '='"));
    }

    #[test]
    fn max_revision_ignores_other_families() {
        let set = parse(&format!(
            "titlekek_07 = {KEY16}\ntitlekek_20 = {KEY16}\nmaster_key_12 = {KEY16}\n"
        ));
        assert_eq!(set.max_revision(), Some(20));
    }

    #[test]
    fn rejects_odd_and_non_hex_values() {
        let set = parse("header_key = abc\nmaster_key_00 = zzzz\n");
        assert_eq!(set.warnings().len(), 2);
        assert!(set.header_key().is_none());
    }

    #[test]
    fn accepts_long_rsa_material_without_warning() {
        // `eticket_rsa_keypair` real: 528 bytes.
        let value = "ab".repeat(528);
        let set = parse(&format!("eticket_rsa_keypair = {value}\n"));
        assert!(set.warnings().is_empty());
        assert_eq!(set.get("eticket_rsa_keypair").unwrap().len(), 528);
    }

    #[test]
    fn last_definition_wins() {
        let mut set = KeySet::default();
        set.merge_str(&format!("header_key = {HEADER}\n"), Path::new("a"));
        set.merge_str(
            &format!("header_key = {}\n", "00".repeat(32)),
            Path::new("b"),
        );
        assert_eq!(set.header_key().unwrap()[0], 0x00);
        assert_eq!(set.sources().len(), 2);
    }
}
