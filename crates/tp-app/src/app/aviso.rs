//! Avisos in-situ (review UI/UX I4).
//!
//! Casi todo lo que la app confirmaba —guardar, copiar una ruta, añadir
//! sprites, borrar una clave— terminaba sólo en el log de abajo: para
//! saber si la pulsación salió bien había que mirar una pestaña que casi
//! nadie tiene desplegada. Aquí el mensaje se pinta además junto al gesto
//! que lo originó, y se va solo. El log sigue recibiéndolo, que es donde
//! queda el registro.

use super::{App, LogKind};
use eframe::egui;

/// Cuánto se ve un aviso: tiempo de sobra para leerlo y se aparta antes
/// de estorbar al gesto siguiente.
const DURACION: f64 = 2.5;

/// Tiempo del aviso que no acompaña a un gesto sino a un cambio que la
/// app hace sola: el dock de Ajustes plegado por ancho (4.2). Un segundo
/// más que el resto porque no basta con leerlo —hay que ir a por `F9` con
/// la ventana todavía estrecha—, y porque no hay control al lado donde
/// mirar para entender qué acaba de pasar.
pub(super) const DURACION_PLIEGUE: f64 = 3.0;

/// Hueco entre el control y el recuadro, en píxeles.
const HUECO: f32 = 6.0;

/// Ancho máximo del recuadro: más allá el texto se parte en varias
/// líneas en vez de estirarse hasta salirse de la ventana.
const ANCHO_MAX: f32 = 420.0;

/// Id del recuadro. Es también la clave con la que egui recuerda el
/// tamaño de la caja, así que tiene que ser el mismo entre avisos.
const ID: &str = "aviso_in_situ";

/// Un aviso en pantalla (o a punto de estarlo).
pub(super) struct Aviso {
    /// Mensaje ya en el idioma de la interfaz: exactamente el mismo que
    /// se apunta en el log.
    texto: String,
    color: egui::Color32,
    /// Punto de anclaje elegido en el primer frame. `None` = al pie de la
    /// ventana (gesto de teclado, sin control donde mirar).
    ancla: Option<egui::Pos2>,
    /// Reloj de egui del primer pintado. Se sella ahí porque es el único
    /// frame en que el reloj y el puntero cuentan para este gesto.
    empezado: Option<f64>,
    /// Cuánto dura este aviso en concreto ([`DURACION`] por regla general,
    /// [`DURACION_PLIEGUE`] si la app se pliega algo sin que nadie lo
    /// pida): el tiempo es del aviso, no del mecanismo.
    duracion: f64,
}

impl App {
    /// Feedback in-situ (I4): lo que antes sólo era una línea en el log
    /// se pinta además junto al gesto que lo originó.
    ///
    /// El recuadro sale donde está el puntero si la acción vino del ratón
    /// —el ratón está justo sobre el control pulsado— y al pie de la
    /// ventana si vino del teclado (Ctrl+S no tiene control donde mirar).
    /// Si en un mismo frame llegan dos avisos gana el último; al log
    /// llegan todos.
    pub(super) fn aviso(&mut self, kind: LogKind, texto: String) {
        self.aviso_durante(kind, texto, DURACION);
    }

    /// Como [`App::aviso`] con el tiempo a medida.
    ///
    /// Sólo lo necesita quien avisa de un cambio que la app hizo sola: el
    /// aviso de un gesto se lee mientras la mano sigue en el control, y
    /// el del pliegue del dock no tiene ese lujo (4.2).
    pub(super) fn aviso_durante(&mut self, kind: LogKind, texto: String, duracion: f64) {
        // El log traduce lo que le llega (los mensajes del motor vienen
        // en español): el aviso enseña exactamente lo que se apunta.
        let texto = crate::i18n::tr(&texto);
        let color = color_de(&self.egui_ctx, &kind);
        self.log(kind, texto.clone());
        self.aviso = Some(Aviso {
            texto,
            color,
            ancla: None,
            empezado: None,
            duracion,
        });
    }
}

/// El color con el que el panel de log pinta cada tipo de mensaje.
fn color_de(ctx: &egui::Context, kind: &LogKind) -> egui::Color32 {
    let style = ctx.style();
    let visuals = &style.visuals;
    match kind {
        LogKind::Info => visuals.text_color(),
        LogKind::Warning => visuals.warn_fg_color,
        LogKind::Error => visuals.error_fg_color,
    }
}

/// Dónde anclar el recuadro: el puntero si acaba de pulsar (el ratón está
/// sobre el control), y `None` si el gesto vino del teclado.
fn ancla_de(ctx: &egui::Context) -> Option<egui::Pos2> {
    let pulsado = ctx.input(|i| {
        i.pointer.button_pressed(egui::PointerButton::Primary)
            || i.pointer.button_pressed(egui::PointerButton::Secondary)
            || i.pointer.button_clicked(egui::PointerButton::Primary)
            || i.pointer.button_clicked(egui::PointerButton::Secondary)
    });
    if pulsado {
        ctx.input(|i| i.pointer.latest_pos())
    } else {
        None
    }
}

/// La esquina en la que cae el recuadro de un gesto de teclado.
fn pie(ctx: &egui::Context) -> egui::Pos2 {
    let rect = ctx.content_rect();
    egui::pos2(rect.right() - 16.0, rect.bottom() - 16.0)
}

/// Pinta el aviso vigente, si lo hay. Se llama al final del `update`,
/// para que el recuadro quede por encima de todo lo demás.
pub(super) fn pintar(ctx: &egui::Context, aviso: &mut Option<Aviso>) {
    let ahora = ctx.input(|i| i.time);
    if let Some(a) = aviso.as_mut() {
        if a.empezado.is_none() {
            a.empezado = Some(ahora);
            a.ancla = ancla_de(ctx);
        }
    }
    let Some(a) = aviso.as_ref() else {
        return;
    };
    let restante = a.duracion - (ahora - a.empezado.unwrap_or(ahora));
    if restante <= 0.0 {
        *aviso = None;
        return;
    }
    // Sin este repintado programado el recuadro se quedaría en pantalla
    // hasta el próximo movimiento del ratón: egui no vuelve a pintar por
    // su cuenta.
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(restante));

    let _pintado = egui::Tooltip::always_open(
        ctx.clone(),
        // La capa de todos los paneles. Al registrar el aviso ahí, egui
        // suprime la ayuda de los controles mientras dure: sin esto, a
        // los 0,5 s la ayuda del botón recién pulsado se pegaría encima
        // del recuadro.
        egui::LayerId::background(),
        egui::Id::new(ID),
        egui::PopupAnchor::Position(a.ancla.unwrap_or_else(|| pie(ctx))),
    )
    .gap(HUECO)
    .width(ANCHO_MAX)
    .show(|ui| {
        // La caja recuerda su tamaño de frame a frame: sin este techo, un
        // aviso corto dejaría al siguiente largo encajado en una columna
        // de dos caracteres. Fijar el máximo aquí hace crecer el espacio
        // disponible en el propio frame, y la caja —que abriga al
        // contenido— se adapta a lo que pese.
        ui.set_max_width(ANCHO_MAX);
        ui.colored_label(a.color, &a.texto);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{texto_pintado, textos_pintados};

    /// Texto de ayuda de las pruebas. Va en una constante y no a pelo en
    /// la llamada porque el escáner i18n (que la app pinta lo que no pasa
    /// por `t!`) mira los argumentos de `on_hover_text`.
    const AYUDA: &str = "ayuda del botón";

    /// Un frame con la ventana a 1360×860 y el reloj puesto a mano: el
    /// aviso vive de tiempo, así que los tests lo manejan.
    fn entrada(t: f64, eventos: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360.0, 860.0),
            )),
            time: Some(t),
            events: eventos,
            ..egui::RawInput::default()
        }
    }

    fn mover_a(pos: egui::Pos2) -> egui::Event {
        egui::Event::PointerMoved(pos)
    }

    fn boton(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// El rectángulo del texto `texto` pintado a menos de `margen` del
    /// punto. El log repite el mismo mensaje en el panel de abajo (a la
    /// izquierda), así que buscar por posición es lo que distingue el
    /// recuadro de la línea del registro.
    fn rect_junto_a(
        pintados: &[(String, egui::Rect, egui::Rect)],
        texto: &str,
        punto: egui::Pos2,
        margen: f32,
    ) -> Option<egui::Rect> {
        pintados
            .iter()
            .filter(|(t, _, _)| t.contains(texto))
            .map(|(_, rect, _)| *rect)
            .find(|rect| rect.min.distance(punto) <= margen)
    }

    /// Crea el aviso y corre los dos frames que hacen falta para verlo:
    /// en el primero egui sella el anclaje y mide la caja (ese frame
    /// todavía no pinta nada) y en el segundo ya sale el recuadro.
    fn ver_aviso(
        app: &mut App,
        ctx: &egui::Context,
        donde: egui::Pos2,
        mensaje: &str,
        raton: bool,
    ) -> egui::FullOutput {
        if raton {
            // La bajada queda registrada en un frame y la subida en el
            // siguiente: egui sólo suelta el «click» al soltar.
            app.run_frame(ctx, entrada(0.0, vec![mover_a(donde), boton(donde, true)]));
            app.aviso(LogKind::Info, mensaje.to_string());
            app.run_frame(ctx, entrada(0.016, vec![boton(donde, false)]));
        } else {
            app.run_frame(ctx, entrada(0.0, vec![mover_a(donde)]));
            app.aviso(LogKind::Info, mensaje.to_string());
            app.run_frame(ctx, entrada(0.016, vec![]));
        }
        app.run_frame(ctx, entrada(0.032, vec![]))
    }

    /// Un aviso pintado en una capa a solas, para las pruebas que sólo
    /// miran el recuadro y no quieren el ruido del resto de la app.
    fn frame_solo_aviso(
        ctx: &egui::Context,
        t: f64,
        aviso: &mut Option<Aviso>,
    ) -> egui::FullOutput {
        ctx.run(entrada(t, vec![]), |ctx| pintar(ctx, aviso))
    }

    /// Un frame limpio, sin ayuda de ningún tipo.
    fn frame_limpio(ctx: &egui::Context, t: f64, eventos: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run(entrada(t, eventos), |ctx| {
            egui::CentralPanel::default().show(ctx, |_ui| {});
        })
    }

    /// Un frame con un botón con ayuda en la capa de los paneles, que es
    /// donde vive la ayuda real de la app; pinta el aviso si lo hay, como
    /// hace el `update` de verdad. El hover se calcula con un frame de
    /// retraso, así que hace falta repetirlo.
    fn frame_con_ayuda(ctx: &egui::Context, t: f64, aviso: &mut Option<Aviso>) -> egui::FullOutput {
        ctx.run(entrada(t, vec![]), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let respuesta = ui.put(
                    egui::Rect::from_min_size(egui::pos2(600.0, 300.0), egui::vec2(140.0, 30.0)),
                    egui::Button::new("Botón con ayuda"),
                );
                let _ = respuesta.on_hover_text(AYUDA);
            });
            if aviso.is_some() {
                pintar(ctx, aviso);
            }
        })
    }

    /// El aviso del ratón se pinta junto al control (el ratón está justo
    /// ahí) y sigue llegando al log: era el doble propósito del I4.
    #[test]
    fn el_aviso_del_raton_se_pinta_junto_al_control_y_al_log() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let donde = egui::pos2(600.0, 300.0);
        const MENSAJE: &str = "Ruta copiada: /tmp/x/hero.png";

        let salida = ver_aviso(&mut app, &ctx, donde, MENSAJE, true);

        let rect = rect_junto_a(&textos_pintados(&salida), MENSAJE, donde, 80.0)
            .unwrap_or_else(|| panic!("el aviso no sale junto al puntero {donde:?}"));
        assert!(
            rect.min.y >= donde.y && rect.min.y <= donde.y + 150.0,
            "debería salir por debajo del control: {rect:?}"
        );
        assert!(
            app.logs.iter().any(|l| l.text.contains(MENSAJE)),
            "el aviso tiene que seguir llegando al log"
        );
    }

    /// Se apaga solo: a los tres segundos no queda nada en el sitio donde
    /// estaba (el log, que es el registro, sigue con su línea).
    #[test]
    fn el_aviso_se_va_solo() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let donde = egui::pos2(600.0, 300.0);
        const MENSAJE: &str = "Ruta copiada: /tmp/x/hero.png";

        let salida = ver_aviso(&mut app, &ctx, donde, MENSAJE, true);
        assert!(
            rect_junto_a(&textos_pintados(&salida), MENSAJE, donde, 80.0).is_some(),
            "arranca visible"
        );

        let tarde = app.run_frame(&ctx, entrada(3.0, vec![]));
        assert!(
            rect_junto_a(&textos_pintados(&tarde), MENSAJE, donde, 80.0).is_none(),
            "a los tres segundos ya no debe estar en el sitio del gesto"
        );
        assert!(app.aviso.is_none(), "y el aviso se apaga de verdad");
    }

    /// Ctrl+S no tiene control donde mirar, así que el recuadro cae al
    /// pie de la ventana aunque el puntero esté en mitad del lienzo.
    #[test]
    fn el_aviso_del_teclado_cae_al_pie_de_la_ventana() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let donde = egui::pos2(600.0, 300.0);
        const MENSAJE: &str = "Proyecto guardado en /tmp/x.tpproj";

        let salida = ver_aviso(&mut app, &ctx, donde, MENSAJE, false);

        let pintados = textos_pintados(&salida);
        assert!(
            pintados
                .iter()
                .any(|(t, r, _)| t.contains(MENSAJE) && r.min.x > 800.0),
            "el recuadro debe caer al pie de la ventana (a la derecha)"
        );
        assert!(
            rect_junto_a(&pintados, MENSAJE, donde, 80.0).is_none(),
            "…y no junto al puntero, que no ha pulsado nada"
        );
    }

    /// Mientras se ve un aviso, egui no enseña la ayuda del control sobre
    /// el que se acaba de pulsar. Sin esta cortesía, a los 0,5 s la ayuda
    /// («Guardar proyecto — Ctrl+S») se pegaría encima del recuadro.
    ///
    /// El hover se calcula con un frame de retraso y la ayuda mide su
    /// caja en otro, así que hacen falta tres frames para que la ayuda
    /// llegue a pintarse.
    #[test]
    fn el_aviso_corta_la_ayuda_de_los_controles() {
        let donde = egui::pos2(610.0, 315.0);

        // Con aviso: la ayuda no debe salir.
        let ctx = egui::Context::default();
        let mut aviso = Some(Aviso {
            texto: "aviso de prueba".into(),
            color: egui::Color32::WHITE,
            ancla: None,
            empezado: None,
            duracion: DURACION,
        });
        frame_limpio(&ctx, 0.5, vec![mover_a(donde)]);
        frame_solo_aviso(&ctx, 1.0, &mut aviso);
        frame_con_ayuda(&ctx, 1.1, &mut aviso);
        frame_con_ayuda(&ctx, 1.2, &mut aviso);
        let con = frame_con_ayuda(&ctx, 1.3, &mut aviso);
        assert!(
            !texto_pintado(&con).contains(AYUDA),
            "con el aviso en pantalla la ayuda no debe aparecer"
        );

        // Sin aviso: si aquí tampoco sale, el test no prueba nada.
        let ctx = egui::Context::default();
        let mut sin_aviso = None;
        frame_limpio(&ctx, 0.5, vec![mover_a(donde)]);
        frame_limpio(&ctx, 1.0, vec![]);
        frame_con_ayuda(&ctx, 1.1, &mut sin_aviso);
        frame_con_ayuda(&ctx, 1.2, &mut sin_aviso);
        let sin = frame_con_ayuda(&ctx, 1.3, &mut sin_aviso);
        assert!(
            texto_pintado(&sin).contains(AYUDA),
            "control: sin aviso la ayuda sí debe salir"
        );
    }
}
