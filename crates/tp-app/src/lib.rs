//! TexturePacker-RS desktop application (egui/eframe).
//!
//! El estado de la app vive en la librería para que los binarios (`tp-app`,
//! `tp-smoke`) y las pruebas headless compartan exactamente la misma
//! aplicación, sin duplicar código de UI.

mod app;
mod i18n;
mod ui_prefs;

/// Chequeo de que ningún literal de la UI use un carácter sin glifo (sólo en
/// tests: no aporta código a la app).
#[cfg(test)]
mod glyph_guard;

/// Soporte para pruebas (proyecto de ejemplo headless). `doc(hidden)`: no es
/// API pública de la app, la usan `tp-smoke` y los tests de integración.
#[doc(hidden)]
pub mod testing;

pub use app::{begin_sprite_drag, run_headless, App};
