//! Ticket de contenido (`*.tik`).
//!
//! El ticket transporta el titlekey **cifrado** con `titlekek[rev]`. El rights
//! id enlaza el ticket con las NCAs que lo usan.
//!
//! Layout (switchbrew, verificado contra un ticket real):
//! ```text
//! 0x00  u32  tipo de firma (en disco va little-endian: 0x00010004 → 04 00 01 00)
//! 0x04  ...  firma + relleno hasta Y
//! Y+0x00  issuer (0x40)
//! Y+0x40  titlekey cifrado (0x10)
//! Y+0x141 tipo de titlekey (0 = AES, 1 = RSA personalizado)
//! Y+0x145 revisión de master key (1-based)
//! Y+0x160 rights id (0x10)
//! ```

use crate::crypto;
use crate::{Error, KeySet, Result};

const BODY_MIN: usize = 0x180;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub signature_type: u32,
    /// Bytes de la firma tal cual, sin interpretar.
    pub signature: Vec<u8>,
    pub issuer: String,
    pub encrypted_titlekey: [u8; 16],
    pub title_key_type: u8,
    /// Revisión de master key **1-based** (la misma que declara la NCA como
    /// `crypto_type`). Para indexar `titlekek_NN` hay que restar uno.
    pub master_key_revision: u8,
    pub rights_id: [u8; 16],
}

impl Ticket {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 0x40 {
            return Err(Error::Invalid("ticket demasiado corto".into()));
        }

        let (signature_type, _big_endian) = decode_signature_type(data)?;
        let (sig_len, pad) = match signature_type {
            0x010000 => (0x200usize, 0x3Cusize),
            0x010001 => (0x100, 0x3C),
            0x010002 => (0x3C, 0x40),
            0x010003 => (0x200, 0x3C),
            0x010004 => (0x100, 0x3C),
            0x010005 => (0x3C, 0x40),
            0x010006 => (0x14, 0x28),
            other => {
                return Err(Error::Invalid(format!(
                    "ticket con tipo de firma desconocido: 0x{other:06X}"
                )))
            }
        };

        let y = 4 + sig_len + pad;
        if data.len() < y + BODY_MIN {
            return Err(Error::Invalid(format!(
                "ticket truncado: {} bytes, se esperaban al menos {}",
                data.len(),
                y + BODY_MIN
            )));
        }

        let body = &data[y..];
        let issuer = String::from_utf8_lossy(&body[0..0x40])
            .trim_end_matches('\0')
            .to_string();

        let mut encrypted_titlekey = [0u8; 16];
        encrypted_titlekey.copy_from_slice(&body[0x40..0x50]);

        let mut rights_id = [0u8; 16];
        rights_id.copy_from_slice(&body[0x160..0x170]);

        let signature = data[4..4 + sig_len].to_vec();

        Ok(Self {
            signature_type,
            signature,
            issuer,
            encrypted_titlekey,
            title_key_type: body[0x141],
            master_key_revision: body[0x145],
            rights_id,
        })
    }

    /// Revisión indexable en `titlekek_NN` (0-based).
    pub fn titlekek_revision(&self) -> u8 {
        revision_from_generation(self.master_key_revision)
    }

    /// Descifra el titlekey con `titlekek` y devuelve la clave de contenido.
    pub fn content_key(&self, keys: &KeySet) -> Result<[u8; 16]> {
        if self.title_key_type != 0 {
            return Err(Error::TicketPersonalized);
        }
        let revision = self.titlekek_revision();
        let kek = keys.titlekek(revision).ok_or(Error::MissingTitlekek {
            revision,
            declared: self.master_key_revision,
        })?;

        let mut key = self.encrypted_titlekey;
        crypto::ecb_decrypt_block(&kek, &mut key);
        Ok(key)
    }
}

/// `max(rev, 1) - 1`: 0 y 1 comparten `titlekek_00`.
pub fn revision_from_generation(generation: u8) -> u8 {
    if generation > 0 {
        generation - 1
    } else {
        0
    }
}

fn decode_signature_type(data: &[u8]) -> Result<(u32, bool)> {
    let raw = &data[0..4];
    let le = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let be = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let known = |v: u32| (0x010000..=0x010006).contains(&v);

    match (known(le), known(be)) {
        (true, _) => Ok((le, false)),
        (false, true) => Ok((be, true)),
        (false, false) => Err(Error::Invalid(format!(
            "tipo de firma de ticket no reconocido: 0x{le:08X} / 0x{be:08X}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(sig_type: u32, big_endian: bool) -> Vec<u8> {
        let mut out = vec![0u8; 0x2C0];
        let raw = if big_endian {
            sig_type.to_be_bytes()
        } else {
            sig_type.to_le_bytes()
        };
        out[0..4].copy_from_slice(&raw);
        // Issuer en Y = 0x140.
        let issuer = b"Root-CA00000003-XS00000020";
        out[0x140..0x140 + issuer.len()].copy_from_slice(issuer);
        // Titlekey cifrado.
        for (i, b) in out[0x180..0x190].iter_mut().enumerate() {
            *b = i as u8;
        }
        out[0x140 + 0x141] = 0; // AES
        out[0x140 + 0x145] = 10;
        for i in 0..16 {
            out[0x140 + 0x160 + i] = i as u8 + 1;
        }
        out
    }

    #[test]
    fn parses_le_signature_type() {
        let t = Ticket::parse(&build(0x010004, false)).unwrap();
        assert_eq!(t.signature_type, 0x010004);
        assert_eq!(t.issuer, "Root-CA00000003-XS00000020");
        assert_eq!(t.title_key_type, 0);
        assert_eq!(t.master_key_revision, 10);
        assert_eq!(t.titlekek_revision(), 9);
        assert_eq!(t.encrypted_titlekey[0], 0);
        assert_eq!(t.encrypted_titlekey[15], 15);
        assert_eq!(t.rights_id[0], 1);
    }

    #[test]
    fn parses_be_signature_type_as_fallback() {
        // Ticket sintético con cuerpo desplazado a Y = 0x240 (RSA-4096).
        let mut data = vec![0u8; 0x3C0];
        data[0..4].copy_from_slice(&0x010000u32.to_be_bytes());
        let issuer = b"Root-CA00000003-XS00000020";
        data[0x240..0x240 + issuer.len()].copy_from_slice(issuer);
        let t = Ticket::parse(&data).unwrap();
        assert_eq!(t.signature_type, 0x010000);
        assert_eq!(t.issuer, "Root-CA00000003-XS00000020");
    }

    #[test]
    fn rejects_unknown_signature_type() {
        let mut data = build(0x010004, false);
        data[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        assert!(Ticket::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_ticket() {
        assert!(Ticket::parse(&[0u8; 0x20]).is_err());
        let data = build(0x010004, false);
        assert!(Ticket::parse(&data[..0x200]).is_err());
    }

    #[test]
    fn personalized_ticket_is_rejected() {
        let mut data = build(0x010004, false);
        data[0x140 + 0x141] = 1;
        let t = Ticket::parse(&data).unwrap();
        let err = t.content_key(&KeySet::default()).unwrap_err();
        assert_eq!(err.code(), "E_TICKET_PERSONALIZED");
    }

    #[test]
    fn missing_titlekek_is_reported_with_revision() {
        let data = build(0x010004, false);
        let t = Ticket::parse(&data).unwrap();
        let err = t.content_key(&KeySet::default()).unwrap_err();
        assert_eq!(err.code(), "E_KEY_TITLEKEK_MISSING");
        assert!(err.to_string().contains("titlekek_09"));
    }
}
