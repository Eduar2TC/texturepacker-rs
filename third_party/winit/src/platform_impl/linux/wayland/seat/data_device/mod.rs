//! Wayland native drag-and-drop (file drop) handling.
//!
//! This wires `wl_data_device` (via smithay-client-toolkit's `data_device_manager`)
//! into winit's classic `WindowEvent::HoveredFile` / `HoveredFileCancelled` /
//! `DroppedFile` events, so that files dragged from a file manager onto a Wayland
//! window are delivered exactly like they already are on X11 (XDND).
//!
//! Only the *receiving* side is implemented (winit does not expose a drag source).
//! We accept the `text/uri-list` mime type, read the offered payload and turn each
//! `file://` URI into a `PathBuf`.
//!
//! NOTE: reading the offer payload is done with a blocking read on the pipe fd.
//! This mirrors the approach of the long-standing upstream PR #2429 and is fine for
//! typical file-manager sources, which write the (small) uri-list promptly. A fully
//! async implementation would register the `ReadPipe` with the calloop event loop.

use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use sctk::data_device_manager::data_device::{DataDeviceData, DataDeviceHandler};
use sctk::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use sctk::data_device_manager::data_source::DataSourceHandler;
use sctk::data_device_manager::WritePipe;
use sctk::reexports::client::protocol::wl_data_device::WlDataDevice;
use sctk::reexports::client::protocol::wl_data_device_manager::DndAction;
use sctk::reexports::client::protocol::wl_data_source::WlDataSource;
use sctk::reexports::client::{Connection, Proxy, QueueHandle};

use crate::event::WindowEvent;
use crate::platform_impl::wayland::make_wid;
use crate::platform_impl::wayland::state::WinitState;

/// The mime type winit understands for file drops.
const URI_LIST_MIME: &str = "text/uri-list";

impl DataDeviceHandler for WinitState {
    fn enter(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
        surface: &sctk::reexports::client::protocol::wl_surface::WlSurface,
    ) {
        let window_id = make_wid(surface);

        // The drag may enter server-side-decoration subsurfaces (title bar, borders)
        // which are not real winit windows; ignore those so we never emit events for a
        // phantom `WindowId`.
        if !self.windows.borrow().contains_key(&window_id) {
            return;
        }

        // Fetch the drag offer created by sctk for this data device.
        let offer = match data_device.data::<DataDeviceData>().and_then(|d| d.drag_offer()) {
            Some(offer) => offer,
            None => return,
        };

        // We only handle file drops advertised as `text/uri-list`.
        let has_uri_list = offer.with_mime_types(|mimes| mimes.iter().any(|m| m == URI_LIST_MIME));
        if !has_uri_list {
            return;
        }

        // Accept the offer and advertise that we'll perform a copy so the source
        // settles on an action (required for `finish()` on drop to be valid).
        offer.accept_mime_type(offer.serial, Some(URI_LIST_MIME.to_string()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);

        // Read the offered uri-list now so we can report the hovered paths.
        let paths = read_uri_list(conn, &offer);

        self.dnd_window = Some(window_id);
        self.dnd_paths = paths.clone();

        for path in paths {
            self.events_sink.push_window_event(WindowEvent::HoveredFile(path), window_id);
        }
        self.dispatched_events = true;
    }

    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _data_device: &WlDataDevice) {
        // A `leave` that follows a `drop` is part of normal teardown and the window
        // was already cleared in `drop_performed`; only emit the cancel when we're
        // still tracking an active hover.
        if let Some(window_id) = self.dnd_window.take() {
            self.events_sink.push_window_event(WindowEvent::HoveredFileCancelled, window_id);
            self.dispatched_events = true;
        }
        self.dnd_paths.clear();
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
    ) {
        // winit 0.30's DnD API carries no hover-motion event, so there's nothing to emit.
    }

    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
        // Clipboard selection is out of scope for file drops.
    }

    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
    ) {
        let window_id = match self.dnd_window.take() {
            Some(window_id) => window_id,
            None => return,
        };

        // Tell the source the drop is complete. We must NOT destroy the offer here:
        // sctk owns its lifecycle, and destroying it races with trailing compositor
        // events (`action`, etc.) that would then reference a dead object and put the
        // whole Wayland connection into an unrecoverable error state. `finish()` is
        // only valid once an action has been negotiated (protocol version >= 3).
        if let Some(offer) = data_device.data::<DataDeviceData>().and_then(|d| d.drag_offer()) {
            if !offer.selected_action.is_empty() {
                offer.finish();
            }
        }

        // We already read the paths on `enter`; reuse them (the offer's pipe is spent
        // after the first read, so re-reading on drop yields nothing).
        let paths = std::mem::take(&mut self.dnd_paths);
        for path in paths {
            self.events_sink.push_window_event(WindowEvent::DroppedFile(path), window_id);
        }
        self.dispatched_events = true;
    }
}

impl DataOfferHandler for WinitState {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        offer: &mut DragOffer,
        actions: DndAction,
    ) {
        // Prefer a copy when the source offers it.
        let preferred =
            if actions.contains(DndAction::Copy) { DndAction::Copy } else { actions };
        offer.set_actions(preferred, preferred);
    }

    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: DndAction,
    ) {
    }
}

// winit never creates a data source, so every callback here is an unreachable no-op.
// The impl only exists to satisfy `delegate_data_device!`.
impl DataSourceHandler for WinitState {
    fn accept_mime(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: String,
        _fd: WritePipe,
    ) {
    }

    fn cancelled(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {}

    fn dnd_dropped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {}

    fn dnd_finished(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {
    }

    fn action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _action: DndAction,
    ) {
    }
}

/// Request the `text/uri-list` payload from a drag offer and parse it into paths.
///
/// The read is blocking; we flush the connection first so the `receive` request
/// actually reaches the source before we wait on the pipe.
fn read_uri_list(conn: &Connection, offer: &DragOffer) -> Vec<PathBuf> {
    let pipe = match offer.receive(URI_LIST_MIME.to_string()) {
        Ok(pipe) => pipe,
        Err(err) => {
            tracing::warn!("failed to receive drag offer: {err}");
            return Vec::new();
        },
    };

    // Make sure the `receive` request is actually sent to the compositor/source.
    let _ = conn.flush();

    let mut reader = std::io::BufReader::new(pipe);
    let mut buffer = Vec::new();
    if let Err(err) = reader.read_to_end(&mut buffer) {
        tracing::warn!("failed to read drag offer payload: {err}");
        return Vec::new();
    }

    parse_uri_list(&buffer)
}

/// Parse a `text/uri-list` payload into file paths.
///
/// Handles both CRLF (Sway) and LF (GNOME/KDE) line endings, skips comment lines
/// (those starting with `#`), and only keeps `file://` URIs.
fn parse_uri_list(bytes: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(uri_to_path)
        .collect()
}

/// Convert a single `file://[host]/path` URI into a `PathBuf`, percent-decoding it.
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Strip an optional authority/host component: `file://host/path` -> `/path`.
    let path_part = match rest.find('/') {
        Some(idx) => &rest[idx..],
        None => rest,
    };
    let decoded = percent_decode(path_part);
    Some(PathBuf::from(std::ffi::OsString::from_vec(decoded)))
}

/// Minimal percent-decoding (`%20` -> space) that works on raw bytes so non-UTF-8
/// paths survive. Avoids pulling in an extra dependency.
fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
