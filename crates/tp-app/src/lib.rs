//! TexturePacker-RS desktop application (egui/eframe).
//!
//! El estado de la app vive en la librería para que los binarios (`tp-app`,
//! `tp-smoke`) y las pruebas headless compartan exactamente la misma
//! aplicación, sin duplicar código de UI.

mod app;
mod i18n;
mod ui_prefs;

/// Tamaño mínimo de ventana: `main.rs` no deja encoger la app por debajo
/// de esto y las pruebas de layout (`app::ventana_minima`) ejercitan
/// exactamente ese tamaño, de modo que el prometido y el probado se
/// mueven juntos.
pub const VENTANA_MINIMA: (f32, f32) = (900.0, 600.0);

/// Chequeo de que ningún literal de la UI use un carácter sin glifo (sólo en
/// tests: no aporta código a la app).
#[cfg(test)]
mod glyph_guard;

/// Soporte para pruebas (proyecto de ejemplo headless). `doc(hidden)`: no es
/// API pública de la app, la usan `tp-smoke` y los tests de integración.
#[doc(hidden)]
pub mod testing;

pub use app::{begin_sprite_drag, run_headless, App};
