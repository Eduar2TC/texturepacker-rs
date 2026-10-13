//! 4.3: la rejilla de 8 px, medida en el frame de verdad.
//!
//! El paso 5 no reescribe ninguna función: cambia tres números del diseño
//! que convivían con otros tantos que no eran múltiplos de 8 —`add_space(2)`,
//! `(3)`, `(4)` y `(6)` dentro del mismo panel—. Aquí se comprueban los
//! tres:
//!
//! 1. **Márgenes de 8 px**: los dos docks respiran 8 px entre su borde y
//!    su contenido. El marco de egui por defecto es `Frame::default()`,
//!    con margen 0, así que el árbol nacía pegado al borde izquierdo y
//!    «Ajustes» a 4 px del suyo; las bandas de arriba ya llevaban 8.
//! 2. **Separadores de grupo**: `ui.separator()` se lleva 6 px y su línea
//!    cae en el centro, así que el aire entre los dos grupos era de 9 px
//!    en una pila y de 20 en una fila. Ahora son 16 y 24.
//! 3. **Cabeceras de 24 px**: las secciones de Ajustes medían 17 px, que
//!    es lo que `CollapsingHeader` reserva con el texto de serie; 24 son
//!    el objetivo de puntero del paso 4 y el siguiente peldaño de la
//!    rejilla.
//!
//! Todas las medidas se hacen sobre lo pintado —rellenos, que son
//! rectángulos exactos, y cajas de galión, que sí miden la línea—: la
//! tinta de un glifo no sirve para medir, porque cada letra empieza a su
//! altura y una tilde desplaza el resultado tres píxeles.

use super::*;
use crate::testing::{controles_de, galones_pintados, rellenos_pintados};

/// Un frame entero con la ventana dada: los docks sólo existen dentro del
/// montaje de la app, que es donde viven sus marcos.
fn frame_en(app: &mut App, ctx: &egui::Context, ancho: f32, alto: f32) -> eframe::egui::FullOutput {
    let mut input = crate::testing::idle_input();
    input.screen_rect = Some(eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(ancho, alto),
    ));
    app.run_frame(ctx, input)
}

/// El rectángulo pintado con el color de `superficie` que contiene al
/// punto dado y que es más pequeño: el panel que lo aloja, es decir, el
/// borde de fuera, el que el margen separa de la primera letra.
fn panel_de(
    salida: &eframe::egui::FullOutput,
    superficie: egui::Color32,
    punto: egui::Pos2,
) -> egui::Rect {
    rellenos_pintados(salida)
        .into_iter()
        .filter(|(color, rect)| *color == superficie && rect.contains(punto))
        .map(|(_, rect)| rect)
        .min_by_key(|rect| {
            (
                (rect.width() * 1000.0) as i32,
                (rect.height() * 1000.0) as i32,
            )
        })
        .unwrap_or_else(|| panic!("no hay panel pintado que contenga a {punto:?}"))
}

/// El rectángulo pintado más pequeño que contiene al punto dado: el
/// widget que lo aloja. Un `TextEdit` llena su marco entero de
/// `extreme_bg_color`, de modo que su relleno **es** su rectángulo y se
/// puede medir contra el del panel sin incógnitas.
fn relleno_de(salida: &eframe::egui::FullOutput, punto: egui::Pos2) -> egui::Rect {
    rellenos_pintados(salida)
        .into_iter()
        .filter(|(_, rect)| rect.contains(punto))
        .map(|(_, rect)| rect)
        .min_by_key(|rect| {
            (
                (rect.width() * 1000.0) as i32,
                (rect.height() * 1000.0) as i32,
            )
        })
        .unwrap_or_else(|| panic!("no hay relleno pintado que contenga a {punto:?}"))
}

/// El primer galión cuyo texto es exactamente `texto`, con su caja.
fn galion_de(salida: &eframe::egui::FullOutput, texto: &str) -> egui::Rect {
    galones_pintados(salida)
        .into_iter()
        .find(|(t, _)| t.trim() == texto)
        .map(|(_, rect)| rect)
        .unwrap_or_else(|| panic!("«{texto}» no está pintado"))
}

/// 4.3: los dos docks dejan 8 px entre su borde y su contenido.
///
/// El margen va en el marco del `SidePanel` y no en un `add_space`: así
/// cubre también el pie y deja el `add_space` de la jerarquía para lo que
/// es —un salto dentro del contenido—. Se mide de tres maneras: el primer
/// galión del dock derecho (techo e izquierda), los dos campos de filtro
/// (los dos lados, porque un `TextEdit` ocupa el ancho útil entero) y el
/// primer control del dock izquierdo (su techo).
///
/// El pie no tiene con qué medirse —la rejilla no pinta nada allí abajo—,
/// pero es la misma constante que el techo: `Margin::same(8)`.
#[test]
fn los_docks_dejan_ocho_px_entre_su_borde_y_su_contenido() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    let salida = frame_en(&mut app, &ctx, 1360.0, 860.0);
    let superficie = superficies(ctx.style().visuals.dark_mode).panel;

    // Techo e izquierda del dock derecho: «Ajustes» es lo primero que
    // pinta, pegado al margen.
    let ajustes = galion_de(&salida, "Ajustes");
    let panel = panel_de(&salida, superficie, ajustes.center());
    assert_eq!(
        (ajustes.min.x - panel.min.x, ajustes.min.y - panel.min.y),
        (8.0, 8.0),
        "«Ajustes» nace a {:?} px del borde de su panel {panel:?}",
        (ajustes.min.x - panel.min.x, ajustes.min.y - panel.min.y)
    );

    // Los dos lados, en los dos docks: el campo de filtro es lo ancho que
    // es el contenido útil.
    for campo in ["Buscar ajuste…", "Filtrar sprites…"] {
        let relleno = relleno_de(&salida, galion_de(&salida, campo).center());
        let panel = panel_de(&salida, superficie, relleno.center());
        let lados = (relleno.min.x - panel.min.x, panel.max.x - relleno.max.x);
        assert_eq!(
            lados,
            (8.0, 8.0),
            "el campo «{campo}» no está a 8 px de los dos lados: relleno \
             {relleno:?}, panel {panel:?}"
        );
    }

    // Techo del dock izquierdo: en su fila de cabecera el control más
    // alto es el «+» de desplegar, así que su relleno empieza exactamente
    // donde empieza la fila.
    let mas = controles_de(&salida, "+")
        .into_iter()
        .find(|rect| rect.max.x < 250.0)
        .expect("el «+» del árbol debe estar pintado");
    let panel = panel_de(&salida, superficie, mas.center());
    assert_eq!(
        mas.min.y - panel.min.y,
        8.0,
        "la cabecera del árbol nace a {} px del techo de su panel {panel:?}",
        mas.min.y - panel.min.y
    );
}

/// 4.3: un separador de grupo abre 16 px de aire en una pila.
///
/// Se mide de borde a borde —relleno de un botón a caja del galión de una
/// etiqueta—, que es como se ve: el relleno de un botón es su rectángulo
/// entero y la caja del galión, el de su línea, así que la resta no
/// lleva incógnitas. Es el diálogo de «Dividir hoja», donde el aviso de
/// espera va justo debajo del botón de elegir hoja.
#[test]
fn el_separador_de_grupo_abre_dieciseis_px_en_pila() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    app.show_split = true;
    let _ = frame_en(&mut app, &ctx, 1360.0, 860.0);
    // Los `egui::Window` sólo pintan a partir del segundo frame: el
    // primero sirve para que egui les asigne su área.
    let salida = frame_en(&mut app, &ctx, 1360.0, 860.0);

    let boton = *controles_de(&salida, "Elegir hoja…")
        .first()
        .expect("«Elegir hoja…» debe estar pintado");
    let aviso = galion_de(
        &salida,
        "Elige una hoja (sprite sheet) para dividirla en sprites individuales.",
    );

    let hueco = aviso.min.y - boton.max.y;
    assert_eq!(
        hueco, 16.0,
        "entre el botón y el aviso hay {hueco:.0} px de aire y la rejilla \
         pide 16"
    );
}

/// 4.3: en una fila el mismo separador abre 24 px de aire.
///
/// En horizontal `item_spacing` mide 8 en vez de 3, así que el hueco
/// completo es 8 + separador + 8. Bajarlo de 24 exigiría un separador de
/// menos de 8 px, y a 0 egui ni llega a pintar la línea.
#[test]
fn el_separador_de_grupo_abre_veinticuatro_px_en_fila() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    let salida = frame_en(&mut app, &ctx, 1360.0, 860.0);

    let carpeta = *controles_de(&salida, "📁 Añadir carpeta…")
        .first()
        .expect("«📁 Añadir carpeta…» debe estar pintado");
    let sprite = *controles_de(&salida, "⚙ Sprite")
        .first()
        .expect("«⚙ Sprite» debe estar pintado");

    let hueco = sprite.min.x - carpeta.max.x;
    assert_eq!(
        hueco, 24.0,
        "entre los dos grupos de la barra hay {hueco:.0} px de aire y la \
         rejilla pide 24"
    );
}

/// 4.3: una cabecera de sección mide 24 px.
///
/// Se mide de galión a galión entre dos cabeceras seguidas y plegadas, que
/// sólo las separa el `item_spacing` del eje vertical: la resta menos ese
/// `item_spacing` es la altura de la cabecera. Con las 17 px de serie la
/// suma daba 20, y con la tinta no se podía medir, porque «Composición»
/// lleva tilde y «Datos» no.
#[test]
fn las_cabeceras_de_seccion_miden_veinticuatro_px() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    let salida = frame_en(&mut app, &ctx, 1360.0, 860.0);

    let composicion = galion_de(&salida, "Composición");
    let datos = galion_de(&salida, "Datos");

    let entre = datos.min.y - composicion.min.y;
    let altura = entre - ctx.style().spacing.item_spacing.y;
    assert_eq!(
        altura,
        24.0,
        "la cabecera de «Composición» mide {altura:.0} px ({entre:.0} de \
         galión a galión menos los {} de `item_spacing`)",
        ctx.style().spacing.item_spacing.y
    );
}

/// D5: los dos docks espejo montan la misma cabecera.
///
/// Tres medidas de la receta [`super::cabecera_dock`]: las dos filas
/// miden lo mismo —el «+» con su propio `min_size` y «Limpiar», que no
/// lo trae y sólo tiene el `interact_size` de la receta—, los dos
/// controles nacen a 8 px del techo de su panel, y el hueco entre el
/// título y el buscador es el mismo en las dos cajas, que es la forma de
/// ver que el separador va **después** del buscador en las dos y no antes
/// en una.
#[test]
fn las_dos_cabeceras_de_dock_siguen_la_misma_receta() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    let salida = frame_en(&mut app, &ctx, 1360.0, 860.0);
    let superficie = superficies(ctx.style().visuals.dark_mode).panel;

    let mas = controles_de(&salida, "+")
        .into_iter()
        .find(|rect| rect.max.x < 250.0)
        .expect("el «+» del árbol debe estar pintado");
    // El «Limpiar» del registro vive en el panel de abajo; el de la
    // cabecera, arriba del todo.
    let limpiar = controles_de(&salida, "Limpiar")
        .into_iter()
        .find(|rect| rect.min.y < 200.0)
        .expect("el «Limpiar» de Ajustes debe estar pintado");

    for (nombre, boton) in [("«+»", mas), ("«Limpiar»", limpiar)] {
        assert!(
            boton.height() >= OBJETIVO_MIN.y,
            "{nombre} mide {:.0} px de alto y la cabecera de dock pide {}",
            boton.height(),
            OBJETIVO_MIN.y
        );
        let panel = panel_de(&salida, superficie, boton.center());
        assert_eq!(
            boton.min.y - panel.min.y,
            8.0,
            "{nombre} no nace a 8 px del techo de su panel: {boton:?} sobre {panel:?}"
        );
    }

    let titulo_izq = galones_pintados(&salida)
        .into_iter()
        .find(|(texto, _)| texto.starts_with("Sprites ("))
        .map(|(_, rect)| rect)
        .expect("el título del dock izquierdo debe estar pintado");
    let titulo_der = galion_de(&salida, "Ajustes");
    let campo_izq = relleno_de(&salida, galion_de(&salida, "Filtrar sprites…").center());
    let campo_der = relleno_de(&salida, galion_de(&salida, "Buscar ajuste…").center());

    let hueco_izq = campo_izq.min.y - titulo_izq.max.y;
    let hueco_der = campo_der.min.y - titulo_der.max.y;
    assert!(
        (hueco_izq - hueco_der).abs() <= 0.5,
        "los dos docks dejan {hueco_izq:.1} y {hueco_der:.1} px entre su \
         título y su buscador: en uno de los dos hay algo en medio"
    );
}

/// D5: «Limpiar» vive en la cabecera y la caja del buscador no se mueve.
///
/// Con el botón dentro de la fila de la caja, el ancho de la caja
/// dependía de lo escrito: al primer carácter aparecía «Limpiar» y el
/// campo se estrechaba de unos 70 px de golpe, con el texto ya puesto
/// encima. Los dos frames —sin filtro y con— deben pintar exactamente la
/// misma caja.
#[test]
fn limpiar_no_estrecha_el_buscador_de_ajustes() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    let vacio = frame_en(&mut app, &ctx, 1360.0, 860.0);
    let caja_vacia = relleno_de(&vacio, galion_de(&vacio, "Buscar ajuste…").center());

    app.settings_filter = "composición".into();
    let escrito = frame_en(&mut app, &ctx, 1360.0, 860.0);
    let caja_llena = relleno_de(&escrito, galion_de(&escrito, "composición").center());

    assert_eq!(
        caja_vacia, caja_llena,
        "el buscador cambia de caja al aparecer «Limpiar»: con la caja a \
         la izquierda y el botón dentro de su fila, el ancho depende de \
         lo escrito"
    );
}

/// Las líneas verticales pintadas en `banda`, ordenadas por x: los
/// separadores de un grupo. egui las pinta como `LineSegment`.
fn separadores_en(salida: &eframe::egui::FullOutput, banda: egui::Rect) -> Vec<f32> {
    fn caminos(formas: &[eframe::egui::Shape], fuera: &mut Vec<eframe::egui::Rect>) {
        for forma in formas {
            match forma {
                eframe::egui::Shape::LineSegment { points, .. } => {
                    fuera.push(eframe::egui::Rect::from_points(points))
                }
                eframe::egui::Shape::Vec(hijos) => caminos(hijos, fuera),
                _ => {}
            }
        }
    }
    let pintadas: Vec<_> = salida.shapes.iter().map(|f| f.shape.clone()).collect();
    let mut v = Vec::new();
    caminos(&pintadas, &mut v);
    let mut xs: Vec<f32> = v
        .iter()
        .filter(|r| r.width() < 1.0 && r.height() > 4.0 && banda.contains(r.center()))
        .map(|r| r.center().x)
        .collect();
    xs.sort_by(|a, b| a.total_cmp(b));
    xs
}

/// D11: el porcentaje del zoom vive fuera del slider.
///
/// Como rótulo del propio widget (`Slider::text`) caía dentro de su
/// caja, a la izquierda del surco y pegado al «−», así que el par −/+
/// se partía por la mitad y el dato se leía como el rótulo del botón
/// equivocado. Aquí se mide que queda fuera del par, antes del «−».
#[test]
fn el_porcentaje_del_zoom_queda_fuera_del_par_de_botones() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    let salida = frame_en(&mut app, &ctx, 1360.0, 860.0);

    let porcentaje = galion_de(&salida, "Zoom 100 %");
    let menos = controles_de(&salida, "−")
        .into_iter()
        .filter(|r| r.min.y > 680.0)
        .max_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("el botón «−» del zoom debe estar pintado");
    let mas = controles_de(&salida, "+")
        .into_iter()
        .filter(|r| r.min.y > 680.0)
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("el botón «+» del zoom debe estar pintado");

    assert!(
        porcentaje.max.x <= menos.min.x,
        "el porcentaje ({porcentaje:?}) cae dentro del par −/+ ({menos:?}): \
         el dato debe quedar fuera del par, antes del «−»"
    );
    assert!(
        menos.min.x < mas.min.x,
        "el «−» va a la izquierda del «+»: {menos:?} y {mas:?}"
    );
}

/// D9: el indicador de frescura no mueve la barra de zoom.
///
/// Se pintaba al principio de una fila de ancho libre, así que aparecer
/// y desaparecer empujaba los menús y las páginas ~110 px cada vez que
/// la vista previa se recalculaba —el debounce del arrastre son 120 ms,
/// así que el salto se veía a cada paso del slider—. Vive ahora en una
/// ranura fija del grupo de la derecha, que se mide con el texto más
/// largo de los tres; aquí se mide que nadie se mueve, ni siquiera los
/// separadores, en los tres estados con indicador.
#[test]
fn el_indicador_de_frescura_no_desplaza_la_barra_de_zoom() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);

    fn medidas(salida: &eframe::egui::FullOutput) -> (egui::Rect, egui::Rect, Vec<f32>) {
        let vista = *controles_de(salida, "Vista")
            .first()
            .expect("el menú «Vista» debe estar pintado");
        let ajustar = *controles_de(salida, "Ajustar")
            .first()
            .expect("el botón «Ajustar» debe estar pintado");
        let banda = egui::Rect::from_min_max(
            egui::pos2(0.0, vista.min.y - 8.0),
            egui::pos2(1360.0, vista.max.y + 8.0),
        );
        (vista, ajustar, separadores_en(salida, banda))
    }

    // Estado limpio: sin indicador. Es el único frame en el que la
    // vista previa arranca fresca, antes de que el arranque la marque.
    let limpio = frame_en(&mut app, &ctx, 1360.0, 860.0);
    let (vista_ok, ajustar_ok, separadores_ok) = medidas(&limpio);
    assert!(
        separadores_ok.len() >= 2,
        "la barra de zoom lleva al menos dos separadores: {separadores_ok:?}"
    );

    // Desactualizado.
    app.preview_stale = true;
    let salida = frame_en(&mut app, &ctx, 1360.0, 860.0);
    let desactualizado = galion_de(&salida, "⚠ Desactualizado");
    assert!(
        desactualizado.min.x > vista_ok.max.x,
        "el indicador debe quedar a la derecha del menú «Vista»"
    );
    let (vista, ajustar, separadores) = medidas(&salida);
    assert_eq!(
        vista, vista_ok,
        "«Vista» se mueve al aparecer «⚠ Desactualizado»"
    );
    assert_eq!(
        ajustar, ajustar_ok,
        "«Ajustar» se mueve al aparecer «⚠ Desactualizado»"
    );
    assert_eq!(
        separadores, separadores_ok,
        "los separadores se mueven al aparecer «⚠ Desactualizado»"
    );

    // Una corrida en marcha: el receptor vive y no manda nada.
    let (tx, rx) = std::sync::mpsc::channel::<RunMessage>();
    let _tx = tx;
    app.running = Some(rx);
    let salida = frame_en(&mut app, &ctx, 1360.0, 860.0);
    let publicando = galion_de(&salida, "Publicando…");
    assert!(
        publicando.min.x > vista_ok.max.x,
        "el indicador debe quedar a la derecha del menú «Vista»"
    );
    let (vista, ajustar, separadores) = medidas(&salida);
    assert_eq!(
        vista, vista_ok,
        "«Vista» se mueve al aparecer «Publicando…»"
    );
    assert_eq!(
        ajustar, ajustar_ok,
        "«Ajustar» se mueve al aparecer «Publicando…»"
    );
    assert_eq!(
        separadores, separadores_ok,
        "los separadores se mueven al aparecer «Publicando…»"
    );
}
