//! Partes de la CLI: registros y parseo de opciones, textos de ayuda y
//! comandos.

mod args;
mod commands;
mod help;
mod pack;

pub(crate) use commands::cmd_decrypt;
pub(crate) use help::{exporter_list_text, usage, version_line};
pub(crate) use pack::cmd_pack;

/// Resultado de una acción de la CLI: los errores se devuelven en vez de
/// abortar el proceso, y `main` los imprime con [`fail`] y sale con 1.
pub(crate) type CmdResult<T> = Result<T, String>;

/// Punto único por el que sale un error: aquí es donde el mensaje —que el
/// resto del código construye siempre en español— se traduce al idioma
/// activo. `tr` casará la plantilla con huecos si el mensaje ya venía
/// formateado, así que un error del motor interpolado dentro de uno del
/// CLI sale traducido entero, argumentos incluidos.
pub(crate) fn fail(msg: String) -> ! {
    eprintln!("error: {}", tp_i18n::tr(&msg));
    std::process::exit(1);
}
