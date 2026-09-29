//! Minimal drag-and-drop test: create a window, keep it painted, and print every
//! file drag-and-drop event winit emits.
//!
//! This is a *pure winit* example (no egui / no smithay-clipboard), so winit's data
//! device is the only one on the seat and the compositor must route file drops to it.
//! Run with:  cargo run --example dnd_test --features "wayland,x11,rwh_05,rwh_06"

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

#[path = "util/fill.rs"]
mod fill;

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes()
            .with_title("winit DnD test — drop files here")
            .with_inner_size(winit::dpi::LogicalSize::new(480.0, 320.0));
        self.window = Some(Arc::new(event_loop.create_window(attributes).unwrap()));
        println!("[dnd_test] window created — drag a file onto it");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::HoveredFile(path) => println!("HOVERED  : {}", path.display()),
            WindowEvent::HoveredFileCancelled => println!("HOVER CANCELLED"),
            WindowEvent::DroppedFile(path) => println!("DROPPED  : {}", path.display()),
            WindowEvent::RedrawRequested => {
                if let Some(window) = self.window.as_ref() {
                    // Attaching a buffer maps the window so it can receive drag events.
                    fill::fill_window(window);
                }
            },
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {},
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
}
