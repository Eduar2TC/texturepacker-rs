//! Floating "Animation preview" window: plays the selected sprites when there
//! is a selection, otherwise the frames of every sprite group.

use super::App;
use crate::i18n::t;
use eframe::egui;
use tp_core::types::Rect;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Background {
    Dark,
    Light,
    Checker,
    /// Color elegido por el usuario (el original deja fijar el fondo de la
    /// ventana de animación a un color cualquiera).
    Custom(egui::Color32),
}

impl Background {
    fn label(self) -> &'static str {
        match self {
            Background::Dark => "Oscuro",
            Background::Light => "Claro",
            Background::Checker => "Damas",
            Background::Custom(_) => "Personalizado",
        }
    }

    /// Color con el que se rellena el lienzo (para el color libre la
    /// variante lleva su valor).
    fn color(self) -> egui::Color32 {
        match self {
            Background::Dark => egui::Color32::from_gray(30),
            Background::Light => egui::Color32::from_gray(210),
            Background::Checker => egui::Color32::from_gray(70),
            Background::Custom(c) => c,
        }
    }
}

#[derive(Clone)]
pub(super) struct AnimState {
    pub playing: bool,
    pub fps: f32,
    pub frame: usize,
    pub loop_anim: bool,
    pub background: Background,
    pub scale: f32,
    /// Selected animation group (id prefix). Empty = every sprite.
    pub group: String,
    accumulator: f32,
}

impl Default for AnimState {
    fn default() -> Self {
        Self {
            playing: false,
            fps: 12.0,
            frame: 0,
            loop_anim: true,
            background: Background::Dark,
            scale: 1.0,
            group: String::new(),
            accumulator: 0.0,
        }
    }
}

/// A frame ready to be painted (data copied out of the pipeline result).
struct Frame {
    page: usize,
    rect: Rect,
    rotated: bool,
    page_w: i32,
    page_h: i32,
    name: String,
}

/// Split `idle_00` into (`idle_`, Some(0)). No trailing digits → whole name.
fn split_name(id: &str) -> (String, Option<u64>) {
    let digits_start = id
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .map(|(i, _)| i)
        .min()
        .unwrap_or(id.len());
    if digits_start == 0 || digits_start == id.len() {
        return (id.to_string(), None);
    }
    let idx = id[digits_start..].parse::<u64>().ok();
    (id[..digits_start].to_string(), idx)
}

pub(super) fn animation_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_animation {
        return;
    }
    let mut open = app.show_animation;
    egui::Window::new(t!("Vista previa de animación"))
        .id(egui::Id::new("animation_preview"))
        .open(&mut open)
        .default_size([380.0, 340.0])
        .min_size([300.0, 240.0])
        .resizable(true)
        .show(ctx, |ui| {
            animation_ui(app, ctx, ui);
        });
    app.show_animation = open;
}

fn animation_ui(app: &mut App, ctx: &egui::Context, ui: &mut egui::Ui) {
    let frames = collect_frames(app);
    if frames.is_empty() {
        let msg = if app.selected_paths.is_empty() {
            t!("No hay sprites empaquetados.\nAñade sprites y pulsa «Publicar».")
        } else {
            t!("Ningún sprite de la selección está publicado.\nPulsa «Publicar» o quita la selección.")
        };
        ui.label(egui::RichText::new(msg).weak());
        return;
    }

    // Advance the playback clock.
    let dt = ctx.input(|i| i.stable_dt);
    if app.anim.playing && frames.len() > 1 {
        app.anim.accumulator += dt;
        let step = 1.0 / app.anim.fps.max(1.0);
        while app.anim.accumulator >= step {
            app.anim.accumulator -= step;
            if app.anim.frame + 1 < frames.len() {
                app.anim.frame += 1;
            } else if app.anim.loop_anim {
                app.anim.frame = 0;
            } else {
                app.anim.playing = false;
                app.anim.accumulator = 0.0;
                break;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(15));
    }
    if app.anim.frame >= frames.len() {
        app.anim.frame = 0;
    }

    // --- group selector -------------------------------------------------
    let groups = groups_of(&frames);
    // El grupo elegido debe seguir existiendo: con selección cambia lo que se
    // reproduce, así que un grupo ya ausente se reinicia al primero.
    if let Some(g) = effective_group(&groups, &app.anim.group) {
        if g != app.anim.group {
            app.anim.group = g;
            app.anim.frame = 0;
        }
    }
    if !app.selected_paths.is_empty() {
        ui.label(
            egui::RichText::new(t!(
                "Secuencia: selección ({} sprite(s)). Quita la selección para ver todos.",
                frames.len()
            ))
            .weak(),
        );
    }
    if groups.len() > 1 {
        ui.horizontal(|ui| {
            ui.label(t!("Animación:"));
            let mut current = app.anim.group.clone();
            egui::ComboBox::from_id_salt("anim_group")
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for g in &groups {
                        ui.selectable_value(&mut current, g.clone(), g);
                    }
                });
            if current != app.anim.group {
                app.anim.group = current;
                app.anim.frame = 0;
            }
        });
    }
    let group_label = if app.anim.group.is_empty() {
        groups.first().cloned().unwrap_or_default()
    } else {
        app.anim.group.clone()
    };

    let frames: Vec<Frame> = frames
        .into_iter()
        .filter(|(g, _)| app.anim.group.is_empty() || *g == app.anim.group)
        .map(|(_, f)| f)
        .collect();
    if frames.is_empty() {
        ui.label(egui::RichText::new(t!("Ese grupo no tiene fotogramas.")).weak());
        return;
    }
    if app.anim.frame >= frames.len() {
        app.anim.frame = 0;
    }
    let current = &frames[app.anim.frame];

    // --- transport controls --------------------------------------------
    ui.horizontal(|ui| {
        let play_label = if app.anim.playing { "⏸" } else { "▶" };
        if ui
            .add_enabled(frames.len() > 1, egui::Button::new(play_label))
            .on_hover_text(if app.anim.playing {
                t!("Pausar")
            } else {
                t!("Reproducir")
            })
            .clicked()
        {
            app.anim.playing = !app.anim.playing;
            app.anim.accumulator = 0.0;
        }
        if ui
            .button("⏮")
            .on_hover_text(t!("Primer fotograma"))
            .clicked()
        {
            app.anim.playing = false;
            app.anim.frame = 0;
        }
        if ui
            .button("⏭")
            .on_hover_text(t!("Último fotograma"))
            .clicked()
        {
            app.anim.playing = false;
            app.anim.frame = frames.len() - 1;
        }

        let mut frame = app.anim.frame;
        if ui
            .add(
                egui::Slider::new(&mut frame, 0..=frames.len() - 1)
                    .integer()
                    .text(t!("Fotograma")),
            )
            .changed()
        {
            app.anim.frame = frame;
            app.anim.accumulator = 0.0;
        }
        ui.label(format!("{}/{}", app.anim.frame + 1, frames.len()));
    });

    ui.horizontal(|ui| {
        ui.add(
            egui::Slider::new(&mut app.anim.fps, 1.0..=60.0)
                .integer()
                .text("FPS"),
        );
        ui.checkbox(&mut app.anim.loop_anim, t!("Repetir"));
        ui.checkbox(&mut app.anim.playing, t!("Reproducir"));
    });

    ui.horizontal(|ui| {
        ui.label(t!("Fondo:"));
        egui::ComboBox::from_id_salt("anim_bg")
            .selected_text(crate::i18n::translate(app.anim.background.label()))
            .show_ui(ui, |ui| {
                for bg in [Background::Dark, Background::Light, Background::Checker] {
                    ui.selectable_value(
                        &mut app.anim.background,
                        bg,
                        crate::i18n::translate(bg.label()),
                    );
                }
                // Color libre: arranca en gris medio (o en el elegido).
                let custom = match app.anim.background {
                    Background::Custom(c) => c,
                    _ => egui::Color32::from_gray(127),
                };
                ui.selectable_value(
                    &mut app.anim.background,
                    Background::Custom(custom),
                    crate::i18n::translate(Background::Custom(custom).label()),
                );
            });
        if let Background::Custom(color) = &mut app.anim.background {
            ui.color_edit_button_srgba(color)
                .on_hover_text(t!("Color de fondo de la vista de animación"));
        }
        ui.add(
            egui::Slider::new(&mut app.anim.scale, 0.25..=8.0)
                .logarithmic(true)
                .text(t!("Escala")),
        );
    });

    super::separador(ui);
    ui.label(egui::RichText::new(format!("{} — {}", current.name, group_label)).weak());

    // --- canvas ---------------------------------------------------------
    // The canvas always uses the free space of the window: zooming scales the
    // sprite inside it (clipped) instead of growing the window itself.
    let available = ui.available_size().max(egui::vec2(64.0, 48.0));
    let (fw, fh) = frame_dims(current);
    let fit = {
        let sx = (available.x - 8.0) / fw as f32;
        let sy = (available.y - 8.0) / fh as f32;
        sx.min(sy).clamp(0.05, 64.0)
    };
    let scale = fit * app.anim.scale;
    let (rect, _) = ui.allocate_exact_size(available, egui::Sense::hover());

    let painter = ui.painter_at(rect);
    match app.anim.background {
        Background::Checker => {
            let cell = 8.0;
            let mut y = rect.min.y;
            let mut row = 0usize;
            while y < rect.max.y {
                let mut x = rect.min.x;
                let mut col = 0usize;
                while x < rect.max.x {
                    let c = if (row + col).is_multiple_of(2) {
                        egui::Color32::from_gray(70)
                    } else {
                        egui::Color32::from_gray(110)
                    };
                    let r = egui::Rect::from_min_max(
                        egui::pos2(x, y),
                        egui::pos2((x + cell).min(rect.max.x), (y + cell).min(rect.max.y)),
                    );
                    painter.rect_filled(r, 0.0, c);
                    x += cell;
                    col += 1;
                }
                y += cell;
                row += 1;
            }
        }
        bg => {
            painter.rect_filled(rect, 0.0, bg.color());
        }
    }

    let sprite = egui::Rect::from_center_size(
        rect.center(),
        egui::vec2(fw as f32 * scale, fh as f32 * scale),
    );
    if current.page < app.textures.len() {
        let tex = &app.textures[current.page];
        if current.rotated {
            draw_rotated(&painter, tex.id(), sprite, current, scale);
        } else {
            let uv = full_uv(current);
            painter.image(tex.id(), sprite, uv, egui::Color32::WHITE);
        }
    }
}

/// All packable frames as `(group, frame)`, grouped and sorted by index.
///
/// Con selección en el panel izquierdo devuelve solo esos sprites (así lo
/// promete el botón «▶ Animación»); sin selección, todos. La agrupación por
/// prefijo se mantiene en ambos casos, así que una selección de un solo
/// prefijo se reproduce como una sola secuencia.
fn collect_frames(app: &App) -> Vec<(String, Frame)> {
    let Some(out) = &app.result else {
        return Vec::new();
    };
    let page_dims: Vec<(i32, i32)> = out.pages.iter().map(|p| (p.width, p.height)).collect();
    // Selección por ruta de origen (la misma comprobación que
    // `App::selected_sprite_indices`).
    let selected: Option<std::collections::HashSet<usize>> = if app.selected_paths.is_empty() {
        None
    } else {
        Some(
            out.result
                .sprites
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    app.selected_paths
                        .contains(std::path::Path::new(&s.source_path))
                })
                .map(|(i, _)| i)
                .collect(),
        )
    };
    let mut frames: Vec<(String, Option<u64>, usize, Frame)> = out
        .result
        .sprites
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            if let Some(sel) = &selected {
                if !sel.contains(&i) {
                    return None;
                }
            }
            let page = s.atlas_page_index as usize;
            let (page_w, page_h) = *page_dims.get(page)?;
            if page >= app.textures.len() {
                return None;
            }
            let (group, index) = split_name(&s.id);
            Some((
                group,
                index,
                i,
                Frame {
                    page,
                    rect: s.visible_frame,
                    rotated: s.is_rotated,
                    page_w,
                    page_h,
                    name: s.id.clone(),
                },
            ))
        })
        .collect();
    frames.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    frames.into_iter().map(|(g, _, _, f)| (g, f)).collect()
}

fn groups_of(frames: &[(String, Frame)]) -> Vec<String> {
    let mut groups: Vec<String> = Vec::new();
    for (g, _) in frames {
        if !groups.contains(g) {
            groups.push(g.clone());
        }
    }
    groups
}

/// Grupo a reproducir: el actual sigue valiendo solo si la lista lo contiene
/// (con selección la lista cambia); si no, el primero. `None` sin grupos.
fn effective_group(groups: &[String], current: &str) -> Option<String> {
    if groups.is_empty() {
        return None;
    }
    if current.is_empty() || !groups.iter().any(|g| g == current) {
        Some(groups[0].clone())
    } else {
        Some(current.to_string())
    }
}

impl App {
    /// Ids de los fotogramas que reproduciría ahora la vista previa de
    /// animación, en orden de reproducción e ignorando el filtro de grupo:
    /// con selección en el panel, solo los sprites seleccionados.
    pub fn animation_frame_ids(&self) -> Vec<String> {
        collect_frames(self)
            .into_iter()
            .map(|(_, f)| f.name)
            .collect()
    }
}

/// Untrimmed size of a frame in sprite orientation.
fn frame_dims(f: &Frame) -> (i32, i32) {
    if f.rotated {
        (f.rect.height, f.rect.width)
    } else {
        (f.rect.width, f.rect.height)
    }
}

fn full_uv(f: &Frame) -> egui::Rect {
    let x0 = f.rect.x as f32 / f.page_w as f32;
    let y0 = f.rect.y as f32 / f.page_h as f32;
    let x1 = (f.rect.x + f.rect.width) as f32 / f.page_w as f32;
    let y1 = (f.rect.y + f.rect.height) as f32 / f.page_h as f32;
    egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
}

/// Draw a 90°-CW packed sprite upright using a mesh with per-corner UVs.
fn draw_rotated(
    painter: &egui::Painter,
    tex: egui::TextureId,
    dst: egui::Rect,
    f: &Frame,
    _scale: f32,
) {
    // Local sprite (W×H) is packed as an (H×W) footprint at (x, y):
    // local (lx, ly) → atlas (x + H - ly, y + lx).
    let (w, h) = frame_dims(f);
    let (x, y) = (f.rect.x as f32, f.rect.y as f32);
    let (pw, ph) = (f.page_w as f32, f.page_h as f32);
    let (h_f, w_f) = (h as f32, w as f32);

    let uv = |ax: f32, ay: f32| egui::pos2((x + ax) / pw, (y + ay) / ph);
    // Screen TL ← local (0,0) → atlas (H, 0)
    // Screen TR ← local (W,0) → atlas (H, W)
    // Screen BR ← local (W,H) → atlas (0, W)
    // Screen BL ← local (0,H) → atlas (0, 0)
    let corners = [
        (dst.min, uv(h_f, 0.0)),
        (egui::pos2(dst.max.x, dst.min.y), uv(h_f, w_f)),
        (dst.max, uv(0.0, w_f)),
        (egui::pos2(dst.min.x, dst.max.y), uv(0.0, 0.0)),
    ];
    let color = egui::Color32::WHITE;
    let vertices: Vec<egui::epaint::Vertex> = corners
        .iter()
        .map(|(pos, uv)| egui::epaint::Vertex {
            pos: *pos,
            uv: *uv,
            color,
        })
        .collect();
    let mesh = egui::Mesh {
        vertices,
        indices: vec![0, 1, 2, 0, 2, 3],
        texture_id: tex,
    };
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_name_groups_by_trailing_digits() {
        assert_eq!(split_name("idle_00"), ("idle_".to_string(), Some(0)));
        assert_eq!(split_name("walk_12"), ("walk_".to_string(), Some(12)));
        assert_eq!(split_name("hero2"), ("hero".to_string(), Some(2)));
    }

    #[test]
    fn split_name_keeps_names_without_trailing_digits() {
        assert_eq!(split_name("badge"), ("badge".to_string(), None));
        assert_eq!(
            split_name("button_hover"),
            ("button_hover".to_string(), None)
        );
        assert_eq!(split_name(""), ("".to_string(), None));
    }

    #[test]
    fn split_name_all_digit_names_stay_unchanged() {
        assert_eq!(split_name("007"), ("007".to_string(), None));
        assert_eq!(split_name("0"), ("0".to_string(), None));
    }

    #[test]
    fn frame_dims_swaps_size_when_rotated() {
        let f = Frame {
            page: 0,
            rect: Rect::new(3, 5, 40, 20),
            rotated: false,
            page_w: 512,
            page_h: 512,
            name: "a".to_string(),
        };
        assert_eq!(frame_dims(&f), (40, 20));
        let r = Frame { rotated: true, ..f };
        assert_eq!(frame_dims(&r), (20, 40));
    }

    #[test]
    fn background_labels_are_spanish() {
        assert_eq!(Background::Dark.label(), t!("Oscuro"));
        assert_eq!(Background::Light.label(), t!("Claro"));
        assert_eq!(Background::Checker.label(), t!("Damas"));
        assert_eq!(
            Background::Custom(egui::Color32::from_rgb(1, 2, 3)).label(),
            t!("Personalizado")
        );
    }

    #[test]
    fn background_colors_keep_their_palette() {
        assert_eq!(
            Background::Dark.color(),
            egui::Color32::from_gray(30),
            "el fondo oscuro no cambia de tono"
        );
        assert_eq!(Background::Light.color(), egui::Color32::from_gray(210));
        let c = egui::Color32::from_rgb(200, 30, 60);
        assert_eq!(
            Background::Custom(c).color(),
            c,
            "el color libre se respeta"
        );
    }

    #[test]
    fn effective_group_falls_back_when_the_group_is_gone() {
        let groups = vec!["idle_".to_string(), "walk_".to_string()];
        // Elegido y presente → se respeta (también si viene vacío, que es el
        // estado inicial: se adopta el primero).
        assert_eq!(effective_group(&groups, "walk_").as_deref(), Some("walk_"));
        assert_eq!(effective_group(&groups, "").as_deref(), Some("idle_"));
        // Elegido pero ausente (cambió la selección) → primero.
        assert_eq!(effective_group(&groups, "run_").as_deref(), Some("idle_"));
        // Sin grupos → nada que reproducir.
        assert_eq!(effective_group(&[], "idle_"), None);
        assert_eq!(effective_group(&[], ""), None);
    }
}
