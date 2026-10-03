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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
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
