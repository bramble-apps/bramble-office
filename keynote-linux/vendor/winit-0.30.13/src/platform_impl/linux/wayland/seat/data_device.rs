//! File drag and drop on Wayland (wl_data_device), reported as `WindowEvent::DroppedFile` /
//! `HoveredFileCancelled` like on X11, with `CursorMoved` while the drag moves over the window
//! (the pointer itself leaves the window during a drag, so the app still knows where the files
//! are). Added for keynote-linux: winit 0.30 has no drag and drop on Wayland at all.
//!
//! Only files (`text/uri-list` with `file:` URIs) are taken, as on X11. The list is read
//! without blocking: the pipe is a calloop source, and the events go out when it has all come.

use std::io::Read;
use std::path::PathBuf;

use sctk::data_device_manager::data_device::{DataDeviceHandler, DataDevice};
use sctk::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use sctk::data_device_manager::data_source::DataSourceHandler;
use sctk::data_device_manager::{DataDeviceManagerState, WritePipe};
use sctk::reexports::calloop::generic::Generic;
use sctk::reexports::calloop::{Interest, Mode, PostAction};
use sctk::reexports::client::protocol::wl_data_device::WlDataDevice;
use sctk::reexports::client::protocol::wl_data_device_manager::DndAction;
use sctk::reexports::client::protocol::wl_data_source::WlDataSource;
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Proxy, QueueHandle};

use sctk::compositor::SurfaceData;

use crate::dpi::{LogicalPosition, PhysicalPosition};
use crate::event::WindowEvent;
use crate::platform_impl::wayland::state::WinitState;
use crate::platform_impl::wayland::{self, DeviceId, WindowId};

const URI_LIST: &str = "text/uri-list";

/// The data device global and the drag in progress.
pub struct DataDeviceState {
    manager: DataDeviceManagerState,
    /// the window a drag of files is over
    over: Option<WindowId>,
}

impl DataDeviceState {
    pub fn new(manager: DataDeviceManagerState) -> Self {
        Self { manager, over: None }
    }

    /// The data device of a seat.
    pub fn device(&self, seat: &WlSeat, queue_handle: &QueueHandle<WinitState>) -> DataDevice {
        self.manager.get_data_device(queue_handle, seat)
    }
}

/// The files a `text/uri-list` names.
fn paths(list: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(list)
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("file://"))
        .filter_map(|l| l.strip_prefix("localhost").or(Some(l)).filter(|l| l.starts_with('/')))
        .filter_map(|l| {
            let b = l.as_bytes();
            let mut out = Vec::with_capacity(b.len());
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'%' && i + 2 < b.len() {
                    out.push(u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).ok()?, 16).ok()?);
                    i += 3;
                } else {
                    out.push(b[i]);
                    i += 1;
                }
            }
            String::from_utf8(out).ok()
        })
        .map(PathBuf::from)
        .collect()
}

impl WinitState {
    fn drag_offer(data_device: &WlDataDevice) -> Option<DragOffer> {
        data_device.data::<sctk::data_device_manager::data_device::DataDeviceData>()?.drag_offer()
    }

    /// The window under a drag and the drag's point in it (physical pixels).
    fn drag_window(&mut self, surface: &WlSurface, x: f64, y: f64) -> Option<(WindowId, PhysicalPosition<f64>)> {
        let parent = surface.data::<SurfaceData>().and_then(|d| d.parent_surface().cloned()).unwrap_or_else(|| surface.clone());
        let window_id = wayland::make_wid(&parent);
        let scale = self.windows.get_mut().get(&window_id)?.lock().unwrap().scale_factor();
        Some((window_id, LogicalPosition::new(x, y).to_physical(scale)))
    }

    fn device_id() -> crate::event::DeviceId {
        crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(DeviceId))
    }
}

impl DataDeviceHandler for WinitState {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, data_device: &WlDataDevice, x: f64, y: f64, surface: &WlSurface) {
        let Some(offer) = Self::drag_offer(data_device) else { return };
        let files = offer.with_mime_types(|m| m.iter().any(|t| t == URI_LIST));
        let Some((window_id, position)) = self.drag_window(surface, x, y) else {
            offer.accept_mime_type(offer.serial, None);
            return;
        };
        if !files {
            offer.accept_mime_type(offer.serial, None);
            return;
        }
        offer.accept_mime_type(offer.serial, Some(URI_LIST.into()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);
        if let Some(dd) = self.data_device.as_mut() {
            dd.over = Some(window_id);
        }
        let device_id = Self::device_id();
        self.events_sink.push_window_event(WindowEvent::CursorEntered { device_id }, window_id);
        self.events_sink.push_window_event(WindowEvent::CursorMoved { device_id, position }, window_id);
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, data_device: &WlDataDevice) {
        let Some(window_id) = self.data_device.as_mut().and_then(|d| d.over.take()) else { return };
        // (after a drop the offer stays until its files are read)
        if Self::drag_offer(data_device).is_some_and(|o| o.dropped) {
            return;
        }
        self.events_sink.push_window_event(WindowEvent::HoveredFileCancelled, window_id);
        self.events_sink.push_window_event(WindowEvent::CursorLeft { device_id: Self::device_id() }, window_id);
    }

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, data_device: &WlDataDevice, x: f64, y: f64) {
        if self.data_device.as_ref().and_then(|d| d.over).is_none() {
            return;
        }
        let Some(offer) = Self::drag_offer(data_device) else { return };
        if let Some((window_id, position)) = self.drag_window(&offer.surface, x, y) {
            self.events_sink.push_window_event(WindowEvent::CursorMoved { device_id: Self::device_id(), position }, window_id);
        }
    }

    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn drop_performed(&mut self, conn: &Connection, _: &QueueHandle<Self>, data_device: &WlDataDevice) {
        let Some(window_id) = self.data_device.as_mut().and_then(|d| d.over.take()) else { return };
        let Some(offer) = Self::drag_offer(data_device) else { return };
        let Ok(pipe) = offer.receive(URI_LIST.into()) else {
            offer.finish();
            offer.destroy();
            return;
        };
        let _ = conn.flush();
        let file: std::fs::File = std::os::fd::OwnedFd::from(pipe).into();
        let mut list = Vec::new();
        let source = Generic::new(file, Interest::READ, Mode::Level);
        let added = self.loop_handle.insert_source(source, move |_, file, state: &mut WinitState| {
            let mut buf = [0u8; 8192];
            // SAFETY: the file is only read, never closed or replaced, here
            match unsafe { file.get_mut() }.read(&mut buf) {
                Ok(n) if n > 0 => {
                    list.extend_from_slice(&buf[..n]);
                    return Ok(PostAction::Continue);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted || e.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(PostAction::Continue);
                }
                _ => {}
            }
            for path in paths(&list) {
                state.events_sink.push_window_event(WindowEvent::DroppedFile(path), window_id);
            }
            offer.finish();
            offer.destroy();
            Ok(PostAction::Remove)
        });
        if added.is_err() {
            tracing::warn!("couldn't read the dropped files");
        }
    }
}

impl DataOfferHandler for WinitState {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, offer: &mut DragOffer, _: DndAction) {
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }

    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
}

// (winit offers nothing to other apps' drops)
impl DataSourceHandler for WinitState {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: Option<String>) {}
    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: String, _: WritePipe) {}
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}

sctk::delegate_data_device!(WinitState);

#[cfg(test)]
mod tests {
    #[test]
    fn uri_lists() {
        let l = b"file:///home/me/My%20Pictures/a%C3%A9.png\r\nfile://localhost/tmp/b.pdf\r\nhttps://x.org/c\r\n";
        assert_eq!(super::paths(l), vec![std::path::PathBuf::from("/home/me/My Pictures/a\u{e9}.png"), std::path::PathBuf::from("/tmp/b.pdf")]);
    }
}
