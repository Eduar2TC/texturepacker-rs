//! Global event handlers: files dropped from the OS (anywhere on the
//! window) and the keyboard shortcuts.

use super::drag::SpriteDrag;
use super::preview;
use super::{App, LogKind, JUST_ADDED_HL};
use crate::i18n::t;
use eframe::egui;
use std::path::Path;

/// Soltar ficheros del SO en cualquier parte de la ventana: los añade al
/// workspace. Devuelve un overlay azul mientras el arrastre esté sobre la
/// app (feedback estándar) y registra en el Log lo añadido.
/// Un fichero de proyecto que la ventana sabe abrir al soltarlo. Solo las
/// extensiones de proyecto: un `Cargo.toml` o un `ui.toml` sueltos encima
/// no deben reemplazar la configuración cargada.
fn is_project_file(path: &Path) -> bool {
    path.is_file()
        && matches!(
            path.extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("tpproj" | "tps")
        )
}

pub(super) fn handle_global_file_drop(app: &mut App, ctx: &egui::Context) {
    let (hovered, dropped) =
        ctx.input(|i| (i.raw.hovered_files.clone(), i.raw.dropped_files.clone()));
    if !hovered.is_empty() {
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            "drop_overlay".into(),
        ));
        painter.rect_filled(
            screen.shrink(2.0),
            6.0,
            egui::Color32::from_rgba_unmultiplied(90, 180, 255, 28),
        );
        painter.rect_stroke(
            screen.shrink(4.0),
            6.0,
            egui::Stroke::new(
                2.0_f32,
                egui::Color32::from_rgba_unmultiplied(120, 200, 255, 200),
            ),
            egui::StrokeKind::Outside,
        );
        let text = if hovered.len() == 1 {
            t!("Suelta para añadir al workspace").to_string()
        } else {
            t!("Suelta para añadir {} elemento(s)", hovered.len())
        };
        painter.text(
            screen.center() + egui::vec2(0.0, -16.0),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(22.0),
            egui::Color32::from_rgba_unmultiplied(230, 245, 255, 255),
        );
        painter.text(
            screen.center() + egui::vec2(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            t!("(sprites, carpetas o proyectos; los sprites se empaquetan al instante)"),
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgba_unmultiplied(180, 210, 235, 220),
        );
        // El overlay solo existe si hay frames: el hover no pasa por
        // `poll_changes`, así que el siguiente frame se pide aquí.
        ctx.request_repaint();
    }
    if dropped.is_empty() {
        return;
    }
    let total = dropped.len();
    let mut added = 0;
    for file in dropped {
        if let Some(path) = file.path {
            // Un proyecto soltado (`.tpproj` o `.tps`) se abre en la ventana;
            // `add_input` lo descartaría por no ser una imagen.
            if is_project_file(&path) {
                app.open_project(path.clone());
                if app.project_path.as_deref() == Some(path.as_path()) {
                    // El drop corre después de `poll_changes`: se programa el
                    // repack y el frame siguiente para que el atlas salga ya
                    // con el proyecto cargado y no a la siguiente interacción.
                    app.after_workspace_change();
                    ctx.request_repaint();
                }
                continue;
            }
            if app.add_input(path) {
                added += 1;
            }
        }
    }
    if added > 0 {
        app.aviso(LogKind::Info, t!("{} sprite(s) añadido(s).", added));
        // Empaqueta ya, sin esperar al debounce: el lienzo arranca a
        // calcular en el mismo gesto y no en el siguiente round-trip.
        app.request_preview(false);
        // Programa el frame que apagará el resaltado «recién añadido».
        ctx.request_repaint_after(JUST_ADDED_HL);
    } else {
        app.aviso(
            LogKind::Warning,
            t!(
                "Nada nuevo en el workspace ({} fichero(s) soltado(s): \
                 ya estaban o no son imágenes).",
                total
            ),
        );
    }
    // `poll_changes` (que programa el repack) ya corrió AL INICIO de este
    // frame, antes de procesar el drop: sin este repaint la cadena
    // debounce → hilo → resultado moriría aquí y el atlas no se actualizaría
    // hasta la próxima interacción (p. ej. pulsar Publicar).
    ctx.request_repaint();
}

/// Atajos de teclado globales (estilo estándar de herramientas de escritorio):
/// Ctrl+O abrir, Ctrl+S guardar, Ctrl+P publicar, Supr quitar selección,
/// +/-/0 zoom, F ajustar, Esc cierra ventanas flotantes.
pub(super) fn handle_shortcuts(app: &mut App, ctx: &egui::Context) {
    let consume = |ctx: &egui::Context, key: egui::Key| {
        ctx.input(|i| {
            let mods = i.modifiers;
            i.key_pressed(key) && (mods.ctrl || mods.command)
        })
    };
    if consume(ctx, egui::Key::O) {
        app.load_project();
    }
    if consume(ctx, egui::Key::S) {
        app.save_project();
    }
    if consume(ctx, egui::Key::P) && app.running.is_none() {
        app.start_pack(false);
    }

    // Zoom de teclado (sin modificadores, como en Figma/Photoshop):
    // +/- acercan y alejan, 0 restaura 1:1 y F encuadra.
    let (zoom_in, zoom_out, zoom_reset, zoom_fit) = ctx.input(|i| {
        let blocked = i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt;
        (
            !blocked && i.key_pressed(egui::Key::Equals),
            !blocked && i.key_pressed(egui::Key::Minus),
            !blocked && i.key_pressed(egui::Key::Num0),
            !blocked && i.key_pressed(egui::Key::F),
        )
    });
    if zoom_in {
        preview::zoom_step(app, 1);
    }
    if zoom_out {
        preview::zoom_step(app, -1);
    }
    if zoom_reset {
        app.zoom = 1.0;
        app.auto_fit = false;
    }
    if zoom_fit {
        app.fit_zoom();
        app.auto_fit = false;
    }

    // F1 o «?» conmutan la ayuda de atajos: la única forma de descubrir el
    // teclado desde dentro de la app (I2). «?» no se atiende mientras se
    // escribe en un campo, para no dejar un «?» huérfano en el texto.
    let (ayuda, ayuda_tecla) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::F1),
            i.key_pressed(egui::Key::Questionmark),
        )
    });
    if ayuda || (ayuda_tecla && !ctx.wants_keyboard_input()) {
        app.show_shortcuts = !app.show_shortcuts;
    }

    // Esc cierra la ventana flotante activa (convención estándar) y, de
    // paso, devuelve el foco de teclado de la lista de sprites al ratón.
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if esc {
        app.tree_kb_focus = false;
        if app.show_about {
            app.show_about = false;
        } else if app.show_shortcuts {
            app.show_shortcuts = false;
        } else if app.show_animation {
            app.show_animation = false;
        } else if app.show_split {
            app.show_split = false;
        } else if app.show_sprite_settings {
            app.show_sprite_settings = false;
        }
    }

    // Cursor de «copiando» mientras se arrastra un sprite del árbol hacia
    // el lienzo o una hoja (feedback estándar de arrastrar-y-soltar).
    if SpriteDrag::has_payload(ctx) {
        ctx.set_cursor_icon(egui::CursorIcon::Copy);
    }
}
