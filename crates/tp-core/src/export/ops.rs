use crate::config::GpuFormat;

/// Escribe la resolución pedida (`--dpi`) en un PNG ya codificado, dejando
/// intacto el resto de formatos (no tienen un campo de resolución común).
pub fn apply_dpi(bytes: Vec<u8>, format: GpuFormat, dpi: Option<u32>) -> Vec<u8> {
    match (dpi, format) {
        (Some(dpi), GpuFormat::Png | GpuFormat::Png8) => insert_phys(bytes, dpi),
        _ => bytes,
    }
}

/// Inserta el chunk `pHYs` tras la cabecera `IHDR` de un PNG.
///
/// Los codificadores que usamos no escriben esa resolución, así que basta con
/// añadirla; `dpi` pulgadas se convierten a metros (1 in = 0,0254 m), que es
/// la unidad del chunk.
fn insert_phys(mut png: Vec<u8>, dpi: u32) -> Vec<u8> {
    const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
    const IHDR_LEN: usize = 8 + 4 + 4 + 13 + 4;
    if png.len() < IHDR_LEN || png[..8] != SIGNATURE {
        return png;
    }
    let per_meter = ((f64::from(dpi) / 0.0254).round() as u32).max(1);
    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9u32.to_be_bytes());
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&per_meter.to_be_bytes());
    chunk.extend_from_slice(&per_meter.to_be_bytes());
    chunk.push(1); // unidad: metro
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    png.splice(IHDR_LEN..IHDR_LEN, chunk);
    png
}

/// CRC-32 (polinomio 0xEDB88320) del cuerpo de un chunk PNG.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Voltea verticalmente un buffer RGBA (`--flip-y`; solo formatos de
/// hardware — las coordenadas de los frames no cambian).
pub fn flip_vertical_rgba(rgba: &mut [u8], width: usize, height: usize) {
    let stride = width * 4;
    if stride == 0 || rgba.len() < stride * height {
        return;
    }
    for y in 0..height / 2 {
        let top = y * stride;
        let bottom = (height - 1 - y) * stride;
        for i in 0..stride {
            rgba.swap(top + i, bottom + i);
        }
    }
}

/// Contorno que dibuja `--shape-debug` sobre la hoja publicada.
#[derive(Debug, Clone, Default)]
pub struct DebugShape {
    /// Rectángulo visible del sprite `[x, y, w, h]` en píxeles del lienzo.
    pub rect: [i32; 4],
    /// Contornos poligonales ya proyectados al lienzo (píxeles).
    pub contours: Vec<Vec<[f32; 2]>>,
}

/// Lienzo RGBA sobre el que dibuja `--shape-debug`, recortando al tamaño
/// de la hoja.
struct ShapeCanvas<'a> {
    rgba: &'a mut [u8],
    width: usize,
    height: usize,
}

impl ShapeCanvas<'_> {
    fn put(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.width || y >= self.height {
            return;
        }
        let i = (y * self.width + x) * 4;
        if let Some(px) = self.rgba.get_mut(i..i + 4) {
            px.copy_from_slice(&color);
        }
    }

    /// Línea de Bresenham de 1 px.
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 4]) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            self.put(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
}

/// Dibuja el contorno de cada sprite (rectángulo visible + polígonos) en
/// magenta a 1 px sobre la hoja, para depurar el reparto. Los trazos fuera
/// del lienzo se recortan.
pub fn draw_shape_debug(rgba: &mut [u8], width: usize, height: usize, shapes: &[DebugShape]) {
    const PINK: [u8; 4] = [255, 0, 255, 255];
    let mut canvas = ShapeCanvas {
        rgba,
        width,
        height,
    };
    for shape in shapes {
        let [x, y, w, h] = shape.rect;
        if w <= 0 || h <= 0 {
            continue;
        }
        let (x2, y2) = (x + w - 1, y + h - 1);
        canvas.line(x, y, x2, y, PINK);
        canvas.line(x, y2, x2, y2, PINK);
        canvas.line(x, y, x, y2, PINK);
        canvas.line(x2, y, x2, y2, PINK);
        for contour in &shape.contours {
            if contour.len() < 2 {
                continue;
            }
            for i in 0..contour.len() {
                let a = contour[i];
                let b = contour[(i + 1) % contour.len()];
                canvas.line(
                    a[0].round() as i32,
                    a[1].round() as i32,
                    b[0].round() as i32,
                    b[1].round() as i32,
                    PINK,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_vertical_helper_reverses_rows() {
        // 2x2 RGBA: fila 0 rojo, fila 1 azul.
        let mut rgba = vec![
            255, 0, 0, 255, 255, 0, 0, 255, // fila 0
            0, 0, 255, 255, 0, 0, 255, 255, // fila 1
        ];
        flip_vertical_rgba(&mut rgba, 2, 2);
        assert_eq!(&rgba[..8], &[0, 0, 255, 255, 0, 0, 255, 255]);
        assert_eq!(&rgba[8..], &[255, 0, 0, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn shape_debug_draws_the_borders_and_keeps_the_interior() {
        let (w, h) = (16, 16);
        let mut rgba = vec![0u8; w * h * 4];
        let shapes = vec![DebugShape {
            rect: [4, 4, 6, 6],
            contours: vec![vec![[6.0, 6.0], [9.0, 6.0], [9.0, 9.0]]],
        }];
        draw_shape_debug(&mut rgba, w, h, &shapes);
        let at = |x: usize, y: usize| rgba[(y * w + x) * 4..][..4].to_vec();
        // Esquinas y bordes del rectángulo: magenta.
        assert_eq!(at(4, 4), [255, 0, 255, 255]);
        assert_eq!(at(9, 9), [255, 0, 255, 255]);
        assert_eq!(at(7, 4), [255, 0, 255, 255]);
        // Contorno poligonal (arista horizontal en y=6).
        assert_eq!(at(7, 6), [255, 0, 255, 255]);
        // Interior libre: sigue transparente.
        assert_eq!(at(5, 5), [0, 0, 0, 0]);
        // Fuera del sprite: intacto.
        assert_eq!(at(12, 12), [0, 0, 0, 0]);
    }

    #[test]
    fn shape_debug_clips_out_of_canvas_shapes() {
        let (w, h) = (8, 8);
        let mut rgba = vec![0u8; w * h * 4];
        let shapes = vec![
            // Rectángulo que se sale por la izquierda y por arriba.
            DebugShape {
                rect: [-4, -3, 10, 8],
                contours: vec![vec![[-20.0, -20.0], [30.0, 30.0]]],
            },
            // Rect degenerado: no dibuja ni panic.
            DebugShape {
                rect: [0, 0, 0, 5],
                contours: vec![],
            },
        ];
        draw_shape_debug(&mut rgba, w, h, &shapes);
        assert_eq!(&rgba[..4], &[255, 0, 255, 255], "recorte en el borde");
        // La parte del rectángulo dentro del lienzo se pinta.
        assert_eq!(
            &rgba[(5 * w + 5) * 4..][..4],
            &[255, 0, 255, 255],
            "centro del rect recortado"
        );
    }
}
