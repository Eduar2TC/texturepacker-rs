//! Sprite drag & drop: payload shared by every destination, the placement
//! plan shown by the ghost and the drop handler of the canvas.

use super::{App, LogKind};
use crate::i18n::t;
use eframe::egui;
use std::path::PathBuf;
use tp_core::config::ProjectConfig;

/// Umbral mínimo de arrastre (px) para decisiones de interacción propias:
/// la zona vacía del lienzo no inicia marquee hasta superar esta distancia
/// (evita marquesinas accidentales con micro-movimientos; egui ya exige
/// `max_click_dist` = 6 px para convertir un press en drag).
pub(crate) const DRAG_THRESHOLD_PX: f32 = 6.0;

/// Tamaño mínimo en pantalla del fantasma de arrastre (px): a zoom bajo un
/// sprite de 8 px sería invisible; se dibuja al menos tan grande como esto.
pub(crate) const GHOST_MIN_SIZE_PX: f32 = 24.0;

/// Arrastre de sprites del panel izquierdo hacia el lienzo: los ids
/// arrastrados (multi-selección incluida) y el frame visible de cada uno
/// (para el fantasma y el plan de colocación). Viaja como payload de
/// DragAndDrop.
pub(crate) struct SpriteDrag {
    pub ids: Vec<String>,
    pub first_frame: Option<tp_core::types::Rect>,
    /// Frame visible de cada id arrastrado (para previsualizar la
    /// disposición completa de la multi-selección).
    pub frames: std::collections::BTreeMap<String, tp_core::types::Rect>,
}

impl SpriteDrag {
    pub(crate) fn payload(ctx: &egui::Context) -> Option<std::sync::Arc<SpriteDrag>> {
        egui::DragAndDrop::payload::<SpriteDrag>(ctx)
    }

    pub(crate) fn clear(ctx: &egui::Context) {
        egui::DragAndDrop::clear_payload(ctx);
    }

    /// ¿Hay un arrastre de sprites en marcha (cualquier tipo de destino)?
    pub(crate) fn has_payload(ctx: &egui::Context) -> bool {
        egui::DragAndDrop::has_payload_of_type::<SpriteDrag>(ctx)
    }

    /// Recupera y consume el payload (para destinos que no sean el lienzo).
    pub(crate) fn take(ctx: &egui::Context) -> Option<std::sync::Arc<SpriteDrag>> {
        egui::DragAndDrop::take_payload::<SpriteDrag>(ctx)
    }
}

/// Registra el inicio de un arrastre de sprites (desde el árbol) con los ids
/// arrastrados: si la fila arrastrada pertenece a la selección multi, viajan
/// todos los seleccionados.
/// Inicia un arrastre de sprites desde el árbol hacia el lienzo. `pub`
/// (no solo `pub(crate)`) para que los tests de integración conduzcan el
/// mismo camino exacto que la UI.
pub fn begin_sprite_drag(app: &App, ctx: &egui::Context, mut ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    ids.sort();
    let mut frames = std::collections::BTreeMap::new();
    if let Some(out) = app.result.as_ref() {
        for s in &out.result.sprites {
            if ids.contains(&s.id) && !s.is_alias {
                frames.insert(s.id.clone(), s.visible_frame);
            }
        }
    }
    let first_frame = ids.iter().find_map(|id| frames.get(id).copied());
    // OJO: set_payload ya envuelve en Arc internamente; pasar el struct
    // directamente (no un Arc propio) para que payload::<SpriteDrag>()
    // haga downcast correctamente.
    egui::DragAndDrop::set_payload(
        ctx,
        SpriteDrag {
            ids,
            first_frame,
            frames,
        },
    );
}

impl App {
    /// Suelta sprites arrastrados del panel sobre el lienzo: activa el
    /// algoritmo Manual si hace falta, fija las posiciones relativas entre
    /// sí (con el mismo snap y clamp que promete el fantasma) y repacktua
    /// al instante. Devuelve cuántos sprites se colocaron.
    pub fn drop_sprites_on_canvas(&mut self, atlas_pos: egui::Pos2) -> usize {
        let Some(drag) = SpriteDrag::payload(&self.egui_ctx) else {
            return 0;
        };
        let ids = drag.ids.clone();
        let frames = drag.frames.clone();
        let first_frame = drag.first_frame;
        SpriteDrag::clear(&self.egui_ctx);
        if ids.is_empty() {
            return 0;
        }
        // Se necesitan frames conocidos: sin resultado previo no hay nada
        // que colocar (los sprites recién añadidos se empaquetan solos).
        if self.result.is_none() {
            self.aviso(
                LogKind::Warning,
                t!("Espera a que la vista se calcule antes de colocar sprites.").into(),
            );
            return 0;
        }

        // Activar Manual si no lo está: el usuario está componiendo a mano.
        if self.config.effective_algorithm() != tp_core::config::PackingAlgorithm::Manual {
            self.config.algorithm = tp_core::config::PackingAlgorithm::Manual;
            self.log(
                LogKind::Info,
                t!("Algoritmo cambiado a Manual: los sprites soltados fijan su posición.").into(),
            );
        }

        // Fantasma y drop comparten el plan: lo que se ve es lo que queda.
        let placed_list =
            Self::plan_canvas_drop(&ids, &frames, first_frame, atlas_pos, &self.config);
        for (id, (px, py)) in &placed_list {
            self.config.manual_positions.insert(id.clone(), (*px, *py));
        }
        let placed = placed_list.len();
        let first_pos = placed_list.first().map(|(_, p)| *p);

        if placed > 0 {
            // Selección visible: los colocados quedan seleccionados.
            self.selected_paths.clear();
            for id in &ids {
                if let Some(s) = self
                    .result
                    .as_ref()
                    .and_then(|o| o.result.sprites.iter().find(|s| &s.id == id))
                {
                    self.selected_paths.insert(PathBuf::from(&s.source_path));
                }
            }
            let n = placed;
            let (px, py) = first_pos.unwrap_or((0, 0));
            self.selected_sprite = ids
                .iter()
                .find(|id| self.config.manual_positions.contains_key(*id))
                .cloned();
            self.aviso(
                LogKind::Info,
                t!(
                    "{} sprite(s) colocado(s) en ({}, {}): se reempaqueta al instante.",
                    n,
                    px,
                    py
                ),
            );
            self.pending_force = true;
            self.after_workspace_change();
        }
        placed
    }

    /// Plan de colocación de un drop en el lienzo: para cada id arrastrado
    /// colocado (no alias, con frame conocido), la posición manual `(x, y)`
    /// resultante. Único camino para el fantasma del arrastre y para el drop
    /// real: el fantasma dibuja exactamente estas posiciones.
    ///
    /// - El frame del sprite primero queda bajo el cursor (esquina superior
    ///   izquierda), menos borde/padding del atlas: el píxel, no la caja.
    /// - La disposición relativa entre los sprites se conserva.
    /// - Con rejilla Manual activa, el anclaje se imanta al paso (el motor
    ///   aplica el mismo snap al empaquetar: consistencia garantizada).
    /// - Cada sprite se clampea para que quepa dentro del lienzo (igual que
    ///   el arrastre en vivo: soltar en el borde no sale del atlas).
    pub fn plan_canvas_drop(
        ids: &[String],
        frames: &std::collections::BTreeMap<String, tp_core::types::Rect>,
        first_frame: Option<tp_core::types::Rect>,
        atlas_pos: egui::Pos2,
        config: &ProjectConfig,
    ) -> Vec<(String, (i32, i32))> {
        let (bp, pad) = (config.border_padding.max(0), config.padding.max(0));
        let mut anchor_x = (atlas_pos.x as i32 - bp - pad).max(0);
        let mut anchor_y = (atlas_pos.y as i32 - bp - pad).max(0);
        if let Some(g) = &config.manual_grid {
            let (sx, sy) = g.snap_pos((anchor_x, anchor_y));
            anchor_x = sx;
            anchor_y = sy;
        }
        let frame0 = first_frame.unwrap_or(tp_core::types::Rect::new(0, 0, 0, 0));
        let origin = (frame0.x, frame0.y);
        let canvas_w = if config.fixed_width > 0 {
            config.fixed_width
        } else {
            config.max_texture_size
        };
        let canvas_h = if config.fixed_height > 0 {
            config.fixed_height
        } else {
            config.max_texture_size
        };

        let mut out = Vec::new();
        for id in ids {
            // Los aliases no ocupan frame propio: quedan donde el motor los
            // ponga (superpuestos a su objetivo).
            let Some(frame) = frames.get(id) else {
                continue;
            };
            let px = anchor_x + frame.x - origin.0;
            let py = anchor_y + frame.y - origin.1;
            let max_x = (canvas_w - 2 * bp - (frame.width + 2 * pad)).max(0);
            let max_y = (canvas_h - 2 * bp - (frame.height + 2 * pad)).max(0);
            out.push((id.clone(), (px.clamp(0, max_x), py.clamp(0, max_y))));
        }
        out
    }
}

#[cfg(test)]
mod drag_payload_tests {
    use super::*;

    #[test]
    fn payload_set_then_read_without_frames() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        begin_sprite_drag(&app, &ctx, vec!["a".into(), "b".into()]);
        let got = SpriteDrag::payload(&ctx);
        assert!(
            got.is_some(),
            "payload debe sobrevivir sin frames intermedios"
        );
        SpriteDrag::clear(&ctx);
        assert!(SpriteDrag::payload(&ctx).is_none());
        let _ = &mut app;
    }

    #[test]
    fn payload_survives_a_frame_without_release() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        begin_sprite_drag(&app, &ctx, vec!["a".into()]);
        let _ = app.run_frame(&ctx, egui::RawInput::default());
        assert!(
            SpriteDrag::payload(&ctx).is_some(),
            "payload debe sobrevivir a un frame sin release"
        );
    }
}
