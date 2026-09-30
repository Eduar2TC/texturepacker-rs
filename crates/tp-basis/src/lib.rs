//! Compresor Basis Universal (ETC1S) con una API segura para el resto del
//! workspace.
//!
//! Es la **única excepción** a `unsafe_code = "forbid"`: el FFI de
//! `basis-universal` expone `Compressor::init` y `Compressor::process` como
//! `unsafe fn`, así que este pequeño crate concentra esas dos llamadas y todo
//! lo que sale de aquí son tipos y funciones seguros (`Vec<u8>`, tuplas y
//! `Result`).

use basis_universal::{
    BasisTextureFormat, Compressor, CompressorParams, TranscodeParameters, Transcoder,
    TranscoderTextureFormat,
};

/// Traduce una calidad 0-100 (la escala de los demás `*_quality` del proyecto)
/// al rango 1-255 de ETC1S; 50 ≈ la calidad por defecto de Basis (128).
pub fn etc1s_quality(quality: u8) -> u32 {
    let scaled = (u32::from(quality) * 255 + 50) / 100;
    scaled.clamp(
        basis_universal::ETC1S_QUALITY_MIN,
        basis_universal::ETC1S_QUALITY_MAX,
    )
}

/// Codifica una imagen RGBA8 como fichero `.basis` (ETC1S, con alfa si la hay).
///
/// `quality` va de 0 a 100 y se traduce con [`etc1s_quality`]. No genera
/// mipmaps: una hoja de atlas se publica en un solo nivel.
pub fn encode_etc1s(rgba: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("imagen 0×0".to_string());
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| "dimensiones desbordadas".to_string())?;
    if rgba.len() < expected {
        return Err(format!(
            "se esperaban {expected} bytes de RGBA y hay {}",
            rgba.len()
        ));
    }

    let threads = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
        .max(1);

    let mut params = CompressorParams::new();
    params.set_basis_format(BasisTextureFormat::ETC1S);
    params.set_etc1s_quality_level(etc1s_quality(quality));
    params.set_print_status_to_stdout(false);
    params.set_generate_mipmaps(false);
    {
        let mut image = params.source_image_mut(0);
        image.init(rgba, width, height, 4);
    }

    let mut compressor = Compressor::new(threads);
    // Las dos únicas llamadas `unsafe` del workspace: la C++ de Binomial no
    // valida los parámetros y promete UB si son incorrectos, por eso van aquí,
    // detrás de las comprobaciones de arriba.
    let configured = unsafe { compressor.init(&params) };
    if !configured {
        return Err("Basis rechazó los parámetros del compresor".to_string());
    }
    unsafe { compressor.process() }.map_err(|e| format!("Basis: {e:?}"))?;

    let file = compressor.basis_file().to_vec();
    if file.is_empty() {
        return Err("Basis no produjo datos".to_string());
    }
    Ok(file)
}

/// Transcodifica un `.basis` de vuelta a RGBA8 (primer nivel, primera imagen)
/// para verificar round-trips en los tests.
pub fn transcode_rgba(basis: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut transcoder = Transcoder::new();
    if transcoder.image_level_count(basis, 0) == 0 {
        return Err("el fichero .basis no tiene niveles".to_string());
    }
    let description = transcoder
        .image_level_description(basis, 0, 0)
        .ok_or_else(|| "descripción del nivel no disponible".to_string())?;
    transcoder
        .prepare_transcoding(basis)
        .map_err(|_| "no se pudo preparar la transcodificación".to_string())?;
    let rgba = transcoder
        .transcode_image_level(
            basis,
            TranscoderTextureFormat::RGBA32,
            TranscodeParameters {
                image_index: 0,
                level_index: 0,
                ..Default::default()
            },
        )
        .map_err(|e| format!("transcodificación: {e:?}"))?;
    transcoder.end_transcoding();
    Ok((
        description.original_width,
        description.original_height,
        rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cuatro cuadrantes de color de 16×16 (32×32 en total), el bloque de 4×4
    /// de ETC1S cae entero dentro de cada cuadrante.
    fn quadrants() -> (Vec<u8>, u32, u32) {
        let (w, h) = (32, 32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c: [u8; 4] = match (x < 16, y < 16) {
                    (true, true) => [255, 0, 0, 255],
                    (false, true) => [0, 255, 0, 255],
                    (true, false) => [0, 0, 255, 255],
                    (false, false) => [255, 255, 0, 128],
                };
                rgba.extend_from_slice(&c);
            }
        }
        (rgba, w, h)
    }

    #[test]
    fn etc1s_quality_scales_the_project_range_to_basis() {
        assert_eq!(etc1s_quality(0), 1);
        assert_eq!(etc1s_quality(50), 128);
        assert_eq!(etc1s_quality(100), 255);
    }

    #[test]
    fn encode_etc1s_roundtrips_through_the_transcoder() {
        let (rgba, w, h) = quadrants();
        let basis = encode_etc1s(&rgba, w, h, 80).unwrap();
        // Firma de 2 bytes `sB` (cBASISSigValue = ('B' << 8) | 's').
        assert_eq!(&basis[..2], b"sB", "firma de .basis");

        let (tw, th, out) = transcode_rgba(&basis).unwrap();
        assert_eq!((tw, th), (w, h));
        assert_eq!(out.len(), (w * h * 4) as usize);

        let at = |x: u32, y: u32| -> [u8; 4] {
            let i = ((y * w + x) * 4) as usize;
            [out[i], out[i + 1], out[i + 2], out[i + 3]]
        };
        let close = |got: [u8; 4], want: [u8; 4]| {
            for c in 0..4 {
                assert!(
                    i32::from(got[c]) - i32::from(want[c]) <= 40
                        && i32::from(want[c]) - i32::from(got[c]) <= 40,
                    "canal {c}: {got:?} frente a {want:?}"
                );
            }
        };
        close(at(8, 8), [255, 0, 0, 255]);
        close(at(24, 8), [0, 255, 0, 255]);
        close(at(8, 24), [0, 0, 255, 255]);
        close(at(24, 24), [255, 255, 0, 128]);
    }

    #[test]
    fn encode_reports_bad_input() {
        let err = encode_etc1s(&[0u8; 3], 4, 4, 50).unwrap_err();
        assert!(err.contains("64"), "{err}");
        assert!(encode_etc1s(&[], 0, 0, 50).is_err());
    }
}
