//! Regresión de interacción del árbol: el clic y el arrastre deben convivir
//! en la misma fila. El patrón roto (un overlay `ui.interact` con id distinto
//! sobre el rect de la fila) robaba el press: las filas quedaban muertas.

use eframe::egui;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Probe {
    clicked_row: Option<usize>,
    dragged_row: Option<usize>,
    rects: Vec<egui::Rect>,
}

fn frame(ctx: &egui::Context, probe: &Arc<Mutex<Probe>>, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        events,
        ..egui::RawInput::default()
    };
    let _full_output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let names = [
                "bg.png",
                "btn_ko.png",
                "btn_ok.png",
                "hero.png",
                "smoke.png",
            ];
            for (i, name) in names.iter().enumerate() {
                let resp = ui.selectable_label(false, *name);
                if probe.lock().unwrap().rects.len() <= i {
                    probe.lock().unwrap().rects.push(resp.rect);
                }
                if resp.clicked() {
                    probe.lock().unwrap().clicked_row = Some(i);
                }
                // Patrón corregido: extender la respuesta de la fila con el
                // mismo id (los sentidos se fusionan; el clic no se pierde).
                let dnd = resp.interact(egui::Sense::drag());
                if dnd.dragged() {
                    probe.lock().unwrap().dragged_row = Some(i);
                }
            }
        });
    });
}

fn press_move_release(from: egui::Pos2, to: egui::Pos2) -> Vec<Vec<egui::Event>> {
    let mut frames = vec![vec![egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    }]];
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        frames.push(vec![egui::Event::PointerMoved(egui::pos2(
            from.x + (to.x - from.x) * t,
            from.y + (to.y - from.y) * t,
        ))]);
    }
    frames.push(vec![egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    }]);
    frames
}

#[test]
fn drag_starts_in_pressed_row_and_click_still_selects() {
    let ctx = egui::Context::default();
    let probe = Arc::new(Mutex::<Probe>::default());
    frame(&ctx, &probe, vec![]);

    let rects = probe.lock().unwrap().rects.clone();
    let from = rects[3].center(); // hero.png
    let to = egui::pos2(from.x, from.y - 200.0); // hacia arriba, cruzando filas

    for evs in press_move_release(from, to) {
        frame(&ctx, &probe, evs);
    }
    let p = probe.lock().unwrap();
    assert_eq!(
        p.dragged_row,
        Some(3),
        "la fila donde se presionó debe reportar el drag, pese a cruzar vecinas"
    );
    assert!(
        p.clicked_row.is_none(),
        "un drag no debe contar como clic de selección"
    );
}

#[test]
fn plain_click_selects_the_row() {
    let ctx = egui::Context::default();
    let probe = Arc::new(Mutex::<Probe>::default());
    frame(&ctx, &probe, vec![]);

    let rects = probe.lock().unwrap().rects.clone();
    let pos = rects[0].center(); // bg.png

    frame(
        &ctx,
        &probe,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }],
    );
    frame(
        &ctx,
        &probe,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }],
    );
    let p = probe.lock().unwrap();
    assert_eq!(
        p.clicked_row,
        Some(0),
        "un clic simple debe seleccionar la fila"
    );
    assert!(p.dragged_row.is_none());
}
