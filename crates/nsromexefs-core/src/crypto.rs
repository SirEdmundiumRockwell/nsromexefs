//! Primitivas AES que usan los contenedores de Switch.
//!
//! - **ECB** para descifrar titlekeys y el *key area*.
//! - **XTS** para la cabecera NCA. Nintendo usa un tweak propio: el número de
//!   sector en big-endian ocupa los bytes 8..16 (no el estándar IEEE 1619, que
//!   usa little-endian en los bytes 0..8). Ver `hactool/aes.c::get_tweak`.
//! - **CTR** para las secciones, con contador big-endian de 128 bits.
//!
//! La lógica de XTS está portada de `hactool` (ISC, SciresM); ver
//! `THIRD-PARTY-NOTICES.md`.

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use ctr::Ctr128BE;

use crate::{Error, Result};

pub type Aes128Ctr = Ctr128BE<Aes128>;

fn new_aes(key: &[u8; 16]) -> Aes128 {
    Aes128::new(key.into())
}

/// AES-128-ECB: descifra un bloque de 16 bytes en sitio.
pub fn ecb_decrypt_block(key: &[u8; 16], block: &mut [u8; 16]) {
    let cipher = new_aes(key);
    cipher.decrypt_block(aes::Block::from_mut_slice(block));
}

/// AES-128-ECB: cifra un bloque de 16 bytes en sitio.
pub fn ecb_encrypt_block(key: &[u8; 16], block: &mut [u8; 16]) {
    let cipher = new_aes(key);
    cipher.encrypt_block(aes::Block::from_mut_slice(block));
}

/// AES-128-ECB sobre un buffer completo (múltiplo de 16).
pub fn ecb_decrypt(key: &[u8; 16], data: &mut [u8]) -> Result<()> {
    if data.len() % 16 != 0 {
        return Err(Error::Invalid(
            "AES-ECB requiere un múltiplo de 16 bytes".into(),
        ));
    }
    let cipher = new_aes(key);
    for chunk in data.chunks_mut(16) {
        cipher.decrypt_block(aes::Block::from_mut_slice(chunk));
    }
    Ok(())
}

/// AES-128-CTR con IV explícito, de una sola pasada.
pub fn ctr_apply(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8]) {
    let mut cipher = Aes128Ctr::new(key.into(), iv.into());
    cipher.apply_keystream(data);
}

/// AES-128-CTR posicionado en `byte_offset` dentro del flujo.
pub fn ctr_apply_at(key: &[u8; 16], iv: &[u8; 16], byte_offset: u64, data: &mut [u8]) {
    let mut cipher = Aes128Ctr::new(key.into(), iv.into());
    cipher.seek(byte_offset);
    cipher.apply_keystream(data);
}

/// Multiplicación por α en GF(2^128) con el polinomio 0x87 (XTS).
fn mul_alpha(t: &mut [u8; 16]) {
    let mut carry = 0u8;
    for byte in t.iter_mut() {
        let next = *byte >> 7;
        *byte = (*byte << 1) | carry;
        carry = next;
    }
    if carry != 0 {
        t[0] ^= 0x87;
    }
}

/// Descifra un área AES-128-XTS con sectores de `sector_size` bytes.
///
/// `first_sector` es el índice del primer sector (offset / 0x200) y se codifica
/// en big-endian en los bytes 8..16 del tweak, como hace la consola.
pub fn xts_decrypt(key: &[u8; 32], data: &mut [u8], first_sector: u64, sector_size: usize) {
    xts_process(key, data, first_sector, sector_size, false);
}

/// Cifra un área AES-128-XTS (solo para tests y herramientas de depuración).
pub fn xts_encrypt(key: &[u8; 32], data: &mut [u8], first_sector: u64, sector_size: usize) {
    xts_process(key, data, first_sector, sector_size, true);
}

fn xts_process(
    key: &[u8; 32],
    data: &mut [u8],
    first_sector: u64,
    sector_size: usize,
    encrypt: bool,
) {
    debug_assert!(sector_size > 0 && sector_size % 16 == 0);

    let mut data_key = [0u8; 16];
    let mut tweak_key = [0u8; 16];
    data_key.copy_from_slice(&key[0..16]);
    tweak_key.copy_from_slice(&key[16..32]);

    let data_cipher = new_aes(&data_key);
    let tweak_cipher = new_aes(&tweak_key);

    for (i, sector) in data.chunks_mut(sector_size).enumerate() {
        let mut tweak = [0u8; 16];
        tweak[8..16].copy_from_slice(&(first_sector + i as u64).to_be_bytes());

        // T = AES_enc(k2, tweak)
        tweak_cipher.encrypt_block(aes::Block::from_mut_slice(&mut tweak));

        for chunk in sector.chunks_mut(16) {
            let mut tmp = [0u8; 16];
            for j in 0..16 {
                tmp[j] = chunk[j] ^ tweak[j];
            }
            if encrypt {
                data_cipher.encrypt_block(aes::Block::from_mut_slice(&mut tmp));
            } else {
                data_cipher.decrypt_block(aes::Block::from_mut_slice(&mut tmp));
            }
            for j in 0..16 {
                chunk[j] = tmp[j] ^ tweak[j];
            }
            mul_alpha(&mut tweak);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY1: [u8; 16] = [
        0x27, 0x18, 0x28, 0x18, 0x28, 0x45, 0x90, 0x45, 0x23, 0x53, 0x60, 0x28, 0x74, 0x71, 0x35,
        0x26,
    ];
    const KEY2: [u8; 16] = [
        0x31, 0x41, 0x59, 0x26, 0x53, 0x58, 0x97, 0x93, 0x23, 0x84, 0x62, 0x64, 0x33, 0x83, 0x27,
        0x95,
    ];

    #[test]
    fn xts_roundtrip_is_inverse() {
        let mut key = [0u8; 32];
        key[..16].copy_from_slice(&KEY1);
        key[16..].copy_from_slice(&KEY2);

        let original: Vec<u8> = (0..0x400).map(|i| (i % 251) as u8).collect();
        let mut buf = original.clone();

        xts_encrypt(&key, &mut buf, 0, 0x200);
        assert_ne!(buf, original, "el cifrado debe cambiar los datos");

        xts_decrypt(&key, &mut buf, 0, 0x200);
        assert_eq!(buf, original);
    }

    #[test]
    fn xts_sectors_use_different_tweaks() {
        let mut key = [0u8; 32];
        key[..16].copy_from_slice(&KEY1);
        key[16..].copy_from_slice(&KEY2);

        let zeros = [0u8; 0x400];
        let mut buf = zeros;
        xts_encrypt(&key, &mut buf, 0, 0x200);
        assert_ne!(&buf[0..0x200], &buf[0x200..0x400]);
    }

    #[test]
    fn ctr_matches_nist_vector() {
        // SP 800-38A, F.5.1 (AES-128-CTR), primer bloque.
        let key = [
            0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf,
            0x4f, 0x3c,
        ];
        let iv = [
            0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd,
            0xfe, 0xff,
        ];
        let mut data = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a,
        ];
        ctr_apply(&key, &iv, &mut data);
        assert_eq!(
            data,
            [
                0x87, 0x4d, 0x61, 0x91, 0xb6, 0x20, 0xe3, 0x26, 0x1b, 0xef, 0x68, 0x64, 0x99, 0x0d,
                0xb6, 0xce,
            ]
        );
    }

    #[test]
    fn ecb_matches_fips_vector() {
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let mut block = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        ecb_encrypt_block(&key, &mut block);
        assert_eq!(
            block,
            [
                0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4,
                0xc5, 0x5a,
            ]
        );
    }

    #[test]
    fn ecb_roundtrip() {
        let mut block = [0x11u8; 16];
        ecb_encrypt_block(&KEY1, &mut block);
        assert_ne!(block, [0x11u8; 16]);
        ecb_decrypt_block(&KEY1, &mut block);
        assert_eq!(block, [0x11u8; 16]);
    }

    #[test]
    fn ctr_seek_matches_continuous_stream() {
        let mut data: Vec<u8> = (0..64).map(|i| i as u8).collect();
        let iv = [0u8; 16];

        let mut whole = data.clone();
        ctr_apply(&KEY1, &iv, &mut whole);

        ctr_apply_at(&KEY1, &iv, 0, &mut data[..16]);
        ctr_apply_at(&KEY1, &iv, 16, &mut data[16..48]);
        ctr_apply_at(&KEY1, &iv, 48, &mut data[48..]);

        assert_eq!(data, whole);
    }

    #[test]
    fn ctr_seek_requires_alignment_only_for_partial() {
        let mut data = [0u8; 16];
        // ctr 0.9 admite offsets no alineados; el resultado debe ser el mismo
        // que aplicar desde cero y descartar los primeros bytes.
        ctr_apply_at(&KEY1, &[0u8; 16], 3, &mut data);
        let mut reference = [0u8; 32];
        ctr_apply(&KEY1, &[0u8; 16], &mut reference);
        assert_eq!(data, reference[3..19]);
    }

    #[test]
    fn ecb_rejects_bad_length() {
        assert!(ecb_decrypt(&KEY1, &mut [0u8; 17]).is_err());
    }
}
