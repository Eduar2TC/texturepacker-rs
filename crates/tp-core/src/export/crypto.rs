use super::options::EncodeOptions;
use super::pixel::apply_pixel_format;
use super::png::encode_png;
use crate::config::PixelFormat;
use crate::error::{Result, TpError};

// ---------------------------------------------------------------------------
// Encryption (AES-256-GCM)
// ---------------------------------------------------------------------------

/// Header magic for encrypted texture files (legacy).
pub const ENC_MAGIC: &[u8; 6] = b"TPENC1";

/// Header magic of the current format: `TPENC2` + salt + nonce + ciphertext.
pub const ENC_MAGIC_V2: &[u8; 6] = b"TPENC2";

/// Salt length of `TPENC2`, in bytes.
const SAL_LEN: usize = 16;
/// AES-GCM nonce length, in bytes.
const NONCE_LEN: usize = 12;
/// Argon2id parameters: 19 MiB, 2 passes, single thread — exactly the
/// figures OWASP recommends for Argon2id. Measured at ~46 ms per
/// derivation on a modern core, which is what stops an offline dictionary
/// attack: SHA-256 alone handed out ~10⁹ guesses per second per GPU,
/// while this leaves an attacker with ~20 guesses per second per core.
const ARGON2_MEMORY_KIB: u32 = 19 * 1024;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;

/// Derive a 32-byte AES key from a passphrase (SHA-256).
///
/// **Legacy only**: this is what `TPENC1` files were encrypted with, and it
/// is kept solely so they keep opening. New files go through
/// [`derive_key_v2`], which adds a per-file salt and a KDF.
pub fn derive_key(passphrase: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(passphrase.as_bytes());
    hasher.finalize().into()
}

/// Derive a 32-byte AES key with Argon2id and a per-file salt.
fn derive_key_v2(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    use argon2::{Algorithm, Argon2, Params, Version};

    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(32),
    )
    .map_err(|e| TpError::Other(format!("Parámetros de Argon2 inválidos: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| TpError::Other(format!("Error derivando la clave: {e}")))?;
    Ok(key)
}

/// Encrypt `plaintext` with AES-256-GCM. Output layout:
/// `TPENC2` (6) + salt (16) + nonce (12) + ciphertext+tag.
///
/// Every file gets its own random salt, so the same passphrase derives a
/// different key for each one: files can't be tied to each other and a
/// wrong guess has to pay the full Argon2 cost again for each file.
pub fn encrypt_bytes(plaintext: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use rand::RngCore;

    let mut salt = [0u8; SAL_LEN];
    rand::rng().fill_bytes(&mut salt);
    let key = derive_key_v2(passphrase, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| TpError::Other(format!("Clave inválida: {e}")))?;
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);

    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|e| TpError::Other(format!("Error cifrando: {e}")))?;

    let mut out = Vec::with_capacity(6 + SAL_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(ENC_MAGIC_V2);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Split an encrypted file by its header, deriving the key that belongs to
/// that format. Returns `(nonce, ciphertext, key)`.
fn abrir_cifrado<'a>(data: &'a [u8], passphrase: &str) -> Result<(&'a [u8], &'a [u8], [u8; 32])> {
    if data.len() < 6 + NONCE_LEN {
        return Err(TpError::Other(
            "Archivo cifrado demasiado corto".to_string(),
        ));
    }
    if &data[..6] == ENC_MAGIC_V2 {
        // `TPENC2` (6) + salt (16) + nonce (12) + ciphertext+tag.
        let (sal, resto) = data[6..].split_at(SAL_LEN);
        if resto.len() < NONCE_LEN {
            return Err(TpError::Other(
                "Archivo cifrado demasiado corto".to_string(),
            ));
        }
        let key = derive_key_v2(passphrase, sal)?;
        Ok((&resto[..NONCE_LEN], &resto[NONCE_LEN..], key))
    } else if &data[..6] == ENC_MAGIC {
        // `TPENC1` (6) + nonce (12) + ciphertext+tag, sin sal ni KDF.
        let key = derive_key(passphrase);
        Ok((&data[6..6 + NONCE_LEN], &data[6 + NONCE_LEN..], key))
    } else {
        Err(TpError::Other(
            "No es un archivo cifrado TexturePacker-RS (falta cabecera TPENC1 o TPENC2)"
                .to_string(),
        ))
    }
}

/// Decrypt a file produced by [`encrypt_bytes`] (or by an older release,
/// in the `TPENC1` format). Returns the plaintext.
pub fn decrypt_bytes(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    let (nonce, ct, key) = abrir_cifrado(data, passphrase)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| TpError::Other(format!("Clave inválida: {e}")))?;
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| TpError::Other("Error descifrando (¿clave incorrecta?)".to_string()))
}

/// Decode a decrypted texture file into a viewable PNG for preview.
///
/// The atlas files generated with a `pixel_format` other than `RGBA8888`
/// store the channels in that same order (p. ej. `BGRA8888` swaps R and B),
/// so a normal viewer would show the colors wrong. This helper decodes the
/// image, re-applies [`apply_pixel_format`] — which restores the natural RGBA8
/// order for the involutive formats (channel swaps and bit-replicated
/// quantizations) — and re-encodes a regular RGBA8 PNG.
///
/// The grayscale formats (`Alpha8`, `Intensity8`, `AlphaIntensity8`) are the
/// exception: their file already stores exactly the visible channel, and the
/// conversion is not involutive (re-applying it would alter the values), so
/// the decoded image is re-encoded as-is.
pub fn decode_texture_preview_png(data: &[u8], pixel_format: PixelFormat) -> Result<Vec<u8>> {
    let img = image::load_from_memory(data)?;
    let (width, height) = (img.width() as usize, img.height() as usize);

    let needs_conversion = !matches!(
        pixel_format,
        PixelFormat::Alpha8 | PixelFormat::Intensity8 | PixelFormat::AlphaIntensity8
    );
    let rgba = if needs_conversion {
        apply_pixel_format(img.to_rgba8().as_raw(), pixel_format).0
    } else {
        img.to_rgba8().into_raw()
    };

    encode_png(
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
        &EncodeOptions::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GpuFormat;
    use crate::export::encode_to_bytes;
    use crate::export::test_helpers::opts;

    #[test]
    fn encryption_roundtrip() {
        let plain = b"hello texture world".to_vec();
        let enc = encrypt_bytes(&plain, "secret123").unwrap();
        assert_eq!(&enc[..6], ENC_MAGIC_V2);
        assert_eq!(enc.len(), 6 + SAL_LEN + NONCE_LEN + plain.len() + 16);
        let dec = decrypt_bytes(&enc, "secret123").unwrap();
        assert_eq!(dec, plain);
        assert!(decrypt_bytes(&enc, "wrong").is_err());
        assert!(decrypt_bytes(b"garbage", "x").is_err());
    }

    /// Los ficheros `TPENC1` de las versiones anteriores siguen
    /// descifrando. Se reconstruyen aquí con la derivación de entonces
    /// (SHA-256 pelón, sin sal) para que la rama legacy no pueda caerse
    /// sin que este test lo note.
    #[test]
    fn los_ficheros_tpenc1_seguen_descifrando() {
        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};
        use sha2::{Digest, Sha256};

        let plain = b"atlas de una version anterior".to_vec();
        let nonce = [3u8; 12];
        let key: [u8; 32] = Sha256::digest("secret123".as_bytes()).into();
        let ct = Aes256Gcm::new_from_slice(&key)
            .unwrap()
            .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
            .unwrap();
        let mut legacy = ENC_MAGIC.to_vec();
        legacy.extend_from_slice(&nonce);
        legacy.extend_from_slice(&ct);

        assert_eq!(decrypt_bytes(&legacy, "secret123").unwrap(), plain);
        assert!(decrypt_bytes(&legacy, "otra-clave").is_err());
    }

    /// Cada fichero lleva su sal: misma passphrase, claves distintas, y
    /// dos cifrados del mismo contenido no se parecen entre sí, así que
    /// un atacante ni siquiera puede comprobar si dos ficheros salieron
    /// de la misma contraseña.
    #[test]
    fn cada_fichero_tiene_su_sal() {
        let plain = b"mismo contenido".to_vec();
        let a = encrypt_bytes(&plain, "secret123").unwrap();
        let b = encrypt_bytes(&plain, "secret123").unwrap();
        assert_ne!(a, b);
        assert_ne!(
            &a[6..6 + SAL_LEN],
            &b[6..6 + SAL_LEN],
            "la sal debe ser aleatoria por fichero"
        );
    }

    /// La clave ya no es un SHA-256 del passphrase: sin coste ni sal, un
    /// diccionario por GPU probaba passphrases a razón de ~10⁹/s.
    #[test]
    fn la_clave_v2_no_es_un_sha256_del_passphrase() {
        use sha2::{Digest, Sha256};

        let con_sal_1 = derive_key_v2("secret123", &[1u8; SAL_LEN]).unwrap();
        let con_sal_2 = derive_key_v2("secret123", &[2u8; SAL_LEN]).unwrap();
        let sin_kdf: [u8; 32] = Sha256::digest("secret123".as_bytes()).into();

        assert_ne!(
            con_sal_1, sin_kdf,
            "la derivación ha vuelto a ser SHA-256 puro"
        );
        assert_ne!(con_sal_1, con_sal_2, "la sal no separa claves");
        assert_ne!(
            derive_key_v2("otra-clave", &[1u8; SAL_LEN]).unwrap(),
            con_sal_1
        );
    }

    #[test]
    fn decrypt_preview_restores_bgra_channels() {
        // Atlas publicado con pixel_format BGRA8888: el archivo lleva R y B
        // intercambiados; el preview debe devolver el orden natural RGBA.
        let bgra = vec![255u8, 0, 0, 255, 0, 0, 255, 255]; // 2 px: azul, rojo
        let png = encode_to_bytes(&bgra, 2, 1, &opts(GpuFormat::Png)).unwrap();
        let enc = encrypt_bytes(&png, "clave").unwrap();
        let plain = decrypt_bytes(&enc, "clave").unwrap();

        let preview = decode_texture_preview_png(&plain, PixelFormat::Bgra8888).unwrap();
        let rgba = image::load_from_memory(&preview).unwrap().to_rgba8();
        assert_eq!(rgba.as_raw(), &[0, 0, 255, 255, 255, 0, 0, 255]);

        // Sin la conversión, los canales seguirían intercambiados.
        let sin_conversion = image::load_from_memory(&plain).unwrap().to_rgba8();
        assert_ne!(rgba.as_raw(), sin_conversion.as_raw());
    }

    #[test]
    fn decrypt_preview_keeps_alpha8_as_grayscale() {
        // Alpha8 no es involutivo: el archivo ya guarda el canal visible (el
        // nivel de alfa en escala de grises) y el preview lo respeta tal cual.
        let rgba = vec![10u8, 20, 30, 77, 40, 50, 60, 200]; // 2 px
        let png = encode_to_bytes(
            &rgba,
            2,
            1,
            &EncodeOptions {
                pixel_format: PixelFormat::Alpha8,
                ..opts(GpuFormat::Png)
            },
        )
        .unwrap();
        let preview = decode_texture_preview_png(&png, PixelFormat::Alpha8).unwrap();
        let img = image::load_from_memory(&preview).unwrap().to_rgba8();
        assert_eq!(img.as_raw(), &[77, 77, 77, 255, 200, 200, 200, 255]);
    }
}
