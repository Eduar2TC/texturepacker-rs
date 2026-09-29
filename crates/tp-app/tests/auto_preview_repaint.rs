//! Reproduce el bucle de repaint de eframe (ventana real): la app solo
//! avanza cuando egui pide repaint (`set_request_repaint_callback`). Los
//! tests existentes bombean frames a mano cada 10-25 ms y enmascaran
//! cualquier fallo de programación del preview/debounce: aquí la vista
//! previa debe llegar sola, sin pulsar Publicar y sin input del usuario.

use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tp_app::{testing::create_example_project, App};

fn tmp_project(tag: &str) -> (App, egui::Context, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "tp_repaint_{tag}_{}_{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let sprites = tmp.join("sprites");
    std::fs::create_dir_all(&sprites).unwrap();
    let project = create_example_project(&tmp, &sprites).expect("proyecto de ejemplo");
    let ctx = egui::Context::default();
    let app = App::new_for_testing(ctx.clone(), Some(project));
    (app, ctx, tmp)
}

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1360.0, 860.0),
        )),
        time: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        ),
        events,
        ..egui::RawInput::default()
    }
}

/// Conduce la app como la ventana real: solo pinta cuando egui lo pide.
/// Devuelve (frames, se quedó sin repaint, frames_idle).
fn pump(app: &mut App, ctx: &egui::Context, max: Duration) -> (usize, bool, usize) {
    let pending: Arc<Mutex<Option<Duration>>> = Arc::new(Mutex::new(None));
    let slot = pending.clone();
    ctx.set_request_repaint_callback(move |info| {
        let mut g = slot.lock().unwrap();
        *g = Some(match *g {
            Some(prev) => prev.min(info.delay),
            None => info.delay,
        });
    });

    let deadline = Instant::now() + max;
    let mut frames = 0usize;
    let mut idle = 0usize;
    loop {
        if app.result().is_some() {
            return (frames, false, idle);
        }
        if Instant::now() > deadline {
            return (frames, true, idle);
        }
        *pending.lock().unwrap() = None;
        let _ = app.run_frame(ctx, input(vec![]));
        frames += 1;
        let wait = pending.lock().unwrap().take();
        match wait {
            Some(delay) => {
                idle = 0;
                std::thread::sleep(delay.max(Duration::from_millis(1)));
            }
            None => {
                // La ventana real se quedaría quieta aquí.
                idle += 1;
                if idle >= 3 {
                    return (frames, true, idle);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[test]
fn preview_arrives_on_demand_without_publishing() {
    let (mut app, ctx, tmp) = tmp_project("startup");
    let (frames, stuck, idle) = pump(&mut app, &ctx, Duration::from_secs(10));
    assert!(
        app.result().is_some(),
        "la vista previa debe llegar sola al arrancar (sin pulsar Publicar): \
         frames={frames} stuck={stuck} idle_frames={idle}"
    );
    std::fs::remove_dir_all(&tmp).ok();
}
