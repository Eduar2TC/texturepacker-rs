//! Global visual style: theme bootstrap and the accent colors shared by
//! every panel of the window.

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

/// Estilo visual global: esquinas suaves, selección y enlaces teñidos con el
/// acento, sliders rellenos. Se aplica a los estilos oscuro y claro a la vez
/// y sólo la primera vez (los valores no dependen del tema elegido).
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
