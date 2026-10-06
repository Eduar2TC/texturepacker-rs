//! Deshacer (review UI/UX C6).
//!
//! No existía forma de deshacer: «Restablecer» sustituía toda la
//! configuración, «Limpiar posiciones manuales» borraba el mapa entero y
//! `Supr` quitaba sprites, y lo único inverso era «↺ Restaurar», que sólo
//! recupera exclusiones. Aquí hay una pila de instantáneas de la
//! configuración previa a esas operaciones: un paso por gesto y `Ctrl+Z`
//! para deshacer. No hay rehacer, que es lo mínimo viable que pedía la
//! review.
//!
//! Se guarda la configuración **entera**, no la parte tocada: es lo que
//! comparten las tres operaciones (reset, posiciones manuales y sprites
//! quitados) y sale más barato —y más difícil de romper— que rastrear
//! deltas por campo.

use super::{App, LogKind};
use crate::i18n::t;

/// Pasos guardados. De sobra para despiste largos y, sobre todo, para
/// que la pila no crezca sin límite a base de arrastres en el lienzo.
pub(super) const MAXIMO: usize = 32;

impl App {
    /// Anota la configuración **previa** a una operación deshacible. Se
    /// llama antes de tocar nada: el paso guarda el «antes».
    pub(super) fn anotar_deshacer(&mut self) {
        if self.deshacer.len() >= MAXIMO {
            self.deshacer.remove(0);
        }
        self.deshacer.push(self.config.clone());
    }

    /// Deshace el último paso (`Ctrl+Z`). Sin pasos no hace nada: no es
    /// un error, simplemente no hay nada que deshacer.
    pub(super) fn deshacer(&mut self) {
        let Some(config) = self.deshacer.pop() else {
            return;
        };
        self.config = config;
        // El texto de los campos de Ajustes sigue apuntando al estado que
        // se acaba de borrar: se resincroniza igual que al restablecer.
        self.sync_variants();
        self.sync_paths();
        // Las rutas pueden haber cambiado: el watcher vuelve a mirarlas.
        self.start_watcher();
        self.aviso(LogKind::Info, t!("Cambio deshecho.").into());
        // El árbol, la cola de la vista previa y la bandera «sin guardar»
        // salen de la propia configuración, así que basta con avisar.
        self.after_workspace_change();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    fn app() -> (App, egui::Context) {
        let ctx = egui::Context::default();
        let app = App::new_for_testing(ctx.clone(), None);
        (app, ctx)
    }

    /// C6: «Restablecer» sustituía toda la configuración sin vuelta atrás.
    #[test]
    fn el_reset_se_deshace() {
        let (mut app, _ctx) = app();
        let por_defecto = tp_core::config::ProjectConfig::default();
        app.config.padding = por_defecto.padding + 7;
        app.config.max_texture_size = por_defecto.max_texture_size + 64;

        app.reset_defaults();

        assert_eq!(
            app.config.padding, por_defecto.padding,
            "el reset deja los valores por defecto"
        );
        app.deshacer();
        assert_eq!(
            app.config.padding,
            por_defecto.padding + 7,
            "Ctrl+Z devuelve la configuración previa"
        );
        assert_eq!(
            app.config.max_texture_size,
            por_defecto.max_texture_size + 64,
            "…entera, no sólo el campo que se miraba"
        );
    }

    /// C6: `Supr` quitaba sprites sin vuelta atrás.
    #[test]
    fn quitar_sprites_se_deshace() {
        let (mut app, _ctx) = app();
        app.config.input_directory = std::path::PathBuf::from("/tmp/tp_c6/sprites");
        let sprite = std::path::PathBuf::from("/tmp/tp_c6/sprites/hero.png");
        app.selected_paths.insert(sprite.clone());

        app.remove_selected(std::slice::from_ref(&sprite));

        assert!(
            app.config.excluded_inputs.contains(&sprite),
            "el sprite queda excluido del workspace"
        );
        app.deshacer();
        assert!(
            !app.config.excluded_inputs.contains(&sprite),
            "Ctrl+Z lo vuelve a meter"
        );
    }

    /// C6: «Limpiar posiciones manuales» borraba el mapa entero.
    #[test]
    fn limpiar_posiciones_manuales_se_deshace() {
        let (mut app, _ctx) = app();
        app.config.manual_positions.insert("hero".into(), (10, 20));
        app.config.manual_positions.insert("coin".into(), (90, 40));

        app.limpiar_posiciones_manuales();

        assert!(app.config.manual_positions.is_empty(), "se limpian todas");
        app.deshacer();
        assert_eq!(
            app.config.manual_positions.get("hero").copied(),
            Some((10, 20)),
            "Ctrl+Z devuelve el mapa entero"
        );
        assert_eq!(
            app.config.manual_positions.get("coin").copied(),
            Some((90, 40))
        );
    }

    /// El atajo llega del teclado: `Ctrl+Z` deshace el último gesto.
    #[test]
    fn ctrl_z_desde_el_teclado_deshece() {
        let (mut app, ctx) = app();
        let por_defecto = tp_core::config::ProjectConfig::default();
        app.config.padding = por_defecto.padding + 7;

        app.reset_defaults();

        assert_eq!(
            app.config.padding, por_defecto.padding,
            "el reset ha tenido que surtir efecto"
        );

        let mods = egui::Modifiers::CTRL;
        app.run_frame(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1360.0, 860.0),
                )),
                modifiers: mods,
                events: vec![egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: mods,
                }],
                ..egui::RawInput::default()
            },
        );

        assert_eq!(
            app.config.padding,
            por_defecto.padding + 7,
            "Ctrl+Z deshace lo que el teclado acababa de romper"
        );
    }

    /// La misma tecla pulsada dentro de un campo es del campo: deshacer
    /// la configuración a mitad de escribir una ruta sería un desastre.
    #[test]
    fn mientras_escribes_no_deshece() {
        let (mut app, ctx) = app();
        let por_defecto = tp_core::config::ProjectConfig::default();
        app.config.padding = por_defecto.padding + 7;

        app.reset_defaults();

        ctx.memory_mut(|m| m.request_focus(egui::Id::new("campo_de_ruta")));
        assert!(ctx.wants_keyboard_input(), "el campo tiene el foco");
        let mods = egui::Modifiers::CTRL;
        let _ = ctx.run(
            egui::RawInput {
                modifiers: mods,
                events: vec![egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: mods,
                }],
                ..egui::RawInput::default()
            },
            |ctx_frame| {
                crate::app::events::handle_shortcuts(&mut app, ctx_frame);
            },
        );

        assert_eq!(
            app.config.padding, por_defecto.padding,
            "Ctrl+Z dentro de un campo no debe tocar la configuración"
        );
    }

    /// Sin pasos guardados, deshacer es un no-op: ni panico ni vacía la
    /// configuración que había.
    #[test]
    fn sin_pasos_no_toca_nada() {
        let (mut app, _ctx) = app();
        let por_defecto = tp_core::config::ProjectConfig::default();
        app.config.padding = por_defecto.padding + 7;

        app.deshacer();

        assert_eq!(
            app.config.padding,
            por_defecto.padding + 7,
            "deshacer sin pila no puede inventarse un estado"
        );
    }

    /// La pila tiene tope: un tope roto convertiría cada arrastre en
    /// memoria para siempre.
    #[test]
    fn la_pila_tiene_tope() {
        let (mut app, _ctx) = app();
        for i in 0..(MAXIMO + 8) {
            app.config.padding = i as i32;
            app.anotar_deshacer();
        }
        assert_eq!(app.deshacer.len(), MAXIMO, "la pila no pasa del tope");
        assert_eq!(
            app.deshacer.first().map(|c| c.padding),
            Some(8),
            "los pasos más viejos son los que se caen primero"
        );
    }
}
