//! Subsystem 5: Exportación & Cifrado.
//!
//! - Texture encoding: PNG con paleta automática de 8 bits (*Png Opt
//!   Level*), PNG-8 con cuantización y dithering (*Dithering* PngQuant),
//!   JPG y WebP lossy (*Image quality*), WebP sin pérdidas, ASTC 4x4 (via
//!   ARM astcenc when the `gpu-formats` feature is enabled), ETC2 RGBA8,
//!   PVRTC1 4bpp, KTX2 sin comprimir y Basis Universal (con la feature
//!   `gpu-formats`) (built-in encoders)
//! - Conversión de formato de píxel (*Pixel format*) y volteo vertical
//!   (*flip-y*, solo formatos de hardware)
//! - AES-256-GCM symmetric encryption of generated image files
//! - Image scaling for @2x/@1x style variants

mod astc;
mod basis;
mod crypto;
mod dds;
mod encode;
mod etc;
mod image_formats;
mod jpg;
mod ktx;
mod ops;
mod options;
mod pixel;
mod png;
mod pvr;
mod scale;
mod webp;

#[cfg(test)]
mod test_helpers;

pub use crypto::{decode_texture_preview_png, decrypt_bytes, derive_key, encrypt_bytes, ENC_MAGIC};
pub use encode::encode_to_bytes;
pub use ops::{apply_dpi, draw_shape_debug, flip_vertical_rgba, DebugShape};
pub use options::EncodeOptions;
pub use scale::scale_rgba;
