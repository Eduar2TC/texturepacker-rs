//! Conversión de contenedores entre `.basis` y KTX2 con supercompresión Basis.
//!
//! El codificador de `tp-basis` produce ficheros `.basis`. Los formatos
//! `KTX2_BASIS` (ETC1S, supercompresión BasisLZ) y `KTX2_BASIS_UASTC`
//! (UASTC 4x4, sin supercompresión) reescriben ese fichero dentro de un
//! contenedor KTX2 con el mismo layout que ensambla el codificador de
//! referencia de Basis Universal (`create_ktx2_file()`), y el lector hace el
//! viaje inverso: reconstruye el `.basis` (con sus CRC16) y lo transcodifica
//! con la misma crate que lo escribe.
//!
//! Layout del `.basis` (todo little-endian, `packed_uint<N>` = N bytes):
//! cabecera de 77 bytes + un slice desc de 23 bytes por slice + paletas de
//! endpoints/selectors + tablas + datos de slices. El CRC16 de cabecera
//! cubre la cabecera desde `m_data_size` (offset 8) hasta el final.
//!
//! Layout del KTX2 (idéntico al del vendor): cabecera de 80 + índice de
//! niveles de 24 (offset 80) = DFD en 104, KVD contiguo al DFD, SGD
//! alineado a 8 (sólo ETC1S) y el nivel al final (alineado a 16 cuando la
//! supercompresión es NONE, con la entrada ficticia del vendor dentro del
//! KVD para no dejar bytes huérfanos).

use super::export::ktx::ktx2_kvd;

// -- Constantes del formato `.basis` ----------------------------------------

const BASIS_HEADER_SIZE: usize = 77;
const BASIS_SLICE_DESC_SIZE: usize = 23;
/// `BASISD_SUPPORTED_BASIS_VERSION` del transcodificador vendor.
const BASIS_VERSION: u16 = 19;
const BASIS_FLAG_ETC1S: u16 = 1;
const BASIS_FLAG_HAS_ALPHA_SLICES: u16 = 4;
const BASIS_FLAG_SRGB: u16 = 16;
const BASIS_SLICE_FLAG_HAS_ALPHA: u8 = 1;
const BASIS_TEX_FORMAT_ETC1S: u8 = 0;
const BASIS_TEX_FORMAT_UASTC: u8 = 1;

// -- Constantes del contenedor KTX2 -----------------------------------------

const KTX2_HEADER_SIZE: usize = 80;
const KTX2_LEVEL_INDEX: usize = KTX2_HEADER_SIZE; // 3 × u64 = 24 bytes
const KTX2_DFD_OFFSET: usize = KTX2_HEADER_SIZE + 24; // 104
const KTX2_SS_NONE: u32 = 0;
const KTX2_SS_BASISLZ: u32 = 1;
const DFD_MODEL_ETC1S: u8 = 163;
const DFD_MODEL_UASTC: u8 = 166;
const DFD_TRANSFER_LINEAR: u8 = 1;
const DFD_TRANSFER_SRGB: u8 = 2;
/// `KTX2_DF_CHANNEL_UASTC_RGBA` / `_RRRG`: los dos canales con alfa.
const DFD_UASTC_ALPHA_CHANNELS: [u8; 2] = [3, 5];
const KTX2_IDENT: &[u8; 12] = b"\xABKTX 20\xBB\r\n\x1A\n";

// -- Plantillas DFD del codificador vendor (verbatim) ------------------------

/// DFD ETC1S sin alfa (44 bytes): modelo 163, una muestra RRR.
const DFD_ETC1S_NONALPHA: [u8; 44] = [
    0x2C, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, 0x0, 0x28, 0x0, 0xA3, 0x1, 0x2, 0x0, 0x3, 0x3,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x3F, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0xFF, 0xFF, 0xFF, 0xFF,
];
/// DFD ETC1S con alfa (60 bytes): dos muestras (color + alfa).
const DFD_ETC1S_ALPHA: [u8; 60] = [
    0x3C, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, 0x0, 0x38, 0x0, 0xA3, 0x1, 0x2, 0x0, 0x3, 0x3,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x3F, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0x0, 0x3F, 0xF, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0xFF, 0xFF, 0xFF, 0xFF,
];
/// DFD UASTC 4x4 sin alfa (44 bytes): modelo 166, canal RRR.
const DFD_UASTC_NONALPHA: [u8; 44] = [
    0x2C, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, 0x0, 0x28, 0x0, 0xA6, 0x1, 0x2, 0x0, 0x3, 0x3,
    0x0, 0x0, 0x10, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x7F, 0x4, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0xFF, 0xFF, 0xFF, 0xFF,
];
/// DFD UASTC 4x4 con alfa (44 bytes): canal RGBA.
const DFD_UASTC_ALPHA: [u8; 44] = [
    0x2C, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, 0x0, 0x28, 0x0, 0xA6, 0x1, 0x2, 0x0, 0x3, 0x3,
    0x0, 0x0, 0x10, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x7F, 0x3, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0xFF, 0xFF, 0xFF, 0xFF,
];

// -- Lecturas y escrituras packed --------------------------------------------

fn le16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn le32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn le64(b: &[u8], off: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(buf)
}

/// `packed_uint<N>`: N bytes little-endian.
fn push_packed(out: &mut Vec<u8>, bytes: usize, value: u32) {
    out.extend_from_slice(&value.to_le_bytes()[..bytes]);
}

/// Escribe un `packed_uint<N>` en una posición ya reservada.
fn put(out: &mut [u8], off: usize, bytes: usize, value: u32) {
    out[off..off + bytes].copy_from_slice(&value.to_le_bytes()[..bytes]);
}

/// Réplica exacta de `crc16()` del transcodificador vendor (la del header
/// cubre `[8..77)` con semilla 0; la de datos, el bloque posterior).
fn crc16(data: &[u8], seed: u16) -> u16 {
    let mut crc = !seed;
    for &byte in data {
        let q = u16::from(byte) ^ (crc >> 8);
        let k = (q >> 4) ^ q;
        crc = (((crc << 8) ^ k) ^ (k << 5)) ^ (k << 12);
    }
    !crc
}

// -- Parseo del `.basis` ------------------------------------------------------

/// Campos de la cabecera `.basis` que hace falta leer para reescribir el
/// fichero dentro de un KTX2.
struct BasisHeader {
    total_images: u32,
    tex_format: u8,
    flags: u16,
    total_endpoints: u32,
    endpoint_ofs: u32,
    endpoint_size: u32,
    total_selectors: u32,
    selector_ofs: u32,
    selector_size: u32,
    tables_ofs: u32,
    tables_size: u32,
    slice_desc_ofs: u32,
}

/// Un slice desc (23 bytes) con los campos útiles.
struct SliceDesc {
    orig_width: u16,
    orig_height: u16,
    file_ofs: u32,
    file_size: u32,
}

/// Valida la firma y la versión y devuelve los campos de cabecera y los
/// slice descs, comprobando que todas las regiones caen dentro del buffer.
fn parse_basis(bytes: &[u8]) -> Result<(BasisHeader, Vec<SliceDesc>), String> {
    if bytes.len() < BASIS_HEADER_SIZE || &bytes[..2] != b"sB" {
        return Err("cabecera inválida o truncada".to_string());
    }
    let ver = le16(bytes, 2);
    let header_size = le16(bytes, 4);
    if ver != BASIS_VERSION || header_size as usize != BASIS_HEADER_SIZE {
        return Err(format!("cabecera inválida o truncada (versión {ver})"));
    }
    // `packed_uint<3>`: 3 bytes little-endian (el 4.º byte es del campo
    // siguiente, así que se enmascara).
    let p3 = |off: usize| le32(bytes, off) & 0xFF_FFFF;
    let total_slices = p3(14);
    let total_images = p3(17);
    if total_slices == 0 || total_images == 0 || total_images > total_slices {
        return Err("cabecera inválida o truncada".to_string());
    }
    let tex_format = bytes[20];
    let flags = le16(bytes, 21);
    if tex_format != BASIS_TEX_FORMAT_ETC1S && tex_format != BASIS_TEX_FORMAT_UASTC {
        return Err(format!("formato de textura desconocido ({tex_format})"));
    }
    let etc1s = tex_format == BASIS_TEX_FORMAT_ETC1S;
    if etc1s != (flags & BASIS_FLAG_ETC1S != 0) {
        return Err("banderas incoherentes con el formato".to_string());
    }
    if etc1s && flags & BASIS_FLAG_HAS_ALPHA_SLICES != 0 && total_slices & 1 != 0 {
        return Err("banderas incoherentes con el formato".to_string());
    }
    // Regiones de paletas y tablas: dentro del buffer y sin desbordes.
    let h = BasisHeader {
        total_images,
        tex_format,
        flags,
        total_endpoints: le16(bytes, 39) as u32,
        endpoint_ofs: le32(bytes, 41),
        endpoint_size: p3(45),
        total_selectors: le16(bytes, 48) as u32,
        selector_ofs: le32(bytes, 50),
        selector_size: p3(54),
        tables_ofs: le32(bytes, 57),
        tables_size: le32(bytes, 61),
        slice_desc_ofs: le32(bytes, 65),
    };
    let in_bounds = |ofs: u32, size: u32| (ofs as usize) + (size as usize) <= bytes.len();
    if !in_bounds(h.endpoint_ofs, h.endpoint_size)
        || !in_bounds(h.selector_ofs, h.selector_size)
        || !in_bounds(h.tables_ofs, h.tables_size)
        || !in_bounds(
            h.slice_desc_ofs,
            total_slices * BASIS_SLICE_DESC_SIZE as u32,
        )
    {
        return Err("componentes fuera de límites".to_string());
    }
    let slice_desc_ofs = h.slice_desc_ofs as usize;
    let slices = (0..total_slices as usize)
        .map(|i| {
            let s = &bytes[slice_desc_ofs + i * BASIS_SLICE_DESC_SIZE..];
            let desc = SliceDesc {
                orig_width: le16(s, 5),
                orig_height: le16(s, 7),
                file_ofs: le32(s, 13),
                file_size: le32(s, 17),
            };
            if !in_bounds(desc.file_ofs, desc.file_size) {
                return Err("componentes fuera de límites".to_string());
            }
            Ok(desc)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((h, slices))
}

// -- `.basis` → KTX2 ----------------------------------------------------------

/// Reenvuelve un `.basis` (ETC1S o UASTC, con una única imagen) en un KTX2
/// con supercompresión Basis, replicando el layout del codificador vendor.
pub(crate) fn basis_to_ktx2(basis: &[u8]) -> Result<Vec<u8>, String> {
    let (h, slices) = parse_basis(basis).map_err(|e| format!("Basis: {e}"))?;
    if h.total_images != 1 {
        return Err("Basis: se esperaba una única imagen".to_string());
    }
    let etc1s = h.tex_format == BASIS_TEX_FORMAT_ETC1S;
    let alpha = h.flags & BASIS_FLAG_HAS_ALPHA_SLICES != 0;
    // ETC1S con alfa usa dos slices (par rgb/alfa); UASTC lleva el alfa
    // dentro de su único slice.
    let expected = if etc1s && alpha { 2 } else { 1 };
    if slices.len() != expected {
        return Err(format!(
            "Basis: número de slices inesperado ({})",
            slices.len()
        ));
    }

    // El nivel KTX2 es la concatenación de los slices en orden, igual que
    // `create_ktx2_file()`.
    let mut level = Vec::new();
    for s in &slices {
        level.extend_from_slice(
            &basis[s.file_ofs as usize..s.file_ofs as usize + s.file_size as usize],
        );
    }
    let rgb_size = slices[0].file_size;
    let alpha_size = if etc1s && alpha {
        slices[1].file_size
    } else {
        0
    };

    let dfd = match (etc1s, alpha) {
        (true, false) => DFD_ETC1S_NONALPHA.to_vec(),
        (true, true) => DFD_ETC1S_ALPHA.to_vec(),
        (false, false) => DFD_UASTC_NONALPHA.to_vec(),
        (false, true) => DFD_UASTC_ALPHA.to_vec(),
    };

    // KVD contiguo al DFD; con scheme NONE el vendor lo rellena con una
    // entrada ficticia hasta alinearlo a 16 (el nivel arranca alineado).
    let scheme = if etc1s { KTX2_SS_BASISLZ } else { KTX2_SS_NONE };
    let mut kvd = ktx2_kvd();
    if scheme == KTX2_SS_NONE {
        kvd = pad_kvd_to_16(kvd, KTX2_DFD_OFFSET + dfd.len());
    }
    let after_kvd = KTX2_DFD_OFFSET + dfd.len() + kvd.len();

    // El SGD (sólo ETC1S) va alineado a 8 y el nivel le sigue de inmediato.
    let mut sgd = Vec::new();
    let (sgd_ofs, level_ofs) = if etc1s {
        sgd.extend_from_slice(&(h.total_endpoints as u16).to_le_bytes());
        sgd.extend_from_slice(&(h.total_selectors as u16).to_le_bytes());
        sgd.extend_from_slice(&h.endpoint_size.to_le_bytes());
        sgd.extend_from_slice(&h.selector_size.to_le_bytes());
        sgd.extend_from_slice(&h.tables_size.to_le_bytes());
        sgd.extend_from_slice(&0u32.to_le_bytes()); // extended_byte_length
                                                    // Descriptor de la imagen: offsets relativos al nivel.
        sgd.extend_from_slice(&0u32.to_le_bytes()); // image_flags
        sgd.extend_from_slice(&0u32.to_le_bytes()); // rgb_slice_byte_offset
        sgd.extend_from_slice(&rgb_size.to_le_bytes());
        sgd.extend_from_slice(&(if alpha { rgb_size } else { 0 }).to_le_bytes());
        sgd.extend_from_slice(&alpha_size.to_le_bytes());
        sgd.extend_from_slice(
            &basis[h.endpoint_ofs as usize..h.endpoint_ofs as usize + h.endpoint_size as usize],
        );
        sgd.extend_from_slice(
            &basis[h.selector_ofs as usize..h.selector_ofs as usize + h.selector_size as usize],
        );
        sgd.extend_from_slice(
            &basis[h.tables_ofs as usize..h.tables_ofs as usize + h.tables_size as usize],
        );
        let sgd_ofs = (after_kvd + 7) & !7;
        (sgd_ofs, sgd_ofs + sgd.len())
    } else {
        (0, after_kvd) // ya alineado a 16 por el hack del KVD
    };
    let uncompressed = if etc1s { 0 } else { level.len() as u64 };

    let width = u32::from(slices[0].orig_width);
    let height = u32::from(slices[0].orig_height);
    let mut out = Vec::with_capacity(level_ofs + level.len());
    out.extend_from_slice(KTX2_IDENT);
    out.extend_from_slice(&0u32.to_le_bytes()); // vkFormat (DFD)
    out.extend_from_slice(&1u32.to_le_bytes()); // typeSize
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // pixelDepth (2D)
    out.extend_from_slice(&0u32.to_le_bytes()); // layerCount (no-array)
    out.extend_from_slice(&1u32.to_le_bytes()); // faceCount
    out.extend_from_slice(&1u32.to_le_bytes()); // levelCount (solo base)
    out.extend_from_slice(&scheme.to_le_bytes());
    out.extend_from_slice(&(KTX2_DFD_OFFSET as u32).to_le_bytes());
    out.extend_from_slice(&(dfd.len() as u32).to_le_bytes());
    out.extend_from_slice(&((KTX2_DFD_OFFSET + dfd.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(kvd.len() as u32).to_le_bytes());
    out.extend_from_slice(&(sgd_ofs as u64).to_le_bytes());
    out.extend_from_slice(&(sgd.len() as u64).to_le_bytes());
    // Índice de niveles.
    out.extend_from_slice(&(level_ofs as u64).to_le_bytes());
    out.extend_from_slice(&(level.len() as u64).to_le_bytes());
    out.extend_from_slice(&uncompressed.to_le_bytes());
    out.extend_from_slice(&dfd);
    out.extend_from_slice(&kvd);
    if etc1s {
        out.resize(sgd_ofs, 0); // relleno hasta la alineación a 8 del SGD
        out.extend_from_slice(&sgd);
    }
    out.extend_from_slice(&level);
    debug_assert_eq!(out.len(), level_ofs + level.len());
    Ok(out)
}

/// Entrada ficticia dentro del KVD para que su cierre caiga en un múltiplo
/// de 16 (hack del vendor: la clave lleva bytes 127 y un valor vacío, y la
/// longitud crece a ≥ 6 como exige el parseo de KVD).
fn pad_kvd_to_16(mut kvd: Vec<u8>, kvd_ofs: usize) -> Vec<u8> {
    let mut needed = (16 - ((kvd_ofs + kvd.len()) & 15)) & 15;
    if needed == 0 {
        return kvd;
    }
    if needed < 6 {
        needed += 16;
    }
    push_packed(&mut kvd, 4, needed as u32 - 4);
    kvd.extend(std::iter::repeat_n(127u8, needed - 4 - 1 - 1));
    kvd.push(0); // fin de la clave
    kvd.push(0); // valor vacío
    kvd
}

// -- KTX2 → `.basis` ----------------------------------------------------------

/// Reconstruye el `.basis` que describe un KTX2 con payload Basis (DFD
/// colorModel 163 ETC1S o 166 UASTC) para transcodificarlo con `tp_basis`.
pub(crate) fn ktx2_to_basis(ktx2: &[u8]) -> Result<Vec<u8>, String> {
    if ktx2.len() < KTX2_HEADER_SIZE || &ktx2[..12] != KTX2_IDENT {
        return Err("KTX2: cabecera inválida".to_string());
    }
    let vk_format = le32(ktx2, 12);
    if vk_format != 0 {
        return Err(format!(
            "KTX2: vkFormat incoherente con un payload Basis ({vk_format})"
        ));
    }
    let width = le32(ktx2, 20);
    let height = le32(ktx2, 24);
    if width == 0 || height == 0 || le32(ktx2, 28) != 0 || le32(ktx2, 36) != 1 {
        return Err("KTX2: cabecera inválida".to_string());
    }
    let scheme = le32(ktx2, 44);
    if scheme != KTX2_SS_NONE && scheme != KTX2_SS_BASISLZ {
        return Err(format!(
            "KTX2: supercompresión incoherente con un payload Basis ({scheme})"
        ));
    }
    let dfd_ofs = le32(ktx2, 48) as usize;
    let dfd_len = le32(ktx2, 52) as usize;
    if dfd_ofs < KTX2_LEVEL_INDEX + 24
        || dfd_ofs + dfd_len > ktx2.len()
        || (dfd_len != 44 && dfd_len != 60)
    {
        return Err(format!("KTX2: DFD Basis de tamaño inesperado ({dfd_len})"));
    }
    let dfd = &ktx2[dfd_ofs..dfd_ofs + dfd_len];
    if le32(dfd, 0) as usize != dfd_len {
        return Err(format!("KTX2: DFD Basis de tamaño inesperado ({dfd_len})"));
    }
    let model = dfd[12];
    let (etc1s, alpha) = match model {
        DFD_MODEL_ETC1S => (true, dfd_len == 60),
        DFD_MODEL_UASTC => {
            let chan = (le32(dfd, 28) >> 24) as u8 & 15;
            (false, DFD_UASTC_ALPHA_CHANNELS.contains(&chan))
        }
        _ => {
            return Err(format!(
                "KTX2: el DFD no describe Basis (colorModel {model})"
            ))
        }
    };
    if (etc1s && scheme != KTX2_SS_BASISLZ) || (!etc1s && scheme != KTX2_SS_NONE) {
        return Err(format!(
            "KTX2: supercompresión incoherente con un payload Basis ({scheme})"
        ));
    }
    let transfer = dfd[14];
    if transfer != DFD_TRANSFER_LINEAR && transfer != DFD_TRANSFER_SRGB {
        return Err(format!("KTX2: transferencia DFD no soportada ({transfer})"));
    }

    // Índice de niveles: sólo se lee el nivel base.
    let level_ofs = le64(ktx2, KTX2_LEVEL_INDEX) as usize;
    let level_len = le64(ktx2, KTX2_LEVEL_INDEX + 8) as usize;
    if level_ofs < KTX2_LEVEL_INDEX + 24 || level_ofs + level_len > ktx2.len() {
        return Err("KTX2: nivel Basis fuera de límites".to_string());
    }
    let level = &ktx2[level_ofs..level_ofs + level_len];

    let blocks_x = width.div_ceil(4) as u16;
    let blocks_y = height.div_ceil(4) as u16;
    // Componentes ETC1S (paletas y tablas) dentro del SGD; UASTC no lleva
    // ninguno y su nivel es exactamente bloques × 16 bytes.
    let (total_endpoints, total_selectors, endpoints, selectors, tables, rgb, alpha_data) = if etc1s
    {
        let sgd_ofs = le64(ktx2, 64) as usize;
        let sgd_len = le64(ktx2, 72) as usize;
        // Cabecera del SGD (20) + descriptor de la imagen (20).
        if sgd_len < 40 || sgd_ofs + sgd_len > ktx2.len() {
            return Err("KTX2: SGD Basis truncado".to_string());
        }
        let sgd = &ktx2[sgd_ofs..sgd_ofs + sgd_len];
        let endpoints_len = le32(sgd, 4) as usize;
        let selectors_len = le32(sgd, 8) as usize;
        let tables_len = le32(sgd, 12) as usize;
        if le32(sgd, 16) != 0 || 40 + endpoints_len + selectors_len + tables_len > sgd.len() {
            return Err("KTX2: SGD Basis truncado".to_string());
        }
        let rgb_ofs = le32(sgd, 24) as usize;
        let rgb_len = le32(sgd, 28) as usize;
        let alpha_ofs = le32(sgd, 32) as usize;
        let alpha_data_len = le32(sgd, 36) as usize;
        let slice = |ofs: usize, len: usize| -> Result<&[u8], String> {
            if len == 0 || ofs + len > level.len() {
                return Err("KTX2: descriptores de imagen incoherentes en el SGD".to_string());
            }
            Ok(&level[ofs..ofs + len])
        };
        let rgb = slice(rgb_ofs, rgb_len)?;
        let alpha_data = if alpha {
            Some(slice(alpha_ofs, alpha_data_len)?)
        } else {
            None
        };
        let tables_start = 40 + endpoints_len + selectors_len;
        (
            u32::from(le16(sgd, 0)),
            u32::from(le16(sgd, 2)),
            &sgd[40..40 + endpoints_len],
            &sgd[40 + endpoints_len..tables_start],
            &sgd[tables_start..tables_start + tables_len],
            rgb,
            alpha_data,
        )
    } else {
        let expected = usize::from(blocks_x) * usize::from(blocks_y) * 16;
        if level.len() != expected {
            return Err("KTX2: el nivel UASTC no mide bloques x16".to_string());
        }
        (0, 0, &[][..], &[][..], &[][..], level, None)
    };

    // Cabecera de 77 bytes + slice descs + paletas/tablas + datos de slices;
    // los offsets de la cabecera apuntan a posiciones absolutas del fichero.
    let slices_count = if etc1s && alpha { 2 } else { 1 };
    let slice_desc_ofs = BASIS_HEADER_SIZE;
    let endpoint_ofs = slice_desc_ofs + slices_count * BASIS_SLICE_DESC_SIZE;
    let selector_ofs = endpoint_ofs + endpoints.len();
    let tables_ofs = selector_ofs + selectors.len();
    let data_ofs = tables_ofs + tables.len();
    let mut out = Vec::with_capacity(data_ofs + level.len());
    out.resize(slice_desc_ofs, 0); // la cabecera se rellena al final
    let push_slice_desc = |out: &mut Vec<u8>, flags: u8, ofs: usize, len: usize, crc: u16| {
        push_packed(out, 3, 0); // image_index
        push_packed(out, 1, 0); // level_index
        push_packed(out, 1, u32::from(flags));
        push_packed(out, 2, width);
        push_packed(out, 2, height);
        push_packed(out, 2, u32::from(blocks_x));
        push_packed(out, 2, u32::from(blocks_y));
        push_packed(out, 4, ofs as u32);
        push_packed(out, 4, len as u32);
        out.extend_from_slice(&crc.to_le_bytes());
    };
    push_slice_desc(&mut out, 0, data_ofs, rgb.len(), crc16(rgb, 0));
    if let Some(a) = alpha_data {
        push_slice_desc(
            &mut out,
            BASIS_SLICE_FLAG_HAS_ALPHA,
            data_ofs + rgb.len(),
            a.len(),
            crc16(a, 0),
        );
    }
    out.extend_from_slice(endpoints);
    out.extend_from_slice(selectors);
    out.extend_from_slice(tables);
    out.extend_from_slice(rgb);
    if let Some(a) = alpha_data {
        out.extend_from_slice(a);
    }
    debug_assert_eq!(out.len(), data_ofs + level.len());

    // Cabecera con los CRC16 ya sobre el payload completo.
    let data_len = (out.len() - BASIS_HEADER_SIZE) as u32;
    out[0..2].copy_from_slice(b"sB");
    out[2..4].copy_from_slice(&BASIS_VERSION.to_le_bytes());
    put(&mut out, 4, 2, BASIS_HEADER_SIZE as u32);
    put(&mut out, 8, 4, data_len);
    let data_crc = crc16(&out[BASIS_HEADER_SIZE..], 0);
    out[12..14].copy_from_slice(&data_crc.to_le_bytes());
    put(&mut out, 14, 3, slices_count as u32);
    put(&mut out, 17, 3, 1); // total_images
    put(
        &mut out,
        20,
        1,
        u32::from(if etc1s {
            BASIS_TEX_FORMAT_ETC1S
        } else {
            BASIS_TEX_FORMAT_UASTC
        }),
    );
    let mut flags = 0u16;
    if etc1s {
        flags |= BASIS_FLAG_ETC1S;
    }
    if alpha {
        flags |= BASIS_FLAG_HAS_ALPHA_SLICES;
    }
    if transfer == DFD_TRANSFER_SRGB {
        flags |= BASIS_FLAG_SRGB;
    }
    out[21..23].copy_from_slice(&flags.to_le_bytes());
    put(&mut out, 39, 2, total_endpoints);
    put(&mut out, 41, 4, endpoint_ofs as u32);
    put(&mut out, 45, 3, endpoints.len() as u32);
    put(&mut out, 48, 2, total_selectors);
    put(&mut out, 50, 4, selector_ofs as u32);
    put(&mut out, 54, 3, selectors.len() as u32);
    put(&mut out, 57, 4, tables_ofs as u32);
    put(&mut out, 61, 4, tables.len() as u32);
    put(&mut out, 65, 4, slice_desc_ofs as u32);
    let header_crc = crc16(&out[8..BASIS_HEADER_SIZE], 0);
    out[6..8].copy_from_slice(&header_crc.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cuatro cuadrantes de 16×16 (32×32 en total); con alfa el cuadrante
    /// inferior-izquierdo es semitransparente.
    fn quadrants(has_alpha: bool) -> (Vec<u8>, u32, u32) {
        let (w, h) = (32u32, 32u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c: [u8; 4] = match (x < 16, y < 16) {
                    (true, true) => [255, 0, 0, 255],
                    (false, true) => [0, 255, 0, 255],
                    (true, false) => [0, 0, 255, if has_alpha { 128 } else { 255 }],
                    (false, false) => [255, 255, 0, 255],
                };
                rgba.extend_from_slice(&c);
            }
        }
        (rgba, w, h)
    }

    fn encode(etc1s: bool, has_alpha: bool) -> Vec<u8> {
        let (rgba, w, h) = quadrants(has_alpha);
        if etc1s {
            tp_basis::encode_etc1s(&rgba, w, h, 80).unwrap()
        } else {
            tp_basis::encode_uastc(&rgba, w, h, 50).unwrap()
        }
    }

    /// Comprobaciones estructurales espejo de `ktx2_transcoder::init`.
    fn check_container(ktx2: &[u8], etc1s: bool, alpha: bool) {
        assert_eq!(&ktx2[..12], KTX2_IDENT, "identificador KTX2");
        assert_eq!(le32(ktx2, 12), 0, "vkFormat = 0 (DFD)");
        assert_eq!(le32(ktx2, 16), 1, "typeSize = 1");
        assert_eq!(le32(ktx2, 20), 32, "ancho");
        assert_eq!(le32(ktx2, 24), 32, "alto");
        assert_eq!(le32(ktx2, 28), 0, "sin profundidad");
        assert_eq!(le32(ktx2, 36), 1, "cara única");
        assert_eq!(le32(ktx2, 40), 1, "un nivel");
        let scheme = le32(ktx2, 44);
        assert_eq!(scheme, if etc1s { 1 } else { 0 }, "supercompresión");
        let dfd_ofs = le32(ktx2, 48) as usize;
        let dfd_len = le32(ktx2, 52) as usize;
        assert_eq!(dfd_ofs, 104, "el DFD arranca en 104");
        let want_dfd: &[u8] = match (etc1s, alpha) {
            (true, false) => &DFD_ETC1S_NONALPHA,
            (true, true) => &DFD_ETC1S_ALPHA,
            (false, false) => &DFD_UASTC_NONALPHA,
            (false, true) => &DFD_UASTC_ALPHA,
        };
        assert_eq!(&ktx2[dfd_ofs..dfd_ofs + dfd_len], want_dfd, "DFD verbatim");
        // El DFD declara su propio tamaño y el KVD le sigue contiguo.
        assert_eq!(le32(&ktx2[dfd_ofs..], 0) as usize, dfd_len);
        let kvd_ofs = le32(ktx2, 56) as usize;
        let kvd_len = le32(ktx2, 60) as usize;
        assert_eq!(kvd_ofs, dfd_ofs + dfd_len, "KVD contiguo al DFD");
        // El KVD se puede recorrer como pares clave-valor sin romperse
        // (cada entrada padea a 4 bytes, como exige la spec).
        let kvd = &ktx2[kvd_ofs..kvd_ofs + kvd_len];
        let mut rest = kvd;
        let mut entries = 0;
        while rest.len() > 4 {
            let len = le32(rest, 0) as usize;
            assert!(len >= 2 && 4 + len <= rest.len(), "entrada KVD válida");
            entries += 1;
            let consumed = 4 + len;
            rest = &rest[consumed + (4 - (consumed % 4)) % 4..];
        }
        assert!(entries >= 2, "KTXwriter y KTXorientation siguen presentes");
        let level_ofs = le64(ktx2, 80) as usize;
        let level_len = le64(ktx2, 88) as usize;
        let level_uncomp = le64(ktx2, 96);
        assert!(
            level_ofs + level_len <= ktx2.len(),
            "nivel dentro del fichero"
        );
        if etc1s {
            let sgd_ofs = le64(ktx2, 64) as usize;
            let sgd_len = le64(ktx2, 72) as usize;
            assert_eq!(sgd_ofs & 7, 0, "SGD alineado a 8");
            assert!(sgd_len > 20, "SGD mayor que su cabecera");
            assert_eq!(sgd_ofs + sgd_len, level_ofs, "el nivel sigue al SGD");
            assert_eq!(level_uncomp, 0, "BasisLZ exige uncompressed = 0");
            let sgd = &ktx2[sgd_ofs..sgd_ofs + sgd_len];
            assert!(le16(sgd, 0) > 0 && le16(sgd, 2) > 0, "paletas no vacías");
            let rgb_len = le32(sgd, 28) as usize;
            let alpha_ofs = le32(sgd, 32) as usize;
            let alpha_len = le32(sgd, 36) as usize;
            assert!(rgb_len > 0 && rgb_len <= level_len, "slice rgb");
            if alpha {
                assert_eq!(alpha_ofs, rgb_len, "el alfa va tras el rgb");
                assert!(alpha_len > 0 && alpha_ofs + alpha_len <= level_len);
            } else {
                assert_eq!((alpha_ofs, alpha_len), (0, 0), "sin alfa");
            }
        } else {
            assert_eq!(le64(ktx2, 64), 0, "UASTC sin SGD");
            assert_eq!(le64(ktx2, 72), 0, "UASTC sin SGD");
            assert_eq!(level_ofs & 15, 0, "nivel alineado a 16");
            assert_eq!(level_uncomp, level_len as u64, "NONE: igual que byteLength");
            assert_eq!(level_len, 8 * 8 * 16, "UASTC: bloques × 16");
            assert_eq!(kvd_ofs + kvd_len, level_ofs, "KVD y nivel sin huecos");
        }
    }

    #[test]
    fn el_envoltorio_reescribe_etc1s_con_y_sin_alfa() {
        for has_alpha in [false, true] {
            let basis = encode(true, has_alpha);
            let ktx2 = basis_to_ktx2(&basis).unwrap();
            check_container(&ktx2, true, has_alpha);
        }
    }

    #[test]
    fn el_envoltorio_reescribe_uastc_con_y_sin_alfa() {
        for has_alpha in [false, true] {
            let basis = encode(false, has_alpha);
            let ktx2 = basis_to_ktx2(&basis).unwrap();
            check_container(&ktx2, false, has_alpha);
        }
    }

    /// El viaje completo: el KTX2 reconstruye un `.basis` que transcodifica
    /// exactamente a los mismos píxeles que el original.
    #[test]
    fn el_ktx2_leido_transcodifica_igual_que_el_basis_original() {
        for etc1s in [true, false] {
            for has_alpha in [false, true] {
                let basis = encode(etc1s, has_alpha);
                let ktx2 = basis_to_ktx2(&basis).unwrap();
                let rebuilt = ktx2_to_basis(&ktx2).unwrap();
                let (ow, oh, original) = tp_basis::transcode_rgba(&basis).unwrap();
                let (rw, rh, read) = tp_basis::transcode_rgba(&rebuilt).unwrap();
                assert_eq!((ow, oh), (rw, rh), "mismas dimensiones");
                assert_eq!(
                    original, read,
                    "píxeles byte a byte (etc1s={etc1s}, alfa={has_alpha})"
                );
            }
        }
    }

    /// Un DFD que no describe Basis no se cuela por el lector.
    #[test]
    fn un_dfd_ajeno_a_basis_no_se_lee() {
        let mut ktx2 = Vec::new();
        ktx2.extend_from_slice(KTX2_IDENT);
        ktx2.extend_from_slice(&[0u8; 68]); // resto de la cabecera a cero
        ktx2.extend_from_slice(&[0u8; 24]); // índice de niveles
        ktx2.extend_from_slice(&[0u8; 44]); // DFD
        ktx2[20..24].copy_from_slice(&4u32.to_le_bytes()); // width
        ktx2[24..28].copy_from_slice(&4u32.to_le_bytes()); // height
        ktx2[36..40].copy_from_slice(&1u32.to_le_bytes()); // faceCount
        ktx2[48..52].copy_from_slice(&104u32.to_le_bytes()); // dfdByteOffset
        ktx2[52..56].copy_from_slice(&44u32.to_le_bytes()); // dfdByteLength
        ktx2[104] = 44; // dfdTotalSize
        ktx2[104 + 12] = 96; // colorModel RGBSDA, no Basis
        let err = ktx2_to_basis(&ktx2).unwrap_err();
        assert!(err.contains("colorModel 96"), "{err}");
    }

    /// Un `.basis` con la firma rota no se interpreta.
    #[test]
    fn un_basis_con_firma_rota_no_se_envuelve() {
        let err = basis_to_ktx2(b"xx\x13\x00M\x00\x4d\x00").unwrap_err();
        assert!(err.contains("Basis"), "{err}");
    }
}
