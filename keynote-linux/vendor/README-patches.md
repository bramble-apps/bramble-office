# Vendored crates

## smithay-client-toolkit 0.19.2

One change, in `src/seat/pointer/mod.rs` (`set_cursor_legacy`): the cursor image scale is the
larger of the surface's preferred buffer scale and the scale of the outputs the cursor surface has
entered. GNOME 46 (no cursor-shape protocol) sends `preferred_buffer_scale` for windows but not
for cursor surfaces, so without this the cursor is loaded at scale 1 and looks blurry at 200 %.
Found with `WAYLAND_DEBUG=1`: `wl_output.scale(2)`, cursor `wl_surface.enter`, but
`set_buffer_scale(1)` on every `set_cursor`.

## winit 0.30.13

winit 0.30 reports touchpad pinch (`WindowEvent::PinchGesture`) only on macOS and iOS, so on
Linux pinch-to-zoom never reached the app. Added `src/platform_impl/linux/wayland/seat/pointer/
pointer_gestures.rs`: binds `zwp_pointer_gestures_v1`, creates a pinch object per pointer
(`seat/mod.rs`, global in `state.rs`) and turns begin / update / end into `PinchGesture` events
with `delta = ln(scale / previous scale)`, which egui-winit turns into `egui::Event::Zoom`.
Check with `WAYLAND_DEBUG=1`: `get_pinch_gesture` right after the pointer is created.

macOS: `application:openURLs:` added to the app delegate (`src/platform_impl/macos/app_state.rs`),
queuing the paths of documents the system asks the app to open (Finder Open With, `open -a`, a
file dropped on the Dock icon); the app takes them with `winit::platform::macos::take_opened_files()`
(`src/platform/macos.rs`). objc2-foundation gains the `NSURL` feature. Upstream winit 0.30 drops
these Apple events, so a document opened from Finder only brought up the app.

Wayland file drag and drop: winit 0.30 has none on Wayland (files dragged from Files / Nautilus
were simply refused). Added `src/platform_impl/linux/wayland/seat/data_device.rs`: binds
`wl_data_device_manager` (`state.rs`), a data device per seat (`seat/mod.rs`, with the pointer),
accepts drags offering `text/uri-list` (action Copy), reports `CursorMoved` while the drag moves
over the window (the pointer itself leaves during a drag) and, on drop, reads the list through a
calloop source (no blocking) and sends one `WindowEvent::DroppedFile` per `file:` path, as on
X11; a drag that leaves sends `HoveredFileCancelled`. Check with `WAYLAND_DEBUG=1`:
`wl_data_device_manager.get_data_device` after the pointer, and `wl_data_offer.accept(…,
"text/uri-list")` on enter.
