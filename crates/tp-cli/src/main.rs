//! TexturePacker-RS command-line interface.
//!
//! ```
//! tp-cli pack project.tpproj
//! tp-cli pack --input sprites/ --output build/ --max-size 4096 --format astc
//! tp-cli decrypt atlas_0.png.tpenc --key secreto -o atlas.png
//! ```

mod cli;

use cli::{cmd_decrypt, cmd_pack, exporter_list_text, fail, usage, version_line};

fn main() {
    // Idioma de los mensajes: el del equipo, leído de LC_ALL/LC_MESSAGES/
    // LANG, que es una sola variable con la que un script (o un runner de
    // CI, que trae LANG=C.UTF-8) decide el idioma de la salida.
    tp_i18n::set_choice(tp_i18n::LangChoice::System);

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        // Sin comando no hay nada que hacer, y no es una petición de
        // ayuda: error de uso a stderr con el código 2 de la convención
        // Unix. Antes salía con la ayuda y exit 0, y un script que no
        // pasara argumentos lo tomaba por un éxito.
        eprintln!(
            "{}",
            tp_i18n::tr("tp-cli: falta un comando (usa tp-cli --help para ver la ayuda)")
        );
        std::process::exit(2);
    }
    if args[0] == "--help" || args[0] == "-h" {
        usage();
    }
    match args[0].as_str() {
        "--version" | "-V" => println!("{}", version_line()),
        "--exporter-list" => print!("{}", exporter_list_text()),
        "pack" => cmd_pack(&args[1..]).unwrap_or_else(|e| fail(e)),
        "decrypt" => cmd_decrypt(&args[1..]),
        other => fail(format!("Comando desconocido: {other}")),
    }
}
