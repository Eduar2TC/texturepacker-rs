use super::options::EncodeOptions;
use super::pixel::apply_pixel_format;

use crate::error::{Result, TpError};

/// KTX v1.1 container with a single mip level. `gl_type`/`gl_format` are 0
/// for compressed payloads, as the spec requires.
pub(super) fn ktx1_container(
    gl_type: u32,
    gl_format: u32,
    internal_format: u32,
    base_internal_format: u32,
    width: usize,
    height: usize,
    payload: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + 4 + payload.len());
    out.extend_from_slice(b"\xABKTX 11\xBB\r\n\x1A\n");
    out.extend_from_slice(&0x04030201u32.to_le_bytes()); // endianness marker
    out.extend_from_slice(&gl_type.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // glTypeSize
    out.extend_from_slice(&gl_format.to_le_bytes());
    out.extend_from_slice(&internal_format.to_le_bytes());
    out.extend_from_slice(&base_internal_format.to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes()); // pixelWidth
    out.extend_from_slice(&(height as u32).to_le_bytes()); // pixelHeight
    out.extend_from_slice(&0u32.to_le_bytes()); // pixelDepth
    out.extend_from_slice(&0u32.to_le_bytes()); // numberOfArrayElements
    out.extend_from_slice(&1u32.to_le_bytes()); // numberOfFaces
    out.extend_from_slice(&1u32.to_le_bytes()); // numberOfMipmapLevels
    out.extend_from_slice(&0u32.to_le_bytes()); // bytesOfKeyValueData
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // imageSize
    out.extend_from_slice(payload);
    // KTX padi cada nivel a un múltiplo de 4 bytes.
    let pad = (4 - (payload.len() % 4)) % 4;
    out.extend(std::iter::repeat_n(0u8, pad));
    out
}

/// Uncompressed KTX used as the payload of `.zktx`.
pub(super) fn ktx1_rgba8(
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    data: &[u8],
) -> Vec<u8> {
    use image::ExtendedColorType as Ct;
    let (gl_format, internal, base) = match color {
        Ct::Rgb8 => (0x1907, 0x8051, 0x1907), // GL_RGB8
        Ct::L8 => (0x1909, 0x1909, 0x1909),   // GL_LUMINANCE
        Ct::La8 => (0x190A, 0x190A, 0x190A),  // GL_LUMINANCE_ALPHA
        _ => (0x1908, 0x8058, 0x1908),        // GL_RGBA8
    };
    ktx1_container(0x1401, gl_format, internal, base, width, height, data)
}

/// KTX v2 with one uncompressed mip level (`.ktx2`). The pixel format
/// conversion runs as in `.zktx`, but KTX2 only describes three layouts here
/// through vkFormat: RGBA8, RGB8 and R8.
pub(super) fn encode_ktx2(
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    use image::ExtendedColorType as Ct;
    if width == 0 || height == 0 {
        return Err(TpError::Other("KTX2: lienzo 0×0".to_string()));
    }
    let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
    let vk_format: u32 = match color {
        Ct::Rgba8 => 37, // VK_FORMAT_R8G8B8A8_UNORM
        Ct::Rgb8 => 10,  // VK_FORMAT_R8G8B8_UNORM
        Ct::L8 => 9,     // VK_FORMAT_R8_UNORM
        _ => {
            return Err(TpError::Other(format!(
                "KTX2 no puede describir el pixel format {} (solo RGBA8, \
                 RGB8 y un canal de 8 bits)",
                opts.pixel_format.as_str()
            )));
        }
    };
    let texels = width * height;
    let bytes_per_texel = (data.len() / texels) as u8;
    let dfd = ktx2_dfd(color, bytes_per_texel);
    let kvd = ktx2_kvd();
    // Cabecera (64) + índice (32) + índice de niveles (24) = 104.
    let level_offset = 104 + dfd.len() + kvd.len();

    let mut out = Vec::with_capacity(level_offset + data.len());
    out.extend_from_slice(b"\xABKTX 20\xBB\r\n\x1A\n"); // identifier
    out.extend_from_slice(&vk_format.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // typeSize (1 byte por muestra)
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // pixelDepth (2D)
    out.extend_from_slice(&0u32.to_le_bytes()); // layerCount (no-array)
    out.extend_from_slice(&1u32.to_le_bytes()); // faceCount
    out.extend_from_slice(&1u32.to_le_bytes()); // levelCount (solo base)
    out.extend_from_slice(&0u32.to_le_bytes()); // supercompressionScheme: none
    out.extend_from_slice(&104u32.to_le_bytes()); // dfdByteOffset
    out.extend_from_slice(&(dfd.len() as u32).to_le_bytes());
    out.extend_from_slice(&((104 + dfd.len()) as u32).to_le_bytes()); // kvdByteOffset
    out.extend_from_slice(&(kvd.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // sgdByteOffset
    out.extend_from_slice(&0u64.to_le_bytes()); // sgdByteLength
                                                // Índice de niveles: sin supercompresión, byteLength = datos crudos.
    out.extend_from_slice(&(level_offset as u64).to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&dfd);
    out.extend_from_slice(&kvd);
    out.extend_from_slice(&data);
    Ok(out)
}

/// Descriptor de formato (DFD, spec [KDF14]) de una textura sin comprimir:
/// modelo RGBSDA, primarios BT.709, transferencia lineal y una muestra de 8
/// bits por canal con rango 0..255.
fn ktx2_dfd(color: image::ExtendedColorType, bytes_per_texel: u8) -> Vec<u8> {
    use image::ExtendedColorType as Ct;
    // (bitOffset, channel): RED/GREEN/BLUE/ALPHA del modelo RGBSDA.
    let channels: &[(u16, u8)] = match color {
        Ct::Rgba8 => &[(0, 0), (8, 1), (16, 2), (24, 15)],
        Ct::Rgb8 => &[(0, 0), (8, 1), (16, 2)],
        _ => &[(0, 0)],
    };
    // El bloque básico mide 24 bytes y cada muestra otros 16.
    let block_size = 24 + channels.len() * 16;
    let mut dfd = Vec::with_capacity(4 + block_size);
    dfd.extend_from_slice(&((4 + block_size) as u32).to_le_bytes()); // dfdTotalSize
                                                                     // vendorId = 0 (Khronos) y descriptorType = 0 en el primer Word.
    dfd.extend_from_slice(&0u32.to_le_bytes());
    // versionNumber = 2 (bajos) y descriptorBlockSize (altos).
    dfd.extend_from_slice(&((2u32) | ((block_size as u32) << 16)).to_le_bytes());
    // colorModel RGBSDA (1), primarios BT.709 (1), transfer LINEAR (1),
    // flags = 0 (alfa directo, sin premultiplicar).
    dfd.extend_from_slice(&[1, 1, 1, 0]);
    dfd.extend_from_slice(&[0, 0, 0, 0]); // texelBlockDimension (1×1 ⇒ 0)
    dfd.push(bytes_per_texel); // bytesPlane[0]
    dfd.extend_from_slice(&[0u8; 7]); // bytesPlane[1..8]
    for (bit_offset, channel) in channels {
        // bitLength = 7 (8 bits) y channelType en los 4 bits altos.
        dfd.extend_from_slice(
            &((*bit_offset as u32) | (7 << 16) | (u32::from(*channel) << 24)).to_le_bytes(),
        );
        dfd.extend_from_slice(&0u32.to_le_bytes()); // samplePosition[0..4]
        dfd.extend_from_slice(&0u32.to_le_bytes()); // sampleLower
        dfd.extend_from_slice(&255u32.to_le_bytes()); // sampleUpper
    }
    dfd
}

/// Key/value data con las dos claves que define la spec, ordenadas por el
/// código Unicode de la clave: `KTXorientation` (x→derecha, y→abajo, el
/// orden de filas que escribimos) y `KTXwriter`.
fn ktx2_kvd() -> Vec<u8> {
    let entries: [(&str, &str); 2] = [
        ("KTXorientation", "rd"),
        (
            "KTXwriter",
            concat!("TexturePacker-RS ", env!("CARGO_PKG_VERSION")),
        ),
    ];
    let mut kvd = Vec::new();
    for (key, value) in entries {
        // clave + NUL + valor + NUL del valor (incluidos en la longitud).
        kvd.extend_from_slice(&((key.len() + value.len() + 2) as u32).to_le_bytes());
        kvd.extend_from_slice(key.as_bytes());
        kvd.push(0);
        kvd.extend_from_slice(value.as_bytes());
        kvd.push(0);
        // valuePadding hasta la siguiente frontera de 4 bytes.
        let pad = (4 - (kvd.len() % 4)) % 4;
        kvd.resize(kvd.len() + pad, 0);
    }
    kvd
}

/// zlib del contenido (`.zktx`).
pub(super) fn zlib_bytes(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::config::{GpuFormat, PixelFormat};
    use crate::export::encode_to_bytes;
    use crate::export::test_helpers::{gradient, le32, opts};

    #[test]
    fn zktx_is_a_zlib_compressed_ktx() {
        use std::io::Read;
        let (w, h) = (4, 4);
        let rgba = gradient(w, h);
        let bytes = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Zktx)).unwrap();
        let mut ktx = Vec::new();
        flate2::read::ZlibDecoder::new(&bytes[..])
            .read_to_end(&mut ktx)
            .expect("zktx es zlib");
        assert_eq!(&ktx[..12], b"\xABKTX 11\xBB\r\n\x1A\n");
        assert_eq!(le32(&ktx, 16), 0x1401, "glType = GL_UNSIGNED_BYTE");
        assert_eq!(le32(&ktx, 24), 0x1908, "glFormat = GL_RGBA");
        assert_eq!(le32(&ktx, 28), 0x8058, "glInternalFormat = GL_RGBA8");
        assert_eq!(le32(&ktx, 36), w as u32);
        assert_eq!(le32(&ktx, 40), h as u32);
        let image_size = le32(&ktx, 64) as usize;
        assert_eq!(image_size, rgba.len());
        assert_eq!(&ktx[68..68 + image_size], &rgba[..]);
    }

    #[test]
    fn ktx2_writes_a_spec_container_with_dfd_and_kvd() {
        let (w, h) = (8, 4);
        let rgba = gradient(w, h);
        let file = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Ktx2)).unwrap();

        // Cabecera: identificador, vkFormat crudo y un solo nivel sin
        // supercompresión.
        assert_eq!(&file[..12], b"\xABKTX 20\xBB\r\n\x1A\n");
        assert_eq!(le32(&file, 12), 37, "VK_FORMAT_R8G8B8A8_UNORM");
        assert_eq!(le32(&file, 16), 1, "typeSize");
        assert_eq!(le32(&file, 20), w as u32, "pixelWidth");
        assert_eq!(le32(&file, 24), h as u32, "pixelHeight");
        assert_eq!(le32(&file, 36), 1, "faceCount");
        assert_eq!(le32(&file, 40), 1, "levelCount");
        assert_eq!(le32(&file, 44), 0, "supercompressionScheme");

        // Índice: el DFD empieza siempre en 104 (80 de cabecera + 24 del
        // índice de niveles) y la KVD le sigue.
        let dfd_offset = le32(&file, 48) as usize;
        let dfd_len = le32(&file, 52) as usize;
        let kvd_offset = le32(&file, 56) as usize;
        let kvd_len = le32(&file, 60) as usize;
        assert_eq!(dfd_offset, 104);
        assert_eq!(dfd_len, 4 + 24 + 4 * 16, "bloque básico + 4 muestras");
        assert_eq!(kvd_offset, dfd_offset + dfd_len);
        assert_eq!(le32(&file, 64), 0, "sin supercompression global data");

        // El índice de niveles apunta justo detrás de la KVD y cierra el
        // fichero con los datos crudos.
        let level_offset = u64::from_le_bytes(file[80..88].try_into().unwrap()) as usize;
        let level_len = u64::from_le_bytes(file[88..96].try_into().unwrap()) as usize;
        let level_raw = u64::from_le_bytes(file[96..104].try_into().unwrap()) as usize;
        assert_eq!(level_offset, kvd_offset + kvd_len);
        assert_eq!(level_len, w * h * 4);
        assert_eq!(level_raw, level_len, "byteLength = uncompressedByteLength");
        assert_eq!(
            file.len(),
            level_offset + level_len,
            "el nivel cierra el fichero"
        );
        assert_eq!(&file[level_offset..], &rgba[..], "payload RGBA crudo");

        // DFD [KDF14]: RGBSDA, BT.709, transferencia lineal, alfa directo.
        assert_eq!(le32(&file, 104), dfd_len as u32, "dfdTotalSize");
        assert_eq!(le32(&file, 108), 0, "vendorId Khronos y descriptorType");
        assert_eq!(le32(&file, 112) & 0xFFFF, 2, "versionNumber");
        assert_eq!(
            le32(&file, 112) >> 16,
            (dfd_len - 4) as u32,
            "descriptorBlockSize"
        );
        assert_eq!(
            &file[116..120],
            &[1, 1, 1, 0],
            "RGBSDA, BT.709, lineal, flags"
        );
        assert_eq!(&file[120..124], &[0, 0, 0, 0], "texel 1×1 -> dimensiones 0");
        assert_eq!(file[124], 4, "bytesPlane[0] = 4 bytes por texel");
        // Una muestra de 8 bits por canal: R, G, B y alfa en el canal 15.
        let channels = [0u32, 1, 2, 15];
        for (i, channel) in channels.iter().enumerate() {
            let at = 132 + i * 16;
            assert_eq!(
                le32(&file, at),
                (i as u32 * 8) | (7 << 16) | (channel << 24),
                "bitOffset/bitLength/channelType de la muestra {i}"
            );
            assert_eq!(le32(&file, at + 4), 0, "samplePosition");
            assert_eq!(le32(&file, at + 8), 0, "sampleLower");
            assert_eq!(le32(&file, at + 12), 255, "sampleUpper");
        }

        // KVD: `KTXorientation` (rd) y `KTXwriter`, ordenadas por clave y con
        // la fórmula de la spec (4 + ceil(longitud/4)·4 por entrada).
        let kvd = &file[kvd_offset..kvd_offset + kvd_len];
        let orient_len = u32::from_le_bytes(kvd[0..4].try_into().unwrap()) as usize;
        assert_eq!(orient_len, 18, "clave + NUL + 'rd' + NUL");
        assert_eq!(&kvd[4..18], b"KTXorientation");
        assert_eq!(&kvd[19..21], b"rd");
        let writer_start = 4 + orient_len.div_ceil(4) * 4;
        let writer_len =
            u32::from_le_bytes(kvd[writer_start..writer_start + 4].try_into().unwrap()) as usize;
        assert_eq!(
            &kvd[writer_start + 4..writer_start + 13],
            b"KTXwriter",
            "la clave va ordenada después de KTXorientation"
        );
        let writer = &kvd[writer_start + 4..writer_start + 4 + writer_len];
        assert!(
            writer
                .windows(b"TexturePacker-RS".len())
                .any(|w| w == b"TexturePacker-RS"),
            "KTXwriter lleva el nombre y la versión"
        );
        assert_eq!(
            kvd.len(),
            writer_start + 4 + writer_len.div_ceil(4) * 4,
            "kvdByteLength de la spec"
        );
    }

    #[test]
    fn ktx2_pixel_formats_round_trip_through_the_reader() {
        let (w, h) = (6, 5); // dimensiones no múltiplos de 4
        let mut rgba = gradient(w, h);
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let path = std::env::temp_dir().join(format!("tp_ktx2_{}.ktx2", std::process::id()));

        // RGBA8: el payload viaja intacto.
        let file = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Ktx2)).unwrap();
        std::fs::write(&path, &file).unwrap();
        let (rw, rh, out) = crate::reader::load_image_rgba(&path).unwrap();
        assert_eq!((rw, rh), (w as i32, h as i32));
        assert_eq!(out, rgba, "RGBA8 se lee byte a byte");

        // RGB888: se compone sobre negro y el vkFormat pasa a R8G8B8_UNORM.
        let mut o = opts(GpuFormat::Ktx2);
        o.pixel_format = PixelFormat::Rgb888;
        let rgb = encode_to_bytes(&rgba, w, h, &o).unwrap();
        assert_eq!(le32(&rgb, 12), 10, "VK_FORMAT_R8G8B8_UNORM");
        std::fs::write(&path, &rgb).unwrap();
        let (_, _, out) = crate::reader::load_image_rgba(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(out, rgba, "los colores opacos no cambian al pasar a RGB");
    }

    #[test]
    fn ktx2_reports_pixel_formats_it_cannot_describe() {
        let rgba = gradient(4, 4);
        let mut o = opts(GpuFormat::Ktx2);
        o.pixel_format = PixelFormat::AlphaIntensity8;
        let err = encode_to_bytes(&rgba, 4, 4, &o).unwrap_err().to_string();
        assert!(
            err.contains("ALPHA_INTENSITY8") && err.contains("RGBA8"),
            "{err}"
        );
    }
}
