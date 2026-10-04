use std::path::PathBuf;

use super::args::{
    check_unknown_options, parse_args, parse_pixel_format, DECRYPT_FLAGS, DECRYPT_VALUES,
};
use super::fail;
use super::help::decrypt_usage;

pub(crate) fn cmd_decrypt(args: &[String]) {
    let (positionals, values, flags) = parse_args(args);
    // `--help` manda sobre cualquier otra cosa, incluso sobre opciones
    // desconocidas: pedir ayuda no debe terminar en un error.
    if flags.iter().any(|f| f == "help") {
        decrypt_usage();
    }
    check_unknown_options(&values, &flags, DECRYPT_VALUES, DECRYPT_FLAGS)
        .unwrap_or_else(|e| fail(e));
    let Some(file) = positionals.first().map(PathBuf::from) else {
        fail("decrypt necesita un archivo .tpenc".into());
    };
    let Some(key) = values.iter().rfind(|(v, _)| v == "key").map(|(_, v)| v) else {
        fail("decrypt necesita --key CLAVE".into());
    };
    let out_path = values
        .iter()
        .find(|(v, _)| v == "o" || v == "out")
        .map(|(_, v)| PathBuf::from(v))
        .unwrap_or_else(|| {
            let mut p = file.clone();
            p.set_extension("dec.png");
            p
        });
    let data = std::fs::read(&file)
        .unwrap_or_else(|e| fail(format!("No se pudo leer {}: {e}", file.display())));
    let plain = tp_core::export::decrypt_bytes(&data, key)
        .unwrap_or_else(|e| fail(format!("Descifrado fallido: {e}")));
    let val = |k: &str| {
        values
            .iter()
            .rfind(|(v, _)| v == k)
            .map(|(_, v)| v.as_str())
    };

    // Vista previa: con `--pixel-format` se re-aplica la conversión del atlas
    // (BGRA8888 y demás) para que la imagen se vea con los colores correctos.
    let preview = match val("pixel-format") {
        Some(pf) => tp_core::export::decode_texture_preview_png(
            &plain,
            parse_pixel_format(pf).unwrap_or_else(|e| fail(e)),
        )
        .unwrap_or_else(|e| fail(format!("No se pudo generar la vista previa: {e}"))),
        None => plain,
    };

    std::fs::write(&out_path, &preview)
        .unwrap_or_else(|e| fail(format!("No se pudo escribir {}: {e}", out_path.display())));
    if flags.iter().any(|f| f == "quiet") {
        return;
    }
    match val("pixel-format") {
        Some(pf) => println!(
            "✔ Descifrado + vista previa ({}): {}",
            pf.to_ascii_uppercase(),
            out_path.display()
        ),
        None => println!("✔ Descifrado: {}", out_path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn decrypt_writes_preview_png_with_bgra_restored() {
        use image::ImageEncoder;

        // Atlas "publicado" con BGRA8888 (R y B intercambiados) y cifrado.
        let bgra = vec![255u8, 0, 0, 255, 0, 0, 255, 255]; // azul, rojo
        let mut atlas = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut atlas,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(&bgra, 2, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
        let enc = tp_core::export::encrypt_bytes(&atlas, "clave").unwrap();
        let f = std::env::temp_dir().join("tpcli_decrypt_preview.tpenc");
        let o = std::env::temp_dir().join("tpcli_decrypt_preview_out.png");
        std::fs::write(&f, &enc).unwrap();

        cmd_decrypt(&args(&[
            f.to_str().unwrap(),
            "--key",
            "clave",
            "-o",
            o.to_str().unwrap(),
            "--pixel-format",
            "bgra8888",
        ]));
        let out = std::fs::read(&o).unwrap();
        let img = image::load_from_memory(&out).unwrap().to_rgba8();
        // Con la conversión, R/B vuelven al orden natural RGBA.
        assert_eq!(img.as_raw(), &[0, 0, 255, 255, 255, 0, 0, 255]);
        let _ = std::fs::remove_file(&f);
        let _ = std::fs::remove_file(f.with_extension("dec.png"));
        let _ = std::fs::remove_file(&o);
    }
}
