//! Estilo visual global: arranque del tema, acentos compartidos por todos
//! los paneles de la ventana y los **tokens de superficie** con los que
//! cada zona (barras, docks, lienzo) se pinta con su propio rol.

use eframe::egui;

/// Verde del resaltado «recién añadido», legible sobre los dos temas.
///
/// El claro era `(0, 132, 74)`: 4,08:1 sobre la barra, por debajo de AA
/// (re-review V1).
pub(crate) fn just_added_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(130, 220, 160)
    } else {
        egui::Color32::from_rgb(0, 110, 60)
    }
}

/// Ámbar de aviso (carpetas inteligentes, «empaqueta sola», avisos).
///
/// El claro era `(150, 105, 0)`: 4,15:1 sobre la barra (re-review V1).
pub(crate) fn amber_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(230, 190, 60)
    } else {
        egui::Color32::from_rgb(135, 85, 0)
    }
}

/// Azul informativo (carpetas anidadas, «publicando…», «actualizando…»).
///
/// El claro era `(20, 100, 205)`: 4,34:1 sobre el panel (re-review V1).
pub(crate) fn info_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(120, 170, 255)
    } else {
        egui::Color32::from_rgb(16, 88, 180)
    }
}

/// Gris tenue de los textos de las zonas de arrastre (fondo de panel).
pub(crate) fn muted_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_gray(150)
    } else {
        egui::Color32::from_gray(100)
    }
}

/// El mismo gris, pero en su variante de «pasado por encima».
///
/// En claro comparte valor con [`info_color`], que era su color: 4,34:1
/// sobre el panel (re-review V1).
pub(crate) fn muted_hover_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(180, 225, 255)
    } else {
        egui::Color32::from_rgb(16, 88, 180)
    }
}

/// Tinte de fondo de las zonas de arrastre (blanco en oscuro, negro en claro).
pub(crate) fn drop_tint(v: &egui::Visuals, hover: bool) -> egui::Color32 {
    if hover {
        egui::Color32::from_rgba_unmultiplied(120, 200, 255, 60)
    } else if v.dark_mode {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
    } else {
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 14)
    }
}

/// Un color por rol de superficie (fase 2 del rediseño).
///
/// El lienzo es la zona más extrema de cada tema —la más oscura en
/// oscuro, la más clara en claro—, las barras van por medio y los docks
/// y los diálogos comparten la superficie «de contenido». Así las tres
/// zonas de la ventana se distinguen entre sí sin depender del texto.
pub(crate) struct Superficies {
    /// El área de trabajo: el atlas y su sombra.
    pub(crate) lienzo: egui::Color32,
    /// Docks laterales, diálogos, menús y desplegables.
    pub(crate) panel: egui::Color32,
    /// Barra de menús, de herramientas, de estado y de pestañas.
    pub(crate) chrome: egui::Color32,
    /// 1 px que separa zonas: ≥3:1 contra las tres anteriores.
    pub(crate) borde: egui::Color32,
    /// Texto normal: ≥7:1 sobre cualquier superficie; el tenue (60 % de
    /// opacidad, que es como egui lo pinta) mantiene ≥4,5:1.
    pub(crate) texto: egui::Color32,
}

/// Los tokens de [`Superficies`] para el tema dado.
pub(crate) fn superficies(dark_mode: bool) -> Superficies {
    if dark_mode {
        Superficies {
            lienzo: egui::Color32::from_gray(12),
            chrome: egui::Color32::from_gray(24),
            panel: egui::Color32::from_gray(36),
            borde: egui::Color32::from_gray(115),
            texto: egui::Color32::from_gray(232),
        }
    } else {
        Superficies {
            lienzo: egui::Color32::from_gray(252),
            chrome: egui::Color32::from_gray(237),
            panel: egui::Color32::from_gray(226),
            borde: egui::Color32::from_gray(125),
            texto: egui::Color32::from_gray(0),
        }
    }
}

/// Los tokens de las zonas que enseñan una imagen o una malla: el fondo
/// de la hoja (dónde se pinta la textura y se transparentan sus huecos),
/// el suelo del visor que la rodea, el marco de 1 px que la delimita y el
/// texto de los estados vacíos.
///
/// Hasta la re-review D3/D4 estos grises venían fijos y asumían fondo
/// oscuro: en tema claro el lienzo (252) llevaba encima una losa de gris
/// 30 por cada píxel transparente del atlas, el visor del pivot era otra
/// losa de 45 y el estado vacío de «Dividir hoja» quedaba con el texto a
/// 4,10:1. Cada rol coge ahora su valor del tema, y la prueba
/// `los_visores_cumplen_wcag_y_se_ajustan_al_tema` mide los cuatro.
pub(crate) struct Visores {
    /// Detrás de la textura: un paso hacia el gris medio desde el suelo,
    /// para que la hoja se lea como una pieza apoyada y no como un
    /// refresco del lienzo.
    pub(crate) hoja: egui::Color32,
    /// El fondo del visor: dónde descansa la hoja o se enseña la malla.
    pub(crate) suelo: egui::Color32,
    /// Marco de la imagen original: ≥3:1 contra el suelo (WCAG 1.4.11).
    pub(crate) borde: egui::Color32,
    /// Texto de los estados vacíos del visor: ≥4,5:1 sobre el suelo.
    pub(crate) texto: egui::Color32,
}

/// Los tokens de [`Visores`] para el tema dado.
pub(crate) fn visores(dark_mode: bool) -> Visores {
    if dark_mode {
        Visores {
            hoja: egui::Color32::from_gray(30),
            suelo: egui::Color32::from_gray(20),
            borde: egui::Color32::from_gray(120),
            texto: egui::Color32::from_gray(150),
        }
    } else {
        Visores {
            hoja: egui::Color32::from_gray(210),
            suelo: egui::Color32::from_gray(237),
            borde: egui::Color32::from_gray(125),
            texto: egui::Color32::from_gray(100),
        }
    }
}

/// Pinta un estilo con los tokens de [`Superficies`].
///
/// `panel_fill` es el marco por defecto de las barras (`TopBottomPanel`),
/// así que ahí vive el color *chrome*; el lienzo y los docks llevan su
/// propio relleno en `mod.rs`, donde se construyen. `window_fill` cubre
/// menús y diálogos: flotan con la misma superficie que los docks.
fn aplicar_superficies(v: &mut egui::Visuals) {
    let s = superficies(v.dark_mode);
    v.panel_fill = s.chrome;
    v.window_fill = s.panel;
    v.window_stroke = egui::Stroke::new(1.0_f32, s.borde);
    // Separadores, líneas de borde de panel y tiradores de redimensión.
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, s.borde);
    // Texto normal de `ui.label` y base del texto tenue (que egui pinta
    // con el 60 % de la opacidad encima de la superficie).
    v.widgets.noninteractive.fg_stroke.color = s.texto;
}

/// Estilo visual global: esquinas suaves, selección y enlaces teñidos con el
/// acento, sliders rellenos, superficies con rol. Se aplica a los estilos
/// oscuro y claro a la vez y sólo la primera vez (cada token se calcula con
/// el `dark_mode` de su propio estilo).
pub(crate) fn apply_theme(ctx: &egui::Context, theme: crate::ui_prefs::Theme) {
    if ctx.memory(|m| m.data.get_temp::<bool>(egui::Id::new("tp_theme"))) == Some(true) {
        return;
    }
    theme.apply(ctx);
    // Se tocan los dos estilos (oscuro y claro) a la vez: egui elige uno de
    // ellos según la preferencia, así que un cambio de tema en caliente no
    // pierde estos ajustes.
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        aplicar_superficies(v);
        v.window_corner_radius = 8.into();
        v.menu_corner_radius = 6.into();
        v.widgets.noninteractive.corner_radius = 4.into();
        v.widgets.inactive.corner_radius = 5.into();
        v.widgets.hovered.corner_radius = 5.into();
        v.widgets.active.corner_radius = 5.into();
        v.widgets.open.corner_radius = 5.into();
        v.selection.bg_fill = if v.dark_mode {
            egui::Color32::from_rgb(38, 98, 115)
        } else {
            egui::Color32::from_rgb(166, 206, 227)
        };
        v.selection.stroke.width = 1.0;
        v.hyperlink_color = if v.dark_mode {
            egui::Color32::from_rgb(97, 175, 239)
        } else {
            egui::Color32::from_rgb(0, 94, 190)
        };
        // El ámbar y el rojo de serie de egui están elegidos para su
        // fondo, no para el nuestro: en claro el aviso (255, 100, 0) se
        // queda en 2,29:1 sobre el panel y el error (255, 0, 0) en
        // 3,09:1; en oscuro el error se queda en 3,88:1 sobre el panel.
        // WCAG AA pide 4,5:1 para texto (re-review V1).
        if v.dark_mode {
            v.error_fg_color = egui::Color32::from_rgb(255, 90, 90);
        } else {
            v.warn_fg_color = egui::Color32::from_rgb(135, 85, 0);
            v.error_fg_color = egui::Color32::from_rgb(170, 0, 0);
        }
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.4 };
        v.collapsing_header_frame = true;
    });
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new("tp_theme"), true));
}

/// Luminosidad relativa (WCAG 2.1): 0 = negro, 1 = blanco.
///
/// Vive fuera de los tests porque también la usa [`texto_sobre`] para
/// decidir el color de texto de un botón relleno (re-review D6).
pub(crate) fn luminancia(c: egui::Color32) -> f32 {
    fn canal(v: u8) -> f32 {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * canal(c.r()) + 0.7152 * canal(c.g()) + 0.0722 * canal(c.b())
}

/// Contraste WCAG entre dos colores: 1 = iguales, 21 = blanco sobre negro.
pub(crate) fn razon(a: egui::Color32, b: egui::Color32) -> f32 {
    let (la, lb) = (luminancia(a), luminancia(b));
    let (alto, bajo) = if la > lb { (la, lb) } else { (lb, la) };
    (alto + 0.05) / (bajo + 0.05)
}

/// El texto —blanco o negro— que más se lee sobre `fondo` (WCAG 1.4.1).
///
/// Un botón destructivo se pinta con el rojo del tema, y ese rojo no es
/// el mismo en los dos temas: en claro es (170, 0, 0), oscuro, y el
/// texto claro encima da 7,8:1; en oscuro es (255, 90, 90), claro, y
/// ahí el texto claro se queda en 2,6:1 y el que sirve es el oscuro.
/// Fijar «blanco sobre rojo» a medias rompería el tema que no tocó.
pub(crate) fn texto_sobre(fondo: egui::Color32) -> egui::Color32 {
    if razon(egui::Color32::WHITE, fondo) >= razon(egui::Color32::BLACK, fondo) {
        egui::Color32::WHITE
    } else {
        egui::Color32::BLACK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    /// L* (CIE): luz **percibida**, lineal — 0 es negro y 100 blanco. Es la
    /// escala en la que una diferencia de 2.3 ya se ve, que es lo que WCAG
    /// no mide: dos grises muy oscuros pueden ser «3:1» sin verse nunca.
    fn luz_percibida(c: egui::Color32) -> f32 {
        let y = luminancia(c);
        if y > 0.008856 {
            116.0 * y.cbrt() - 16.0
        } else {
            903.3 * y
        }
    }

    /// Separación entre dos colores en luz percibida (L*).
    fn separacion(a: egui::Color32, b: egui::Color32) -> f32 {
        (luz_percibida(a) - luz_percibida(b)).abs()
    }

    /// Cómo pinta egui un color con opacidad sobre un fondo opaco: el
    /// `Color32` trae el alfa ya multiplicado, así que «src + fondo·(1−α)».
    fn sobre(fg: egui::Color32, bg: egui::Color32) -> egui::Color32 {
        let alfa = f32::from(fg.a()) / 255.0;
        let mezcla = |f: u8, b: u8| {
            (f32::from(f) + f32::from(b) * (1.0 - alfa))
                .clamp(0.0, 255.0)
                .round() as u8
        };
        egui::Color32::from_rgb(
            mezcla(fg.r(), bg.r()),
            mezcla(fg.g(), bg.g()),
            mezcla(fg.b(), bg.b()),
        )
    }

    /// El estilo de un tema ya pintado con los tokens, como lo deja
    /// [`apply_theme`].
    fn estilo(dark_mode: bool) -> egui::Visuals {
        let mut v = if dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        aplicar_superficies(&mut v);
        v
    }

    fn temas() -> [bool; 2] {
        [true, false]
    }

    /// Fase 2, contraste del texto: normal ≥7:1 (AAA) y tenue —que egui
    /// pinta al 60 % de opacidad encima de la superficie— ≥4,5:1 (AA),
    /// contra las tres superficies y en los dos temas.
    #[test]
    fn el_texto_cumple_wcag_aa_en_los_dos_temas() {
        for dark_mode in temas() {
            let tema = if dark_mode { "oscuro" } else { "claro" };
            let v = estilo(dark_mode);
            let s = superficies(dark_mode);
            assert_eq!(v.text_color(), s.texto, "{tema}: el texto no usa su token");
            for (nombre, superficie) in [
                ("lienzo", s.lienzo),
                ("chrome", s.chrome),
                ("panel", s.panel),
            ] {
                let normal = razon(v.text_color(), superficie);
                assert!(
                    normal >= 7.0,
                    "{tema}: texto normal sobre {nombre} son {normal:.2}:1, hace falta 7:1"
                );
                let tenue = razon(sobre(v.weak_text_color(), superficie), superficie);
                assert!(
                    tenue >= 4.5,
                    "{tema}: texto tenue sobre {nombre} son {tenue:.2}:1, hace falta 4.5:1"
                );
            }
        }
    }

    /// Fase 2, jerarquía de superficies: el lienzo, las barras y los docks
    /// se separan perceptiblemente entre sí (≥3 L*, la JND ronda 2.3) y el
    /// borde de 1 px que los delimita se ve sobre cada uno de ellos (WCAG
    /// 1.4.11: ≥3:1 para identificar el límite de una zona).
    #[test]
    fn las_superficies_se_distinguen_y_el_borde_se_ve() {
        for dark_mode in temas() {
            let tema = if dark_mode { "oscuro" } else { "claro" };
            let s = superficies(dark_mode);
            for (a, b, que) in [
                (s.lienzo, s.chrome, "lienzo-chrome"),
                (s.chrome, s.panel, "chrome-panel"),
            ] {
                let d = separacion(a, b);
                assert!(
                    d >= 3.0,
                    "{tema}: {que} sólo se separan {d:.2} L*, hacen falta 3.0"
                );
            }
            for (nombre, superficie) in [
                ("lienzo", s.lienzo),
                ("chrome", s.chrome),
                ("panel", s.panel),
            ] {
                let r = razon(s.borde, superficie);
                assert!(
                    r >= 3.0,
                    "{tema}: el borde sobre {nombre} son {r:.2}:1, hace falta 3:1"
                );
            }
        }
    }

    /// Los tokens llegan al estilo que egui usa para pintar: sin esta
    /// asignación las barras se quedarían con el gris por defecto.
    #[test]
    fn los_tokens_llegan_al_estilo_de_la_app() {
        let ctx = egui::Context::default();
        apply_theme(&ctx, crate::ui_prefs::Theme::System);
        let v = &ctx.style().visuals;
        let s = superficies(v.dark_mode);
        assert_eq!(v.panel_fill, s.chrome, "las barras no usan su token");
        assert_eq!(v.window_fill, s.panel, "los diálogos no usan su token");
        assert_eq!(v.window_stroke.color, s.borde, "el borde de ventana");
        assert_eq!(
            v.widgets.noninteractive.bg_stroke.color, s.borde,
            "los separadores no usan su token"
        );
        assert_eq!(v.text_color(), s.texto, "el texto no usa su token");
    }

    /// Cada zona de la ventana se pinta de verdad con su relleno: el lienzo
    /// manda (≥25 % de la pantalla), los docks son verticales (≥40 % de su
    /// altura) y una barra cruza el ancho entero (≥90 %).
    #[test]
    fn cada_zona_de_la_ventana_se_pinta_con_su_relleno() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let entrada = crate::testing::idle_input();
        let pantalla = entrada.screen_rect.expect("la prueba trae pantalla");
        let salida = app.run_frame(&ctx, entrada);
        let rellenos = crate::testing::rellenos_pintados(&salida);
        let s = superficies(ctx.style().visuals.dark_mode);

        let mayor_area = |color: egui::Color32| {
            rellenos
                .iter()
                .filter(|(relleno, _)| *relleno == color)
                .map(|(_, r)| r.width() * r.height())
                .fold(0.0, f32::max)
        };
        let max_ancho = |color: egui::Color32| {
            rellenos
                .iter()
                .filter(|(relleno, _)| *relleno == color)
                .map(|(_, r)| r.width())
                .fold(0.0, f32::max)
        };
        let max_alto = |color: egui::Color32| {
            rellenos
                .iter()
                .filter(|(relleno, _)| *relleno == color)
                .map(|(_, r)| r.height())
                .fold(0.0, f32::max)
        };

        let lienzo = mayor_area(s.lienzo);
        assert!(
            lienzo >= pantalla.area() * 0.25,
            "el lienzo ocupa {lienzo:.0} px² y debe ser ≥25 % de la ventana"
        );
        let chrome = max_ancho(s.chrome);
        assert!(
            chrome >= pantalla.width() * 0.9,
            "la barra más ancha mide {chrome:.0} px y debe cruzar ≥90 % de la ventana"
        );
        let panel = max_alto(s.panel);
        assert!(
            panel >= pantalla.height() * 0.4,
            "el dock más alto mide {panel:.0} px y debe llegar ≥40 % de la ventana"
        );
    }

    /// Los visuals completos como los deja [`apply_theme`] sobre un
    /// contexto de verdad: incluye los ajustes que `estilo()` no toca
    /// (`warn_fg_color`, `error_fg_color`, la selección…).
    fn aplicado(tema: crate::ui_prefs::Theme) -> egui::Visuals {
        let ctx = egui::Context::default();
        apply_theme(&ctx, tema);
        ctx.style().visuals.clone()
    }

    /// V1 (validación de la re-review): con el tema claro —el que
    /// recibe un escritorio Linux en modo claro, porque el
    /// predeterminado es `system`—, varios colores que `visuals()` y los
    /// tokens de la app dan al texto se caían por debajo de AA: el ámbar
    /// de aviso (2,29:1), el rojo de error (3,09:1), el ámbar de tokens
    /// (4,15:1), el azul informativo (4,34:1) y el verde «recién
    /// añadido» (4,08:1); en oscuro, el rojo de error tampoco llegaba
    /// (3,88:1 sobre el panel). Aquí queda la tabla entera de la review
    /// vigilando los dos temas contra las tres superficies.
    ///
    /// Las filas que la review daba por rotas midiendo el píxel
    /// —cabecera, log, texto débil, placeholder, pestaña, deshabilitado—
    /// pasan por color especificado (negro o gris en su sitio): lo que
    /// medía eran los bordes anti-alias de los glifos de 13 px, y WCAG
    /// juzga el color especificado. El deshabilitado, además, está
    /// exento (1.4.3) y aquí se le pide el umbral gráfico de 3:1.
    #[test]
    fn los_avisos_y_los_tokens_cumplen_wcag_aa_en_los_dos_temas() {
        for dark_mode in temas() {
            let tema = if dark_mode { "oscuro" } else { "claro" };
            let v = aplicado(if dark_mode {
                crate::ui_prefs::Theme::Dark
            } else {
                crate::ui_prefs::Theme::Light
            });
            let s = superficies(dark_mode);
            let filas = [
                ("aviso de «Avisos» (ámbar)", v.warn_fg_color, 4.5),
                ("cabecera de sección / log (info)", v.text_color(), 4.5),
                ("texto del Log (error)", v.error_fg_color, 4.5),
                ("texto débil / placeholder", v.weak_text_color(), 4.5),
                ("hipervínculo", v.hyperlink_color, 4.5),
                (
                    "botón deshabilitado (fuente)",
                    v.widgets.inactive.fg_stroke.color,
                    3.0,
                ),
                ("ámbar de tokens", amber_color(&v), 4.5),
                ("azul informativo", info_color(&v), 4.5),
                ("gris tenue (arrastre)", muted_color(&v), 4.5),
                ("gris tenue al pasar", muted_hover_color(&v), 4.5),
                ("recién añadido", just_added_color(&v), 4.5),
            ];
            for (nombre, color, umbral) in filas {
                for (sup_nom, sup) in [
                    ("lienzo", s.lienzo),
                    ("chrome", s.chrome),
                    ("panel", s.panel),
                ] {
                    let r = razon(sobre(color, sup), sup);
                    assert!(
                        r >= umbral,
                        "{tema}: «{nombre}» sobre {sup_nom} son {r:.2}:1,                          hace falta {umbral}:1"
                    );
                }
            }
            // La pestaña activa: su texto contra el relleno de selección.
            let activa = razon(
                sobre(v.text_color(), v.selection.bg_fill),
                v.selection.bg_fill,
            );
            assert!(
                activa >= 4.5,
                "{tema}: el texto de la pestaña activa son {activa:.2}:1,                  hace falta 4.5:1"
            );
        }
    }

    /// Re-review D3/D4: los rellenos de los visores venían fijos en grises
    /// que asumían fondo oscuro —gris 30 detrás de la textura sobre un
    /// lienzo claro de 252, losa de 45 en el pivot y gris 20 en la
    /// malla— y el estado vacío de «Dividir hoja» pintaba su texto a
    /// 4,10:1. Aquí quedan medidos los cuatro roles del token.
    #[test]
    fn los_visores_cumplen_wcag_y_se_ajustan_al_tema() {
        for dark_mode in temas() {
            let tema = if dark_mode { "oscuro" } else { "claro" };
            let s = superficies(dark_mode);
            let v = visores(dark_mode);

            let texto = razon(v.texto, v.suelo);
            assert!(
                texto >= 4.5,
                "{tema}: el texto del estado vacío sobre el suelo son {texto:.2}:1,                  hace falta 4.5:1"
            );
            let borde = razon(v.borde, v.suelo);
            assert!(
                borde >= 3.0,
                "{tema}: el marco del visor sobre el suelo son {borde:.2}:1,                  hace falta 3:1"
            );
            let hoja_suelo = separacion(v.hoja, v.suelo);
            let hoja_lienzo = separacion(v.hoja, s.lienzo);
            assert!(
                hoja_suelo >= 3.0,
                "{tema}: la hoja y el suelo sólo se separan {hoja_suelo:.1} L*,                  hace falta 3"
            );
            assert!(
                hoja_lienzo >= 3.0,
                "{tema}: la hoja y el lienzo sólo se separan {hoja_lienzo:.1} L*,                  hace falta 3"
            );
            // Lo que en oscuro queda por debajo del gris medio en claro
            // tiene que quedar por encima: ese es el fallo de «asumir
            // fondo oscuro» que la re-review medía píxel a píxel.
            for (nombre, color) in [("la hoja", v.hoja), ("el suelo", v.suelo)] {
                assert_eq!(
                    luz_percibida(color) > 50.0,
                    !dark_mode,
                    "{tema}: {nombre} no pinta su tema"
                );
            }
        }
    }

    /// D6: el botón destructivo del diálogo de salida va relleno con el
    /// rojo del tema y el sugerido con el acento, y el texto que llevan
    /// encima lo escoge `texto_sobre`. Ese rojo no es el mismo en los dos
    /// temas —en claro (170, 0, 0), oscuro, y en oscuro (255, 90, 90),
    /// claro—, así que fijar «blanco sobre rojo» serviría en uno y en el
    /// otro se quedaría en 2,57:1.
    #[test]
    fn el_texto_de_un_boton_relleno_cumple_wcag_aa_en_los_dos_temas() {
        for dark_mode in temas() {
            let tema = if dark_mode { "oscuro" } else { "claro" };
            let v = aplicado(if dark_mode {
                crate::ui_prefs::Theme::Dark
            } else {
                crate::ui_prefs::Theme::Light
            });
            for (nombre, fondo) in [
                ("destructivo", v.error_fg_color),
                ("sugerido", v.selection.bg_fill),
            ] {
                let r = razon(texto_sobre(fondo), fondo);
                assert!(
                    r >= 4.5,
                    "{tema}: el texto del botón {nombre} sobre su relleno son \
                     {r:.2}:1, hace falta 4,5:1"
                );
            }
        }
    }
}
