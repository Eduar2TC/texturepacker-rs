//! Subsystem 5: Exportación & Cifrado.
//!
//! - Texture encoding: PNG (lossless), WebP (lossless), ASTC 4x4 (via ARM
//!   astcenc when the `gpu-formats` feature is enabled), ETC2 RGBA8 and
//!   PVRTC1 4bpp (built-in encoders)
//! - AES-256-GCM symmetric encryption of generated image files
//! - Image scaling for @2x/@1x style variants

use crate::config::GpuFormat;
use crate::etc2;
use crate::pvrtc;
use std::io::Cursor;

/// Encode an RGBA8 image into the *file* bytes of the requested format.
pub fn encode_to_bytes(
    rgba: &[u8],
    width: usize,
    height: usize,
    format: GpuFormat,
) -> Result<Vec<u8>, String> {
    match format {
        GpuFormat::Png => encode_png(rgba, width, height),
        GpuFormat::WebP => encode_webp_lossless(rgba, width, height),
        GpuFormat::Astc4x4 => encode_astc(rgba, width, height),
        GpuFormat::Etc2Rgba => Ok(encode_etc2_ktx(rgba, width, height)),
        GpuFormat::Pvrtc4Bpp => encode_pvrtc_pvr(rgba, width, height),
    }
}

fn encode_png(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec())
        .ok_or_else(|| "Dimensiones de imagen inválidas para PNG".to_string())?;
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("Error codificando PNG: {e}"))?;
    Ok(buf)
}

fn encode_webp_lossless(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
        .encode(
            rgba,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("Error codificando WebP: {e}"))?;
    Ok(buf)
}

/// ASTC 4x4: ARM astcenc + `.astc` container.
fn encode_astc(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    #[cfg(feature = "gpu-formats")]
    {
        use astcenc_rs::{
            ConfigBuilder, Context, Extents, Image, Profile, Swizzle, PRESET_MEDIUM,
        };
        let cfg = ConfigBuilder::new()
            .with_block_size(Extents::new(4, 4))
            .with_preset(PRESET_MEDIUM)
            .with_profile(Profile::LdrRgba)
            .build()
            .map_err(|e| format!("Config ASTC inválida: {e:?}"))?;
        let mut ctx = Context::new(cfg).map_err(|e| format!("Contexto ASTC: {e:?}"))?;
        let img = Image {
            extents: Extents::new(width as u32, height as u32),
            data: &[rgba][..],
        };
        let blocks = ctx
            .compress(&img, Swizzle::rgba())
            .map_err(|e| format!("Error comprimiendo ASTC: {e:?}"))?;

        // .astc header (16 bytes): magic, block dims, x/y/z size (24-bit LE).
        let mut out = Vec::with_capacity(16 + blocks.len());
        out.extend_from_slice(&[0x13, 0xAB, 0xA1, 0x5C]); // magic 0x5CA1AB13
        out.push(4); // block_x
        out.push(4); // block_y
        out.push(1); // block_z
        out.extend_from_slice(&(width as u32).to_le_bytes()[..3]);
        out.extend_from_slice(&(height as u32).to_le_bytes()[..3]);
        out.extend_from_slice(&1u32.to_le_bytes()[..3]); // zsize
        out.extend_from_slice(&blocks);
        Ok(out)
    }
    #[cfg(not(feature = "gpu-formats"))]
    {
        let _ = (rgba, width, height);
        Err(
            "ASTC_4x4 requiere compilar con la feature `gpu-formats` \
             (cargo build --features gpu-formats)"
                .to_string(),
        )
    }
}

/// PVRTC1 4bpp in a PVR v3 container (`.pvr`).
fn encode_pvrtc_pvr(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let blocks = pvrtc::encode_pvrtc_4bpp(rgba, width, height)?;
    let mut out = Vec::with_capacity(52 + blocks.len());
    // PVR v3 header (52 bytes), all little-endian.
    out.extend_from_slice(b"PVR\x03"); // version
    out.extend_from_slice(&0u32.to_le_bytes()); // flags
    out.extend_from_slice(&0u64.to_le_bytes()); // pixel_format: PVRTC1 4bpp RGBA
    out.extend_from_slice(&0u32.to_le_bytes()); // colour_space: linearRGB
    out.extend_from_slice(&0u32.to_le_bytes()); // channel_type: unsigned byte
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // depth
    out.extend_from_slice(&1u32.to_le_bytes()); // num_surfaces
    out.extend_from_slice(&1u32.to_le_bytes()); // num_faces
    out.extend_from_slice(&1u32.to_le_bytes()); // mip_map_count
    out.extend_from_slice(&0u32.to_le_bytes()); // meta_data_size
    out.extend_from_slice(&blocks);
    Ok(out)
}

/// ETC2 RGBA8 in a KTX container.
fn encode_etc2_ktx(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let blocks = etc2::encode_etc2_rgba8(rgba, width, height);
    let mut out = Vec::with_capacity(64 + 4 + blocks.len());
    // KTX v1.1 header (64 bytes).
    out.extend_from_slice(b"\xABKTX 11\xBB\r\n\x1A\n");
    out.extend_from_slice(&0x04030201u32.to_le_bytes()); // endianness marker
    out.extend_from_slice(&0u32.to_le_bytes()); // glType
    out.extend_from_slice(&1u32.to_le_bytes()); // glTypeSize
    out.extend_from_slice(&0u32.to_le_bytes()); // glFormat
    out.extend_from_slice(&0x9278u32.to_le_bytes()); // GL_COMPRESSED_RGBA8_ETC2_EAC
    out.extend_from_slice(&0x1908u32.to_le_bytes()); // GL_RGBA
    out.extend_from_slice(&(width as u32).to_le_bytes()); // pixelWidth
    out.extend_from_slice(&(height as u32).to_le_bytes()); // pixelHeight
    out.extend_from_slice(&0u32.to_le_bytes()); // pixelDepth
    out.extend_from_slice(&0u32.to_le_bytes()); // numberOfArrayElements
    out.extend_from_slice(&1u32.to_le_bytes()); // numberOfFaces
    out.extend_from_slice(&1u32.to_le_bytes()); // numberOfMipmapLevels
    out.extend_from_slice(&0u32.to_le_bytes()); // bytesOfKeyValueData
    out.extend_from_slice(&(blocks.len() as u32).to_le_bytes()); // imageSize
    out.extend_from_slice(&blocks);
    out
}

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
pub fn encrypt_bytes(plaintext: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use rand::RngCore;

    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Clave inválida: {e}"))?;
    let mut nonce = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce);

    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|e| format!("Error cifrando: {e}"))?;

    let mut out = Vec::with_capacity(6 + 12 + ct.len());
    out.extend_from_slice(ENC_MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a file produced by [`encrypt_bytes`]. Returns the plaintext.
pub fn decrypt_bytes(data: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    if data.len() < 6 + 12 {
        return Err("Archivo cifrado demasiado corto".into());
    }
    if &data[..6] != ENC_MAGIC {
        return Err("No es un archivo cifrado TexturePacker-RS (falta cabecera TPENC1)".into());
    }
    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Clave inválida: {e}"))?;
    let nonce = &data[6..18];
    let ct = &data[18..];
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "Error descifrando (¿clave incorrecta?)".to_string())
}

// ---------------------------------------------------------------------------
// Scaling (variants)
// ---------------------------------------------------------------------------

/// Scale an RGBA8 image by `factor` (0 < factor <= 1) with bilinear sampling.
pub fn scale_rgba(
    rgba: &[u8],
    width: usize,
    height: usize,
    factor: f32,
) -> (Vec<u8>, usize, usize) {
    let nw = ((width as f32) * factor).round().max(1.0) as usize;
    let nh = ((height as f32) * factor).round().max(1.0) as usize;
    let mut out = vec![0u8; nw * nh * 4];

    for y in 0..nh {
        let sy = (y as f32 + 0.5) / factor - 0.5;
        for x in 0..nw {
            let sx = (x as f32 + 0.5) / factor - 0.5;
            let (x0, y0) = (sx.floor().max(0.0) as usize, sy.floor().max(0.0) as usize);
            let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;
            for c in 0..4 {
                let p00 = rgba[(y0 * width + x0) * 4 + c] as f32;
                let p10 = rgba[(y0 * width + x1) * 4 + c] as f32;
                let p01 = rgba[(y1 * width + x0) * 4 + c] as f32;
                let p11 = rgba[(y1 * width + x1) * 4 + c] as f32;
                let top = p00 * (1.0 - fx) + p10 * fx;
                let bot = p01 * (1.0 - fx) + p11 * fx;
                out[(y * nw + x) * 4 + c] = (top * (1.0 - fy) + bot * fy).round() as u8;
            }
        }
    }
    (out, nw, nh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_roundtrip() {
        let w = 8;
        let h = 8;
        let mut rgba = vec![0u8; w * h * 4];
        for i in 0..(w * h) {
            rgba[i * 4] = (i % 256) as u8;
            rgba[i * 4 + 3] = 255;
        }
        let bytes = encode_to_bytes(&rgba, w, h, GpuFormat::Png).unwrap();
        assert!(bytes.len() > 8);
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!(img.width(), 8);
        assert_eq!(img.height(), 8);
    }

    #[test]
    fn etc2_ktx_header() {
        let bytes = encode_etc2_ktx(&vec![0u8; 8 * 8 * 4], 8, 8);
        assert_eq!(&bytes[..12], b"\xABKTX 11\xBB\r\n\x1A\n");
        // internal format at offset 28 (12 magic + 4*4 header fields)
        let internal = u32::from_le_bytes(bytes[28..32].try_into().unwrap());
        assert_eq!(internal, 0x9278);
        assert_eq!(bytes.len(), 64 + 4 + 4 * 16);
    }

    #[test]
    fn pvrtc_pvr_file() {
        let rgba = vec![255u8; 8 * 8 * 4];
        let bytes = encode_to_bytes(&rgba, 8, 8, GpuFormat::Pvrtc4Bpp).unwrap();
        assert_eq!(&bytes[..4], b"PVR\x03");
        assert_eq!(bytes.len(), 52 + 8 * 8 / 2);
        let payload = &bytes[52..];
        let mut buf = vec![0u32; 8 * 8];
        texture2ddecoder::decode_pvrtc_4bpp(payload, 8, 8, &mut buf).unwrap();
        // Non-power-of-two sizes error clearly (e.g. odd scale variants).
        assert!(encode_to_bytes(&rgba, 96, 96, GpuFormat::Pvrtc4Bpp).is_err());
    }

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
    fn scaling_halves_size() {
        let rgba = vec![255u8; 16 * 16 * 4];
        let (out, w, h) = scale_rgba(&rgba, 16, 16, 0.5);
        assert_eq!((w, h), (8, 8));
        assert!(out.iter().all(|&v| v == 255));
    }
}
