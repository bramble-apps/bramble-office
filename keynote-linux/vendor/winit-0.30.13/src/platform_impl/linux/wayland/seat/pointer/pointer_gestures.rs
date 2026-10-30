//! Touchpad pinch on Wayland (zwp_pointer_gestures_v1), reported as `WindowEvent::PinchGesture`
//! like on macOS. Added for keynote-linux: winit 0.30 only has pinch on macOS / iOS.

use std::sync::Mutex;

use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use sctk::reexports::protocols::wp::pointer_gestures::zv1::client::{
    zwp_pointer_gesture_pinch_v1::{self, ZwpPointerGesturePinchV1},
    zwp_pointer_gestures_v1::ZwpPointerGesturesV1,
};

use sctk::compositor::SurfaceData;
use sctk::globals::GlobalData;

use crate::event::{TouchPhase, WindowEvent};
use crate::platform_impl::wayland::state::WinitState;
use crate::platform_impl::wayland::{self, DeviceId};

/// The pointer gestures global.
pub struct PointerGesturesState {
    manager: ZwpPointerGesturesV1,
}

impl PointerGesturesState {
    pub fn new(globals: &GlobalList, queue_handle: &QueueHandle<WinitState>) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=3, GlobalData)?;
        Ok(Self { manager })
    }

    /// A pinch gesture object for a pointer.
    pub fn pinch(&self, pointer: &sctk::reexports::client::protocol::wl_pointer::WlPointer, queue_handle: &QueueHandle<WinitState>) -> ZwpPointerGesturePinchV1 {
        self.manager.get_pinch_gesture(pointer, queue_handle, PinchData::default())
    }
}

/// The surface the pinch started on and the scale reported last.
#[derive(Default)]
pub struct PinchData {
    inner: Mutex<(Option<WlSurface>, f64)>,
}

impl Dispatch<ZwpPointerGesturesV1, GlobalData, WinitState> for PointerGesturesState {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZwpPointerGesturesV1,
        _event: <ZwpPointerGesturesV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
    }
}

impl Dispatch<ZwpPointerGesturePinchV1, PinchData, WinitState> for PointerGesturesState {
    fn event(
        state: &mut WinitState,
        _proxy: &ZwpPointerGesturePinchV1,
        event: zwp_pointer_gesture_pinch_v1::Event,
        data: &PinchData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        let mut inner = data.inner.lock().unwrap();
        // (phase, change of log scale since the last event)
        let (phase, delta) = match event {
            zwp_pointer_gesture_pinch_v1::Event::Begin { surface, .. } => {
                *inner = (Some(surface), 1.0);
                (TouchPhase::Started, 0.0)
            },
            zwp_pointer_gesture_pinch_v1::Event::Update { scale, .. } => {
                let prev = if inner.1 > 0.0 { inner.1 } else { 1.0 };
                let scale = if scale > 0.0 { scale } else { prev };
                inner.1 = scale;
                (TouchPhase::Moved, (scale / prev).ln())
            },
            zwp_pointer_gesture_pinch_v1::Event::End { cancelled, .. } => {
                (if cancelled != 0 { TouchPhase::Cancelled } else { TouchPhase::Ended }, 0.0)
            },
            _ => return,
        };
        let Some(surface) = inner.0.clone() else { return };
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            *inner = (None, 1.0);
        }
        drop(inner);
        let parent = match surface.data::<SurfaceData>() {
            Some(d) => d.parent_surface().cloned().unwrap_or(surface),
            None => surface,
        };
        let window_id = wayland::make_wid(&parent);
        let device_id = crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(DeviceId));
        state.events_sink.push_window_event(WindowEvent::PinchGesture { device_id, delta, phase }, window_id);
    }
}

delegate_dispatch!(WinitState: [ZwpPointerGesturesV1: GlobalData] => PointerGesturesState);
delegate_dispatch!(WinitState: [ZwpPointerGesturePinchV1: PinchData] => PointerGesturesState);
