//! Estilo visual global: arranque del tema, acentos compartidos por todos
//! los paneles de la ventana y los **tokens de superficie** con los que
//! cada zona (barras, docks, lienzo) se pinta con su propio rol.

use eframe::egui;

/// Verde del resaltado «recién añadido», legible sobre los dos temas.
pub(crate) fn just_added_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(130, 220, 160)
    } else {
        egui::Color32::from_rgb(0, 132, 74)
    }
}

/// Ámbar de aviso (carpetas inteligentes, «empaqueta sola», avisos).
pub(crate) fn amber_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(230, 190, 60)
    } else {
        egui::Color32::from_rgb(150, 105, 0)
    }
}

/// Azul informativo (carpetas anidadas, «publicando…», «actualizando…»).
pub(crate) fn info_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(120, 170, 255)
    } else {
        egui::Color32::from_rgb(20, 100, 205)
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
pub(crate) fn muted_hover_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(180, 225, 255)
    } else {
        egui::Color32::from_rgb(20, 100, 205)
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
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.4 };
        v.collapsing_header_frame = true;
    });
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new("tp_theme"), true));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    /// Luminosidad relativa (WCAG 2.1): 0 = negro, 1 = blanco.
    fn luminancia(c: egui::Color32) -> f32 {
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
    fn razon(a: egui::Color32, b: egui::Color32) -> f32 {
        let (la, lb) = (luminancia(a), luminancia(b));
        let (alto, bajo) = if la > lb { (la, lb) } else { (lb, la) };
        (alto + 0.05) / (bajo + 0.05)
    }

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
}
