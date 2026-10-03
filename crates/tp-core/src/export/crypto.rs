use super::options::EncodeOptions;
use super::pixel::apply_pixel_format;
use super::png::encode_png;
use crate::config::PixelFormat;
use crate::error::{Result, TpError};

// ---------------------------------------------------------------------------
// Encryption (AES-256-GCM)
// ---------------------------------------------------------------------------

/// Header magic for encrypted texture files.
pub const ENC_MAGIC: &[u8; 6] = b"TPENC1";

/// Derive a 32-byte AES key from a passphrase (SHA-256).
pub fn derive_key(passphrase: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(passphrase.as_bytes());
    hasher.finalize().into()
}

/// Encrypt `plaintext` with AES-256-GCM. Output layout:
/// `TPENC1` (6) + nonce (12) + ciphertext+tag.
pub fn encrypt_bytes(plaintext: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use rand::RngCore;

    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| TpError::Other(format!("Clave inválida: {e}")))?;
    let mut nonce = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce);

    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|e| TpError::Other(format!("Error cifrando: {e}")))?;

    let mut out = Vec::with_capacity(6 + 12 + ct.len());
    out.extend_from_slice(ENC_MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a file produced by [`encrypt_bytes`]. Returns the plaintext.
pub fn decrypt_bytes(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    if data.len() < 6 + 12 {
        return Err(TpError::Other(
            "Archivo cifrado demasiado corto".to_string(),
        ));
    }
    if &data[..6] != ENC_MAGIC {
        return Err(TpError::Other(
            "No es un archivo cifrado TexturePacker-RS (falta cabecera TPENC1)".to_string(),
        ));
    }
    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| TpError::Other(format!("Clave inválida: {e}")))?;
    let nonce = &data[6..18];
    let ct = &data[18..];
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
        assert_eq!(&enc[..6], ENC_MAGIC);
        let dec = decrypt_bytes(&enc, "secret123").unwrap();
        assert_eq!(dec, plain);
        assert!(decrypt_bytes(&enc, "wrong").is_err());
        assert!(decrypt_bytes(b"garbage", "x").is_err());
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
