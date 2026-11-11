//! A tiny VST3 plug-in for Studio's hosting tests (built by `cargo test`, never installed),
//! after the vst3 crate's gain example (MIT / Apache-2.0): "Test VST3 Gain", an effect with a
//! separate controller (the two talk through IConnectionPoint and a host-made IMessage), Gain
//! (normalized, shown in dB), Polarity (a two-step list), a 16-sample latency, state, and an
//! X11 editor that embeds a child window and registers a run-loop timer.

#![allow(non_upper_case_globals, non_snake_case, clippy::missing_safety_doc)]

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use vst3::Steinberg::Linux::*;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface, uid};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};

fn copy_cstring(src: &str, dst: &mut [c_char]) {
    let c = CString::new(src).unwrap_or_default();
    for (s, d) in c.as_bytes_with_nul().iter().zip(dst.iter_mut()) {
        *d = *s as c_char;
    }
}

fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let mut n = 0;
    for (s, d) in src.encode_utf16().zip(dst.iter_mut()) {
        *d = s;
        n += 1;
    }
    if n < dst.len() {
        dst[n] = 0;
    }
}

const LATENCY: usize = 16;
static TIMER_TICKS: AtomicU32 = AtomicU32::new(0);
/// The message the controller got from the component ("hello", n = 42), for the test.
static GOT_MESSAGE: AtomicU32 = AtomicU32::new(0);

fn gain_of(norm: f64) -> f64 {
    // 0..1 → -60..+12 dB
    10f64.powf((-60.0 + 72.0 * norm) / 20.0)
}

struct Processor {
    gain: AtomicU64,
    polarity: AtomicU64,
    line: Mutex<(Vec<[f32; 2]>, usize)>,
    peer: RefCell<Option<ComPtr<IConnectionPoint>>>,
    host: Cell<*mut FUnknown>,
}
unsafe impl Send for Processor {}
unsafe impl Sync for Processor {}

impl Class for Processor {
    type Interfaces = (IComponent, IAudioProcessor, IConnectionPoint);
}

impl Processor {
    const CID: TUID = uid(0x4A1B2C3D, 0x11223344, 0x55667788, 0x99AABBCC);
}

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, ctx: *mut FUnknown) -> tresult {
        self.host.set(ctx);
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IConnectionPointTrait for Processor {
    unsafe fn connect(&self, other: *mut IConnectionPoint) -> tresult {
        let Some(o) = (unsafe { ComRef::from_raw(other) }) else { return kInvalidArgument };
        *self.peer.borrow_mut() = Some(o.to_com_ptr());
        // say hello through a message the host makes
        let host = unsafe { ComRef::from_raw(self.host.get()) };
        if let Some(app) = host.and_then(|h| h.cast::<IHostApplication>()) {
            let mut cid = IMessage::IID;
            let mut iid = IMessage::IID;
            let mut obj: *mut c_void = std::ptr::null_mut();
            if unsafe { app.createInstance(cid.as_mut_ptr() as *mut TUID, iid.as_mut_ptr() as *mut TUID, &mut obj) } == kResultOk {
                if let Some(msg) = unsafe { ComPtr::<IMessage>::from_raw(obj as *mut IMessage) } {
                    unsafe {
                        msg.setMessageID(c"hello".as_ptr());
                        if let Some(a) = ComRef::from_raw(msg.getAttributes()) {
                            a.setInt(c"n".as_ptr(), 42);
                        }
                        o.notify(msg.as_ptr());
                    }
                }
            }
        }
        kResultOk
    }
    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        *self.peer.borrow_mut() = None;
        kResultOk
    }
    unsafe fn notify(&self, _m: *mut IMessage) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, id: *mut TUID) -> tresult {
        unsafe { *id = Controller::CID };
        kResultOk
    }
    unsafe fn setIoMode(&self, _m: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, t: MediaType, _d: BusDirection) -> i32 {
        if t == MediaTypes_::kAudio as MediaType { 1 } else { 0 }
    }
    unsafe fn getBusInfo(&self, t: MediaType, d: BusDirection, i: i32, bus: *mut BusInfo) -> tresult {
        if t != MediaTypes_::kAudio as MediaType || i != 0 {
            return kInvalidArgument;
        }
        let b = unsafe { &mut *bus };
        b.mediaType = t;
        b.direction = d;
        b.channelCount = 2;
        copy_wstring(if d == BusDirections_::kInput as BusDirection { "Input" } else { "Output" }, &mut b.name);
        b.busType = BusTypes_::kMain as BusType;
        b.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _i: *mut RoutingInfo, _o: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(&self, _t: MediaType, _d: BusDirection, _i: i32, _s: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, _s: TBool) -> tresult {
        *self.line.lock().unwrap() = (vec![[0.0; 2]; LATENCY], 0);
        kResultOk
    }
    unsafe fn setState(&self, s: *mut IBStream) -> tresult {
        let Some(s) = (unsafe { ComRef::from_raw(s) }) else { return kInvalidArgument };
        let mut b = [0u8; 16];
        let mut n = 0;
        unsafe { s.read(b.as_mut_ptr() as *mut c_void, 16, &mut n) };
        if n != 16 {
            return kResultFalse;
        }
        self.gain.store(u64::from_le_bytes(b[..8].try_into().unwrap()), Ordering::Relaxed);
        self.polarity.store(u64::from_le_bytes(b[8..].try_into().unwrap()), Ordering::Relaxed);
        kResultOk
    }
    unsafe fn getState(&self, s: *mut IBStream) -> tresult {
        let Some(s) = (unsafe { ComRef::from_raw(s) }) else { return kInvalidArgument };
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&self.gain.load(Ordering::Relaxed).to_le_bytes());
        b[8..].copy_from_slice(&self.polarity.load(Ordering::Relaxed).to_le_bytes());
        let mut n = 0;
        unsafe { s.write(b.as_mut_ptr() as *mut c_void, 16, &mut n) };
        kResultOk
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(&self, i: *mut SpeakerArrangement, ni: i32, o: *mut SpeakerArrangement, no: i32) -> tresult {
        if ni == 1 && no == 1 && unsafe { *i == SpeakerArr::kStereo && *o == SpeakerArr::kStereo } { kResultTrue } else { kResultFalse }
    }
    unsafe fn getBusArrangement(&self, _d: BusDirection, i: i32, a: *mut SpeakerArrangement) -> tresult {
        if i != 0 {
            return kInvalidArgument;
        }
        unsafe { *a = SpeakerArr::kStereo };
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, s: i32) -> tresult {
        if s == SymbolicSampleSizes_::kSample32 as i32 { kResultOk } else { kNotImplemented }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        LATENCY as u32
    }
    unsafe fn setupProcessing(&self, _s: *mut ProcessSetup) -> tresult {
        kResultOk
    }
    unsafe fn setProcessing(&self, _s: TBool) -> tresult {
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let d = unsafe { &*data };
        if let Some(pc) = unsafe { ComRef::from_raw(d.inputParameterChanges) } {
            for i in 0..unsafe { pc.getParameterCount() } {
                if let Some(q) = unsafe { ComRef::from_raw(pc.getParameterData(i)) } {
                    let (mut off, mut v) = (0, 0.0);
                    if unsafe { q.getPoint(q.getPointCount() - 1, &mut off, &mut v) } == kResultOk {
                        match unsafe { q.getParameterId() } {
                            0 => self.gain.store(v.to_bits(), Ordering::Relaxed),
                            1 => self.polarity.store(v.to_bits(), Ordering::Relaxed),
                            _ => {}
                        }
                    }
                }
            }
        }
        let n = d.numSamples as usize;
        if d.numInputs != 1 || d.numOutputs != 1 || n == 0 {
            return kResultOk;
        }
        let (ib, ob) = unsafe { (&*d.inputs, &*d.outputs) };
        let (ic, oc) = unsafe { (std::slice::from_raw_parts(ib.__field0.channelBuffers32, 2), std::slice::from_raw_parts(ob.__field0.channelBuffers32, 2)) };
        let g = gain_of(f64::from_bits(self.gain.load(Ordering::Relaxed))) * if f64::from_bits(self.polarity.load(Ordering::Relaxed)) >= 0.5 { -1.0 } else { 1.0 };
        let mut line = self.line.lock().unwrap();
        let (buf, at) = &mut *line;
        for k in 0..n {
            let x = unsafe { [*ic[0].add(k), *ic[1].add(k)] };
            let y = buf[*at];
            buf[*at] = x;
            *at = (*at + 1) % LATENCY;
            unsafe {
                *oc[0].add(k) = y[0] * g as f32;
                *oc[1].add(k) = y[1] * g as f32;
            }
        }
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

struct Controller {
    values: RefCell<[f64; 3]>,
    handler: RefCell<Option<ComPtr<IComponentHandler>>>,
}
unsafe impl Send for Controller {}
unsafe impl Sync for Controller {}

impl Class for Controller {
    type Interfaces = (IEditController, IConnectionPoint);
}
impl Controller {
    const CID: TUID = uid(0x5B2C3D4E, 0x22334455, 0x66778899, 0xAABBCCDD);
}
impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _c: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}
impl IConnectionPointTrait for Controller {
    unsafe fn connect(&self, _o: *mut IConnectionPoint) -> tresult {
        kResultOk
    }
    unsafe fn disconnect(&self, _o: *mut IConnectionPoint) -> tresult {
        kResultOk
    }
    unsafe fn notify(&self, m: *mut IMessage) -> tresult {
        let Some(m) = (unsafe { ComRef::from_raw(m) }) else { return kInvalidArgument };
        let id = unsafe { std::ffi::CStr::from_ptr(m.getMessageID()) };
        if id.to_bytes() == b"hello" {
            let mut n = 0i64;
            if let Some(a) = unsafe { ComRef::from_raw(m.getAttributes()) } {
                unsafe { a.getInt(c"n".as_ptr(), &mut n) };
            }
            GOT_MESSAGE.store(n as u32, Ordering::Relaxed);
        }
        kResultOk
    }
}
impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, s: *mut IBStream) -> tresult {
        let Some(s) = (unsafe { ComRef::from_raw(s) }) else { return kInvalidArgument };
        let mut b = [0u8; 16];
        let mut n = 0;
        unsafe { s.read(b.as_mut_ptr() as *mut c_void, 16, &mut n) };
        if n == 16 {
            let mut v = self.values.borrow_mut();
            v[0] = f64::from_le_bytes(b[..8].try_into().unwrap());
            v[1] = f64::from_le_bytes(b[8..].try_into().unwrap());
        }
        kResultOk
    }
    unsafe fn setState(&self, _s: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _s: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getParameterCount(&self) -> i32 {
        3
    }
    unsafe fn getParameterInfo(&self, i: i32, info: *mut ParameterInfo) -> tresult {
        let info = unsafe { &mut *info };
        let (title, steps, def, flags) = match i {
            0 => ("Gain", 0, 60.0 / 72.0, ParameterInfo_::ParameterFlags_::kCanAutomate),
            1 => ("Polarity", 1, 0.0, ParameterInfo_::ParameterFlags_::kCanAutomate | ParameterInfo_::ParameterFlags_::kIsList),
            2 => ("Meter", 0, 0.0, ParameterInfo_::ParameterFlags_::kIsReadOnly),
            _ => return kInvalidArgument,
        };
        info.id = i as u32;
        copy_wstring(title, &mut info.title);
        copy_wstring(title, &mut info.shortTitle);
        copy_wstring(if i == 0 { "dB" } else { "" }, &mut info.units);
        info.stepCount = steps;
        info.defaultNormalizedValue = def;
        info.unitId = 0;
        info.flags = flags as i32;
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, id: u32, v: f64, s: *mut String128) -> tresult {
        let t = match id {
            0 => format!("{:.1}", -60.0 + 72.0 * v),
            1 => (if v >= 0.5 { "Inverted" } else { "Normal" }).to_string(),
            _ => format!("{v:.3}"),
        };
        copy_wstring(&t, unsafe { &mut *s });
        kResultOk
    }
    unsafe fn getParamValueByString(&self, _id: u32, _s: *mut TChar, _v: *mut f64) -> tresult {
        kNotImplemented
    }
    unsafe fn normalizedParamToPlain(&self, _id: u32, v: f64) -> f64 {
        v
    }
    unsafe fn plainParamToNormalized(&self, _id: u32, v: f64) -> f64 {
        v
    }
    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        self.values.borrow().get(id as usize).copied().unwrap_or(0.0)
    }
    unsafe fn setParamNormalized(&self, id: u32, v: f64) -> tresult {
        if let Some(x) = self.values.borrow_mut().get_mut(id as usize) {
            *x = v;
        }
        kResultOk
    }
    unsafe fn setComponentHandler(&self, h: *mut IComponentHandler) -> tresult {
        *self.handler.borrow_mut() = unsafe { ComRef::from_raw(h) }.map(|r| r.to_com_ptr());
        kResultOk
    }
    unsafe fn createView(&self, _name: FIDString) -> *mut IPlugView {
        let h = self.handler.borrow().clone();
        ComWrapper::new(View { frame: RefCell::new(None), win: RefCell::new(None), timer: ComWrapper::new(Timer { handler: h }) }).to_com_ptr::<IPlugView>().unwrap().into_raw()
    }
}

/// The editor's timer: counts ticks, and on the first one tells the host Gain changed (as an
/// editor's knob would, through IComponentHandler::performEdit).
struct Timer {
    handler: Option<ComPtr<IComponentHandler>>,
}
unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}
impl Class for Timer {
    type Interfaces = (ITimerHandler,);
}
impl ITimerHandlerTrait for Timer {
    unsafe fn onTimer(&self) {
        if TIMER_TICKS.fetch_add(1, Ordering::Relaxed) == 0 {
            if let Some(h) = &self.handler {
                unsafe {
                    h.beginEdit(0);
                    h.performEdit(0, 0.25);
                    h.endEdit(0);
                }
            }
        }
    }
}

struct View {
    frame: RefCell<Option<ComPtr<IPlugFrame>>>,
    win: RefCell<Option<(x11rb::rust_connection::RustConnection, u32)>>,
    timer: ComWrapper<Timer>,
}
unsafe impl Send for View {}
unsafe impl Sync for View {}
impl Class for View {
    type Interfaces = (IPlugView,);
}
impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, t: FIDString) -> tresult {
        if unsafe { std::ffi::CStr::from_ptr(t) } == c"X11EmbedWindowID" { kResultTrue } else { kResultFalse }
    }
    unsafe fn attached(&self, parent: *mut c_void, _t: FIDString) -> tresult {
        let Ok((c, _)) = x11rb::connect(None) else { return kResultFalse };
        let Ok(id) = c.generate_id() else { return kResultFalse };
        if c.create_window(x11rb::COPY_DEPTH_FROM_PARENT, id, parent as usize as u32, 0, 0, 240, 120, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new().background_pixel(0xcc6633)).is_err() {
            return kResultFalse;
        }
        let _ = c.map_window(id);
        let _ = c.flush();
        *self.win.borrow_mut() = Some((c, id));
        if let Some(f) = self.frame.borrow().as_ref() {
            if let Some(rl) = f.cast::<IRunLoop>() {
                unsafe { rl.registerTimer(self.timer.as_com_ref::<ITimerHandler>().unwrap().as_ptr(), 10) };
            }
        }
        kResultOk
    }
    unsafe fn removed(&self) -> tresult {
        if let Some(f) = self.frame.borrow().as_ref() {
            if let Some(rl) = f.cast::<IRunLoop>() {
                unsafe { rl.unregisterTimer(self.timer.as_com_ref::<ITimerHandler>().unwrap().as_ptr()) };
            }
        }
        if let Some((c, w)) = self.win.borrow_mut().take() {
            let _ = c.destroy_window(w);
            let _ = c.flush();
        }
        kResultOk
    }
    unsafe fn onWheel(&self, _d: f32) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyDown(&self, _k: char16, _c: int16, _m: int16) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyUp(&self, _k: char16, _c: int16, _m: int16) -> tresult {
        kResultFalse
    }
    unsafe fn getSize(&self, r: *mut ViewRect) -> tresult {
        unsafe { *r = ViewRect { left: 0, top: 0, right: 240, bottom: 120 } };
        kResultOk
    }
    unsafe fn onSize(&self, _r: *mut ViewRect) -> tresult {
        kResultOk
    }
    unsafe fn onFocus(&self, _s: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setFrame(&self, f: *mut IPlugFrame) -> tresult {
        *self.frame.borrow_mut() = unsafe { ComRef::from_raw(f) }.map(|r| r.to_com_ptr());
        kResultOk
    }
    unsafe fn canResize(&self) -> tresult {
        kResultFalse
    }
    unsafe fn checkSizeConstraint(&self, _r: *mut ViewRect) -> tresult {
        kResultOk
    }
}

struct Factory;
impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}
impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = unsafe { &mut *info };
        copy_cstring("Bramble Test", &mut info.vendor);
        copy_cstring("", &mut info.url);
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        2
    }
    unsafe fn getClassInfo(&self, i: i32, info: *mut PClassInfo) -> tresult {
        let info = unsafe { &mut *info };
        let (cid, cat) = match i {
            0 => (Processor::CID, "Audio Module Class"),
            1 => (Controller::CID, "Component Controller Class"),
            _ => return kInvalidArgument,
        };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(cat, &mut info.category);
        copy_cstring("Test VST3 Gain", &mut info.name);
        kResultOk
    }
    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, obj: *mut *mut c_void) -> tresult {
        let inst = match unsafe { *(cid as *const TUID) } {
            Processor::CID => ComWrapper::new(Processor { gain: AtomicU64::new((60.0f64 / 72.0).to_bits()), polarity: AtomicU64::new(0f64.to_bits()), line: Mutex::new((vec![[0.0; 2]; LATENCY], 0)), peer: RefCell::new(None), host: Cell::new(std::ptr::null_mut()) }).to_com_ptr::<FUnknown>().unwrap(),
            Controller::CID => ComWrapper::new(Controller { values: RefCell::new([60.0 / 72.0, 0.0, 0.0]), handler: RefCell::new(None) }).to_com_ptr::<FUnknown>().unwrap(),
            _ => return kInvalidArgument,
        };
        let p = inst.as_ptr();
        unsafe { ((*(*p).vtbl).queryInterface)(p, iid as *mut TUID, obj) }
    }
}
impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, i: i32, info: *mut PClassInfo2) -> tresult {
        let mut c: PClassInfo = unsafe { std::mem::zeroed() };
        let r = unsafe { self.getClassInfo(i, &mut c) };
        if r != kResultOk {
            return r;
        }
        let info = unsafe { &mut *info };
        info.cid = c.cid;
        info.cardinality = c.cardinality;
        info.category = c.category;
        info.name = c.name;
        info.classFlags = 0;
        copy_cstring("Fx|Tools", &mut info.subCategories);
        copy_cstring("Bramble Test", &mut info.vendor);
        copy_cstring("1.0.0", &mut info.version);
        copy_cstring("VST 3.7", &mut info.sdkVersion);
        kResultOk
    }
}

/// The test reads these through dlsym.
#[unsafe(no_mangle)]
pub extern "C" fn bramble_test_ticks() -> u32 {
    TIMER_TICKS.load(Ordering::Relaxed)
}
#[unsafe(no_mangle)]
pub extern "C" fn bramble_test_message() -> u32 {
    GOT_MESSAGE.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_h: *mut c_void) -> bool {
    true
}
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}
#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>().unwrap().into_raw()
}
