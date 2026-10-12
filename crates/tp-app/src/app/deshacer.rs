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

    /// Quita el paso recién anotado, para cuando el gesto acaba **sin tocar
    /// nada** (una carpeta vacía, un nombre ya excluido, un grupo que no
    /// existe). Sin esto, el paso en blanco se consumiría con el primer
    /// `Ctrl+Z` y parecería que el deshacer se saltaba un paso real
    /// (re-review B4).
    pub(super) fn desanotar_deshacer(&mut self) {
        self.deshacer.pop();
    }

    /// Deshace el último paso (`Ctrl+Z`). Sin pasos no hay nada que
    /// deshacer —no es un error—, pero eso se dice: el atajo seguía vivo
    /// con la pila vacía y se quedaba en un no-op silencioso, sin que el
    /// usuario llegara a saber si la tecla le había hecho caso (re-review
    /// RB3).
    pub(super) fn deshacer(&mut self) {
        let Some(config) = self.deshacer.pop() else {
            // `Ctrl+Z` mantenido repite el evento: mientras el recuadro del
            // aviso siga en pantalla no hace falta anotar la línea otra vez.
            let texto = t!("No hay nada que deshacer.").to_string();
            if self.aviso_en_pantalla() != Some(texto.as_str()) {
                self.aviso(LogKind::Info, texto);
            }
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

    /// RB3: el no-op de arriba era además silencioso. El menú apaga su
    /// entrada con la pila vacía, pero `Ctrl+Z` seguía vivo y no decía
    /// nada: el usuario no llegaba a saber si la tecla le había hecho
    /// caso. Y con la tecla mantenida —que repite el evento— el mismo
    /// aviso no debe repetirse en el registro mientras está en pantalla.
    #[test]
    fn deshacer_sin_nada_que_hacer_se_avisa_sin_repetirse() {
        let (mut app, _ctx) = app();
        assert!(
            app.deshacer.is_empty(),
            "la prueba empieza sin nada que deshacer"
        );

        app.deshacer();
        assert_eq!(app.deshacer.len(), 0, "sin pila no hay nada que deshacer");
        assert!(
            app.aviso.is_some(),
            "el atajo con la pila vacía debe responder en pantalla"
        );
        assert!(
            app.logs
                .last()
                .is_some_and(|l| l.text.contains("No hay nada que deshacer")),
            "y debe quedarse anotado en el registro"
        );
        let anotadas = app.logs.len();

        app.deshacer();
        assert_eq!(
            app.logs.len(),
            anotadas,
            "mientras el aviso está en pantalla no debe repetirse en el registro"
        );

        // Expirado el recuadro, volver a pulsar vuelve a avisar: la
        // deduplicación no puede devolver el silencio.
        app.aviso = None;
        app.deshacer();
        assert_eq!(
            app.logs.len(),
            anotadas + 1,
            "sin aviso en pantalla se vuelve a avisar"
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

    // ------------------------------------------------------------------
    // Re-review B4: cinco gestos tocaban la configuración sin anotar
    // instantánea. Como `deshacer()` hace un `pop()` ciego, `Ctrl+Z`
    // aplicaba entonces el paso **anterior** —el del gesto de antes—: un
    // deshacer imprevisible, peor que no tener.
    // ------------------------------------------------------------------

    /// Quitar un sprite desde el árbol anota su paso y lo recupera.
    #[test]
    fn el_arbol_quita_sprites_con_su_paso_de_deshacer() {
        let (mut app, _ctx) = app();
        let fantasma = std::path::Path::new("/tmp/tp_b4_fantasma.png");

        app.remove_from_tree(fantasma);

        assert_eq!(
            app.config.excluded_inputs.len(),
            1,
            "el sprite queda excluido"
        );
        app.deshacer();
        assert!(
            app.config.excluded_inputs.is_empty(),
            "Ctrl+Z recupera el sprite que quitó el árbol"
        );
    }

    /// Una carpeta sin imágenes no quita nada, y su paso en blanco no debe
    /// consumir el primer `Ctrl+Z`: parecería que el deshacer se saltaba
    /// un paso real.
    #[test]
    fn una_carpeta_vacia_no_deja_un_paso_en_blanco() {
        let (mut app, _ctx) = app();
        // Sin anotar nada antes a propósito: si el gesto dejara un paso en
        // blanco, `Ctrl+Z` lo aplicaría y tocaría esta misma exclusión.
        app.config
            .excluded_inputs
            .push("/tmp/tp_b4_base.png".into());
        let vacia = std::env::temp_dir().join(format!("tp_b4_vacia_{}", std::process::id()));
        std::fs::create_dir_all(&vacia).expect("tmp");

        app.remove_from_tree(&vacia);

        assert_eq!(
            app.deshacer.len(),
            0,
            "sin nada que quitar no debe quedar un paso en blanco"
        );
        app.deshacer();
        assert_eq!(
            app.config.excluded_inputs.len(),
            1,
            "Ctrl+Z no debe tocar nada: el gesto no quitó nada"
        );
        std::fs::remove_dir_all(&vacia).ok();
    }

    /// «Vaciar» un grupo avisaba de cero: era el único gesto destructivo
    /// de la app sin rastro (re-review B3). Ahora avisa y se deshace.
    #[test]
    fn vaciar_un_grupo_avisa_y_es_deshacible() {
        let (mut app, _ctx) = app();
        app.config.folder_groups = vec![tp_core::config::FolderGroup {
            name: "ui".into(),
            sprites: vec!["a".into(), "b".into()],
        }];

        app.empty_group(0);

        assert!(
            app.config.folder_groups[0].sprites.is_empty(),
            "el grupo queda vacío"
        );
        assert!(
            app.logs
                .last()
                .is_some_and(|l| l.text.contains("vaciado") && l.text.contains("2 sprite")),
            "el aviso debe decir qué grupo y cuántos sprites devolvía: {:?}",
            app.logs.last().map(|l| l.text.clone())
        );
        app.deshacer();
        assert_eq!(
            app.config.folder_groups[0].sprites.len(),
            2,
            "Ctrl+Z recupera la asignación que vació el grupo"
        );
    }

    /// Control del anterior: un grupo ya vacío no se toca, así que no debe
    /// anotar paso. Sin esto, un `anotar_deshacer()` incondicional pasaría
    /// la otra prueba dejando pasos que no cambian nada.
    #[test]
    fn vaciar_un_grupo_imposible_no_anota_paso() {
        let (mut app, _ctx) = app();
        app.config.folder_groups = vec![tp_core::config::FolderGroup {
            name: "ui".into(),
            sprites: vec![],
        }];

        app.empty_group(0);

        assert_eq!(
            app.deshacer.len(),
            0,
            "sin nada que vaciar no debe anotar un paso"
        );
    }

    /// Borrar un grupo también deja su paso.
    #[test]
    fn borrar_un_grupo_es_deshacible() {
        let (mut app, _ctx) = app();
        app.config.folder_groups = vec![
            tp_core::config::FolderGroup {
                name: "ui".into(),
                sprites: vec!["a".into()],
            },
            tp_core::config::FolderGroup {
                name: "fx".into(),
                sprites: vec!["b".into()],
            },
        ];

        app.delete_group(0);
        assert_eq!(app.config.folder_groups.len(), 1, "el grupo desaparece");

        app.deshacer();
        assert_eq!(
            app.config.folder_groups.len(),
            2,
            "Ctrl+Z recupera el grupo"
        );
        assert_eq!(app.config.folder_groups[0].name, "ui", "y en su sitio");
    }

    /// «↺ Restaurar» borraba todas las exclusiones sin anotar nada.
    #[test]
    fn restaurar_exclusiones_es_deshacible() {
        let (mut app, _ctx) = app();
        app.config
            .excluded_inputs
            .push("/tmp/tp_b4_excluido.png".into());

        app.restore_excluded();
        assert!(app.config.excluded_inputs.is_empty(), "quedan a la vista");

        app.deshacer();
        assert_eq!(
            app.config.excluded_inputs.len(),
            1,
            "Ctrl+Z recupera las exclusiones"
        );
    }

    /// Quitar una carpeta inteligente también anota su paso.
    #[test]
    fn quitar_carpeta_inteligente_es_deshacible() {
        let (mut app, _ctx) = app();
        let dir = std::env::temp_dir().join(format!("tp_b4_smart_{}", std::process::id()));
        app.config.extra_inputs.push(dir.clone());

        app.remove_smart_folder(&dir);
        assert!(app.config.extra_inputs.is_empty(), "la carpeta desaparece");

        app.deshacer();
        assert_eq!(
            app.config.extra_inputs.len(),
            1,
            "Ctrl+Z recupera la carpeta inteligente"
        );
    }
}
