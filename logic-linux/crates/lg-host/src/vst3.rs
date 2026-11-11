//! VST3 plug-ins (the `vst3` crate's bindings, generated from the SDK's headers). A `.vst3`
//! bundle holds `Contents/x86_64-linux/<name>.so`, which exports `GetPluginFactory`. Studio is
//! the host: IHostApplication (with IMessage / IAttributeList for the component and
//! controller to talk), IComponentHandler (edits from the plug-in's editor), IBStream (state),
//! IParameterChanges / IParamValueQueue and IEventList (the blocks' automation and notes),
//! IPlugFrame with Linux's IRunLoop (the editor's timers and file descriptors).

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use vst3::Steinberg::Linux::*;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::Steinberg::Vst::IAttributeList_::AttrID;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface};

use crate::editor::{Fds, HostWindow, Timers, WinEvent};
use crate::{Backend, Ev, Format, Idle, Kind, ParamInfo, PluginInfo, MAX_BLOCK};

fn wstr(s: &[TChar]) -> String {
    String::from_utf16_lossy(&s.iter().take_while(|c| **c != 0).copied().collect::<Vec<u16>>())
}

fn cchars(b: &[c_char]) -> String {
    let v: Vec<u8> = b.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&v).into_owned()
}

fn hex(cid: &TUID) -> String {
    cid.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

fn unhex(s: &str) -> Option<TUID> {
    if s.len() != 32 {
        return None;
    }
    let mut t: TUID = [0; 16];
    for (i, x) in t.iter_mut().enumerate() {
        *x = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()? as c_char;
    }
    Some(t)
}

/// The shared library inside a bundle (or the file itself, for an old single-file plug-in).
pub fn library_of(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let dir = path.join("Contents").join(format!("{}-linux", std::env::consts::ARCH));
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let p = dir.join(format!("{stem}.so"));
    if p.is_file() {
        return Some(p);
    }
    std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "so"))
}

struct Module {
    _lib: libloading::Library,
    factory: ComPtr<IPluginFactory>,
}
unsafe impl Send for Module {}
unsafe impl Sync for Module {}

fn module(path: &Path) -> Result<&'static Module, String> {
    static M: OnceLock<Mutex<HashMap<PathBuf, &'static Module>>> = OnceLock::new();
    let mut map = M.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = map.get(path) {
        return Ok(m);
    }
    let so = library_of(path).ok_or("no Linux library in the bundle")?;
    unsafe {
        let unix = libloading::os::unix::Library::open(Some(&so), libc::RTLD_NOW | libc::RTLD_LOCAL).map_err(|e| e.to_string())?;
        let handle = unix.into_raw();
        let lib: libloading::Library = libloading::os::unix::Library::from_raw(handle).into();
        if let Ok(entry) = lib.get::<unsafe extern "system" fn(*mut c_void) -> bool>(b"ModuleEntry\0") {
            if !entry(handle) {
                return Err("the plug-in's ModuleEntry failed".into());
            }
        }
        let get = lib.get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0").map_err(|_| "not a VST3 plug-in (no GetPluginFactory)".to_string())?;
        let factory = ComPtr::from_raw(get()).ok_or("no plug-in factory")?;
        let m: &'static Module = Box::leak(Box::new(Module { _lib: lib, factory }));
        map.insert(path.to_path_buf(), m);
        Ok(m)
    }
}

// --- the host's COM objects

/// What the plug-in's calls into the host leave for the main thread.
#[derive(Default)]
struct Shared {
    edits: Mutex<Vec<(u32, f64)>>,
    restart: AtomicI32,
    dirty: AtomicBool,
    timers: Mutex<Timers>,
    timer_handlers: Mutex<HashMap<u64, ComPtr<ITimerHandler>>>,
    fds: Mutex<Fds>,
    fd_handlers: Mutex<HashMap<u64, ComPtr<IEventHandler>>>,
    resize: Mutex<Option<(i32, i32)>>,
}

struct HostApp {
    shared: Arc<Shared>,
}

impl Class for HostApp {
    type Interfaces = (IHostApplication, IComponentHandler);
}

impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let n = unsafe { &mut *name };
        for (k, c) in "Bramble Studio".encode_utf16().chain([0]).enumerate() {
            n[k] = c;
        }
        kResultOk
    }
    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        let want = |i: &vst3::com_scrape_types::Guid| unsafe { (*(cid as *const [u8; 16]) == *i) || (*(iid as *const [u8; 16]) == *i) };
        unsafe {
            if want(&IMessage::IID) {
                let m = ComWrapper::new(Message { id: Mutex::new(None), attrs: ComWrapper::new(Attrs::default()) });
                *obj = m.to_com_ptr::<IMessage>().unwrap().into_raw() as *mut c_void;
                return kResultOk;
            }
            if want(&IAttributeList::IID) {
                *obj = ComWrapper::new(Attrs::default()).to_com_ptr::<IAttributeList>().unwrap().into_raw() as *mut c_void;
                return kResultOk;
            }
            *obj = std::ptr::null_mut();
        }
        kNoInterface
    }
}

impl IComponentHandlerTrait for HostApp {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, id: ParamID, v: ParamValue) -> tresult {
        self.shared.edits.lock().unwrap_or_else(|e| e.into_inner()).push((id, v));
        self.shared.dirty.store(true, Ordering::Release);
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.shared.restart.fetch_or(flags, Ordering::AcqRel);
        kResultOk
    }
}

struct Message {
    id: Mutex<Option<std::ffi::CString>>,
    attrs: ComWrapper<Attrs>,
}
impl Class for Message {
    type Interfaces = (IMessage,);
}
impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(c"".as_ptr(), |c| c.as_ptr())
    }
    unsafe fn setMessageID(&self, id: FIDString) {
        *self.id.lock().unwrap_or_else(|e| e.into_inner()) = (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned());
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attrs.as_com_ref::<IAttributeList>().map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}

#[derive(Clone)]
enum Attr {
    Int(i64),
    Float(f64),
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

#[derive(Default)]
struct Attrs {
    map: Mutex<HashMap<String, Attr>>,
}
impl Class for Attrs {
    type Interfaces = (IAttributeList,);
}
fn attr_key(id: AttrID) -> String {
    if id.is_null() { String::new() } else { unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned() }
}
impl IAttributeListTrait for Attrs {
    unsafe fn setInt(&self, id: AttrID, v: int64) -> tresult {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(attr_key(id), Attr::Int(v));
        kResultOk
    }
    unsafe fn getInt(&self, id: AttrID, v: *mut int64) -> tresult {
        match self.map.lock().unwrap_or_else(|e| e.into_inner()).get(&attr_key(id)) {
            Some(Attr::Int(x)) => {
                unsafe { *v = *x };
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setFloat(&self, id: AttrID, v: f64) -> tresult {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(attr_key(id), Attr::Float(v));
        kResultOk
    }
    unsafe fn getFloat(&self, id: AttrID, v: *mut f64) -> tresult {
        match self.map.lock().unwrap_or_else(|e| e.into_inner()).get(&attr_key(id)) {
            Some(Attr::Float(x)) => {
                unsafe { *v = *x };
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setString(&self, id: AttrID, s: *const TChar) -> tresult {
        let mut v = Vec::new();
        if !s.is_null() {
            let mut k = 0;
            while unsafe { *s.add(k) } != 0 {
                v.push(unsafe { *s.add(k) });
                k += 1;
            }
        }
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(attr_key(id), Attr::Str(v));
        kResultOk
    }
    unsafe fn getString(&self, id: AttrID, s: *mut TChar, size: uint32) -> tresult {
        match self.map.lock().unwrap_or_else(|e| e.into_inner()).get(&attr_key(id)) {
            Some(Attr::Str(x)) => {
                let cap = (size as usize / 2).max(1);
                let n = x.len().min(cap - 1);
                for (k, c) in x.iter().take(n).enumerate() {
                    unsafe { *s.add(k) = *c };
                }
                unsafe { *s.add(n) = 0 };
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size: uint32) -> tresult {
        let v = if data.is_null() { Vec::new() } else { unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) }.to_vec() };
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(attr_key(id), Attr::Bin(v));
        kResultOk
    }
    unsafe fn getBinary(&self, id: AttrID, data: *mut *const c_void, size: *mut uint32) -> tresult {
        // (the bytes stay in the list, as the SDK's own host does)
        match self.map.lock().unwrap_or_else(|e| e.into_inner()).get(&attr_key(id)) {
            Some(Attr::Bin(x)) => {
                unsafe {
                    *data = x.as_ptr() as *const c_void;
                    *size = x.len() as u32;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

/// A byte stream over memory (the plug-in's state).
pub struct Stream {
    data: Mutex<(Vec<u8>, usize)>,
}
impl Class for Stream {
    type Interfaces = (IBStream,);
}
impl Stream {
    fn new(v: Vec<u8>) -> ComWrapper<Stream> {
        ComWrapper::new(Stream { data: Mutex::new((v, 0)) })
    }
    fn bytes(&self) -> Vec<u8> {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).0.clone()
    }
    fn rewind(&self) {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).1 = 0;
    }
}
impl IBStreamTrait for Stream {
    unsafe fn read(&self, buf: *mut c_void, n: int32, read: *mut int32) -> tresult {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let (v, at) = &mut *g;
        let k = (n.max(0) as usize).min(v.len().saturating_sub(*at));
        unsafe { std::ptr::copy_nonoverlapping(v.as_ptr().add(*at), buf as *mut u8, k) };
        *at += k;
        if !read.is_null() {
            unsafe { *read = k as int32 };
        }
        kResultOk
    }
    unsafe fn write(&self, buf: *mut c_void, n: int32, written: *mut int32) -> tresult {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let (v, at) = &mut *g;
        let n = n.max(0) as usize;
        let src = unsafe { std::slice::from_raw_parts(buf as *const u8, n) };
        if *at + n > v.len() {
            v.resize(*at + n, 0);
        }
        v[*at..*at + n].copy_from_slice(src);
        *at += n;
        if !written.is_null() {
            unsafe { *written = n as int32 };
        }
        kResultOk
    }
    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let (v, at) = &mut *g;
        let base = match mode {
            1 => *at as i64,
            2 => v.len() as i64,
            _ => 0,
        };
        *at = (base + pos).clamp(0, v.len() as i64) as usize;
        if !result.is_null() {
            unsafe { *result = *at as i64 };
        }
        kResultOk
    }
    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if !pos.is_null() {
            unsafe { *pos = self.data.lock().unwrap_or_else(|e| e.into_inner()).1 as i64 };
        }
        kResultOk
    }
}

struct Queue {
    id: Mutex<(u32, Vec<(i32, f64)>)>,
}
impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}
impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.lock().unwrap_or_else(|e| e.into_inner()).0
    }
    unsafe fn getPointCount(&self) -> int32 {
        self.id.lock().unwrap_or_else(|e| e.into_inner()).1.len() as int32
    }
    unsafe fn getPoint(&self, i: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        match self.id.lock().unwrap_or_else(|e| e.into_inner()).1.get(i.max(0) as usize) {
            Some(&(o, v)) => {
                unsafe {
                    *offset = o;
                    *value = v;
                }
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        let mut g = self.id.lock().unwrap_or_else(|e| e.into_inner());
        g.1.push((offset, value));
        if !index.is_null() {
            unsafe { *index = g.1.len() as int32 - 1 };
        }
        kResultOk
    }
}

/// A block's parameter changes: a pool of queues made once (no allocation while processing).
struct Changes {
    queues: Vec<ComWrapper<Queue>>,
    used: Mutex<usize>,
}
impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}
impl Changes {
    fn new(n: usize) -> ComWrapper<Changes> {
        ComWrapper::new(Changes { queues: (0..n).map(|_| ComWrapper::new(Queue { id: Mutex::new((0, Vec::with_capacity(16))) })).collect(), used: Mutex::new(0) })
    }
    fn clear(&self) {
        *self.used.lock().unwrap_or_else(|e| e.into_inner()) = 0;
    }
    fn add(&self, id: u32, offset: i32, v: f64) {
        let mut idx = 0;
        let q = unsafe { self.addParameterData(&id, &mut idx) };
        if let Some(q) = unsafe { ComRef::from_raw(q) } {
            unsafe { q.addPoint(offset, v, &mut idx) };
        }
    }
    /// The last value of each changed parameter.
    fn last_values(&self) -> Vec<(u32, f64)> {
        let n = *self.used.lock().unwrap_or_else(|e| e.into_inner());
        self.queues[..n].iter().filter_map(|q| {
            let g = q.id.lock().unwrap_or_else(|e| e.into_inner());
            g.1.last().map(|p| (g.0, p.1))
        }).collect()
    }
}
impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        *self.used.lock().unwrap_or_else(|e| e.into_inner()) as int32
    }
    unsafe fn getParameterData(&self, i: int32) -> *mut IParamValueQueue {
        let n = *self.used.lock().unwrap_or_else(|e| e.into_inner());
        if i < 0 || i as usize >= n {
            return std::ptr::null_mut();
        }
        self.queues[i as usize].as_com_ref::<IParamValueQueue>().map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
    unsafe fn addParameterData(&self, id: *const ParamID, index: *mut int32) -> *mut IParamValueQueue {
        let id = unsafe { *id };
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        let found = self.queues[..*used].iter().position(|q| q.id.lock().unwrap_or_else(|e| e.into_inner()).0 == id);
        let k = match found {
            Some(k) => k,
            None if *used < self.queues.len() => {
                let mut g = self.queues[*used].id.lock().unwrap_or_else(|e| e.into_inner());
                g.0 = id;
                g.1.clear();
                *used += 1;
                *used - 1
            }
            None => return std::ptr::null_mut(),
        };
        if !index.is_null() {
            unsafe { *index = k as int32 };
        }
        self.queues[k].as_com_ref::<IParamValueQueue>().map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}

struct Events {
    list: Mutex<Vec<Event>>,
}
impl Class for Events {
    type Interfaces = (IEventList,);
}
impl IEventListTrait for Events {
    unsafe fn getEventCount(&self) -> int32 {
        self.list.lock().unwrap_or_else(|e| e.into_inner()).len() as int32
    }
    unsafe fn getEvent(&self, i: int32, e: *mut Event) -> tresult {
        match self.list.lock().unwrap_or_else(|e| e.into_inner()).get(i.max(0) as usize) {
            Some(x) => {
                unsafe { *e = *x };
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        let mut l = self.list.lock().unwrap_or_else(|e| e.into_inner());
        if l.len() < l.capacity() {
            l.push(unsafe { *e });
        }
        kResultOk
    }
}

/// The editor's frame and the Linux run loop it asks the frame for.
struct Frame {
    shared: Arc<Shared>,
}
impl Class for Frame {
    type Interfaces = (IPlugFrame, IRunLoop);
}
impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, _view: *mut IPlugView, r: *mut ViewRect) -> tresult {
        if r.is_null() {
            return kInvalidArgument;
        }
        let r = unsafe { &*r };
        *self.shared.resize.lock().unwrap_or_else(|e| e.into_inner()) = Some((r.right - r.left, r.bottom - r.top));
        kResultOk
    }
}
impl IRunLoopTrait for Frame {
    unsafe fn registerEventHandler(&self, h: *mut IEventHandler, fd: FileDescriptor) -> tresult {
        let Some(p) = (unsafe { ComRef::from_raw(h) }).map(|r| r.to_com_ptr()) else { return kInvalidArgument };
        let tag = h as u64;
        self.shared.fds.lock().unwrap_or_else(|e| e.into_inner()).register(fd, 1, tag);
        self.shared.fd_handlers.lock().unwrap_or_else(|e| e.into_inner()).insert(tag, p);
        kResultOk
    }
    unsafe fn unregisterEventHandler(&self, h: *mut IEventHandler) -> tresult {
        let tag = h as u64;
        self.shared.fds.lock().unwrap_or_else(|e| e.into_inner()).list.retain(|x| x.2 != tag);
        self.shared.fd_handlers.lock().unwrap_or_else(|e| e.into_inner()).remove(&tag);
        kResultOk
    }
    unsafe fn registerTimer(&self, h: *mut ITimerHandler, ms: TimerInterval) -> tresult {
        let Some(p) = (unsafe { ComRef::from_raw(h) }).map(|r| r.to_com_ptr()) else { return kInvalidArgument };
        let tag = h as u64;
        self.shared.timers.lock().unwrap_or_else(|e| e.into_inner()).register_as(tag, ms.min(u32::MAX as u64) as u32);
        self.shared.timer_handlers.lock().unwrap_or_else(|e| e.into_inner()).insert(tag, p);
        kResultOk
    }
    unsafe fn unregisterTimer(&self, h: *mut ITimerHandler) -> tresult {
        let tag = h as u64;
        self.shared.timers.lock().unwrap_or_else(|e| e.into_inner()).unregister(tag);
        self.shared.timer_handlers.lock().unwrap_or_else(|e| e.into_inner()).remove(&tag);
        kResultOk
    }
}

// --- the plug-in

pub struct Vst3Plugin {
    shared: Arc<Shared>,
    host: ComWrapper<HostApp>,
    frame: ComWrapper<Frame>,
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    separate: bool,
    points: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    midi_map: Option<ComPtr<IMidiMapping>>,
    infos: Vec<ParamInfo>,
    index: HashMap<u32, usize>,
    units: Vec<String>,
    norm: Vec<f64>,
    pending: Vec<(u32, f64)>,
    in_changes: ComWrapper<Changes>,
    out_changes: ComWrapper<Changes>,
    in_events: ComWrapper<Events>,
    out_events: ComWrapper<Events>,
    in_counts: Vec<i32>,
    out_counts: Vec<i32>,
    scratch: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
    bufs: Vec<AudioBusBuffers>,
    has_events: bool,
    active: bool,
    processing: bool,
    latency: u32,
    rate: f64,
    view: Option<ComPtr<IPlugView>>,
    window: Option<HostWindow>,
    time: i64,
}

unsafe impl Send for Vst3Plugin {}

fn speakers(n: i32) -> SpeakerArrangement {
    match n {
        1 => SpeakerArr::kMono,
        2 => SpeakerArr::kStereo,
        n => (1u64 << n.clamp(0, 63)) - 1,
    }
}

fn channels(a: SpeakerArrangement) -> i32 {
    a.count_ones() as i32
}

impl Vst3Plugin {
    pub fn open(path: &Path, id: &str, rate: f64) -> Result<Vst3Plugin, String> {
        let m = module(path)?;
        let cid = unhex(id).ok_or("bad class id")?;
        let shared = Arc::new(Shared::default());
        let host = ComWrapper::new(HostApp { shared: shared.clone() });
        let frame = ComWrapper::new(Frame { shared: shared.clone() });
        unsafe {
            let host_unknown = host.as_com_ref::<FUnknown>().unwrap().as_ptr();
            if let Some(f3) = m.factory.cast::<IPluginFactory3>() {
                f3.setHostContext(host_unknown);
            }
            let make = |cid: &TUID, iid: &vst3::com_scrape_types::Guid| -> *mut c_void {
                let mut obj: *mut c_void = std::ptr::null_mut();
                let r = m.factory.createInstance(cid.as_ptr(), iid.as_ptr() as FIDString, &mut obj);
                if r == kResultOk { obj } else { std::ptr::null_mut() }
            };
            let component = ComPtr::<IComponent>::from_raw(make(&cid, &IComponent::IID) as *mut IComponent).ok_or("the factory made no component")?;
            if component.initialize(host_unknown) != kResultOk {
                return Err("the plug-in's initialize failed".into());
            }
            let processor = component.cast::<IAudioProcessor>().ok_or("no audio processor")?;
            let mut separate = false;
            let controller = match component.cast::<IEditController>() {
                Some(c) => Some(c),
                None => {
                    let mut ccid: TUID = [0; 16];
                    if component.getControllerClassId(&mut ccid) == kResultOk && ccid != [0; 16] {
                        let c = ComPtr::<IEditController>::from_raw(make(&ccid, &IEditController::IID) as *mut IEditController);
                        if let Some(c) = &c {
                            if c.initialize(host_unknown) != kResultOk {
                                return Err("the plug-in's controller would not initialize".into());
                            }
                            separate = true;
                        }
                        c
                    } else {
                        None
                    }
                }
            };
            let mut points = None;
            if let (true, Some(c)) = (separate, &controller) {
                if let (Some(a), Some(b)) = (component.cast::<IConnectionPoint>(), c.cast::<IConnectionPoint>()) {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                    points = Some((a, b));
                }
            }
            if let Some(c) = &controller {
                c.setComponentHandler(host.as_com_ref::<IComponentHandler>().unwrap().as_ptr());
                // the controller learns the component's state
                let s = Stream::new(Vec::new());
                let sp = s.as_com_ref::<IBStream>().unwrap().as_ptr();
                if component.getState(sp) == kResultOk {
                    s.rewind();
                    c.setComponentState(sp);
                }
            }
            let midi_map = controller.as_ref().and_then(|c| c.cast::<IMidiMapping>());
            let mut me = Vst3Plugin {
                shared,
                host,
                frame,
                component,
                processor,
                controller,
                separate,
                points,
                midi_map,
                infos: Vec::new(),
                index: HashMap::new(),
                units: Vec::new(),
                norm: Vec::new(),
                pending: Vec::with_capacity(1024),
                in_changes: Changes::new(512),
                out_changes: Changes::new(512),
                in_events: ComWrapper::new(Events { list: Mutex::new(Vec::with_capacity(1024)) }),
                out_events: ComWrapper::new(Events { list: Mutex::new(Vec::with_capacity(1024)) }),
                in_counts: Vec::new(),
                out_counts: Vec::new(),
                scratch: Vec::new(),
                ptrs: Vec::new(),
                bufs: Vec::new(),
                has_events: false,
                active: false,
                processing: false,
                latency: 0,
                rate,
                view: None,
                window: None,
                time: 0,
            };
            me.setup_buses();
            me.read_params();
            me.activate()?;
            Ok(me)
        }
    }

    fn setup_buses(&mut self) {
        unsafe {
            let c = &self.component;
            let n_in = c.getBusCount(MediaTypes_::kAudio as MediaType, BusDirections_::kInput as BusDirection);
            let n_out = c.getBusCount(MediaTypes_::kAudio as MediaType, BusDirections_::kOutput as BusDirection);
            let read = |dir: BusDirections, n: i32| -> Vec<i32> {
                (0..n).map(|i| {
                    let mut b: BusInfo = std::mem::zeroed();
                    if c.getBusInfo(MediaTypes_::kAudio as MediaType, dir as BusDirection, i, &mut b) == kResultOk { b.channelCount } else { 2 }
                }).collect()
            };
            let ins = read(BusDirections_::kInput, n_in);
            let outs = read(BusDirections_::kOutput, n_out);
            // the main buses as stereo if the plug-in takes it, else its own arrangement
            let mut ai: Vec<SpeakerArrangement> = ins.iter().map(|c| speakers(*c)).collect();
            let mut ao: Vec<SpeakerArrangement> = outs.iter().map(|c| speakers(*c)).collect();
            if let Some(a) = ai.first_mut() {
                *a = SpeakerArr::kStereo;
            }
            if let Some(a) = ao.first_mut() {
                *a = SpeakerArr::kStereo;
            }
            if self.processor.setBusArrangements(ai.as_mut_ptr(), ai.len() as i32, ao.as_mut_ptr(), ao.len() as i32) != kResultTrue {
                for (k, a) in ai.iter_mut().enumerate() {
                    self.processor.getBusArrangement(BusDirections_::kInput as BusDirection, k as i32, a);
                }
                for (k, a) in ao.iter_mut().enumerate() {
                    self.processor.getBusArrangement(BusDirections_::kOutput as BusDirection, k as i32, a);
                }
                self.processor.setBusArrangements(ai.as_mut_ptr(), ai.len() as i32, ao.as_mut_ptr(), ao.len() as i32);
            }
            self.in_counts = ai.iter().map(|a| channels(*a)).collect();
            self.out_counts = ao.iter().map(|a| channels(*a)).collect();
            for (dir, n) in [(BusDirections_::kInput, n_in), (BusDirections_::kOutput, n_out)] {
                for i in 0..n {
                    c.activateBus(MediaTypes_::kAudio as MediaType, dir as BusDirection, i, (i == 0) as TBool);
                }
            }
            let n_ev = c.getBusCount(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection);
            if n_ev > 0 {
                c.activateBus(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection, 0, 1);
                self.has_events = true;
            }
            let total: usize = self.in_counts.iter().chain(self.out_counts.iter()).map(|c| *c as usize).sum();
            self.scratch = (0..total).map(|_| vec![0.0; MAX_BLOCK]).collect();
            self.ptrs = vec![std::ptr::null_mut(); total];
        }
    }

    fn activate(&mut self) -> Result<(), String> {
        unsafe {
            let mut setup = ProcessSetup { processMode: ProcessModes_::kRealtime as i32, symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32, maxSamplesPerBlock: MAX_BLOCK as i32, sampleRate: self.rate };
            if self.processor.setupProcessing(&mut setup) != kResultOk {
                return Err("the plug-in refused the sample rate or block size".into());
            }
            if self.component.setActive(1) != kResultOk {
                return Err("the plug-in would not activate".into());
            }
            self.latency = self.processor.getLatencySamples();
        }
        self.active = true;
        self.processing = false;
        Ok(())
    }

    fn deactivate(&mut self) {
        unsafe {
            if self.processing {
                self.processor.setProcessing(0);
                self.processing = false;
            }
            if self.active {
                self.component.setActive(0);
                self.active = false;
            }
        }
    }

    fn read_params(&mut self) {
        self.infos.clear();
        self.index.clear();
        self.norm.clear();
        self.units.clear();
        let Some(c) = &self.controller else { return };
        unsafe {
            for i in 0..c.getParameterCount() {
                let mut p: ParameterInfo = std::mem::zeroed();
                if c.getParameterInfo(i, &mut p) != kResultOk {
                    continue;
                }
                let steps = p.stepCount.max(0) as u32;
                let mut choices = Vec::new();
                if steps > 0 && steps <= 64 {
                    for k in 0..=steps {
                        let mut s: String128 = [0; 128];
                        if c.getParamStringByValue(p.id, k as f64 / steps as f64, &mut s) == kResultOk {
                            choices.push(wstr(&s));
                        } else {
                            choices.clear();
                            break;
                        }
                    }
                }
                let hidden = p.flags & (ParameterInfo_::ParameterFlags_::kIsHidden | ParameterInfo_::ParameterFlags_::kIsReadOnly) as i32 != 0;
                self.index.insert(p.id, self.infos.len());
                self.norm.push(c.getParamNormalized(p.id));
                self.units.push(wstr(&p.units));
                let max = if steps > 0 { steps as f64 } else { 1.0 };
                self.infos.push(ParamInfo { native: p.id, name: wstr(&p.title), min: 0.0, max, default: p.defaultNormalizedValue * max, steps, hidden, choices });
            }
        }
    }

    fn steps_of(&self, i: usize) -> u32 {
        self.infos[i].steps
    }

    fn midi_param(&self, cc: i16) -> Option<u32> {
        let m = self.midi_map.as_ref()?;
        let mut id: ParamID = 0;
        (unsafe { m.getMidiControllerAssignment(0, 0, cc, &mut id) } == kResultTrue).then_some(id)
    }

    /// The editor ("editor" view) embedded in a Studio window.
    fn view_open(&mut self, title: &str) -> Result<(), String> {
        let c = self.controller.as_ref().ok_or("the plug-in has no controller")?;
        unsafe {
            let v = ComPtr::<IPlugView>::from_raw(c.createView(ViewType::kEditor)).ok_or("the plug-in has no editor")?;
            if !crate::editor::x11_available() {
                return Err("the plug-in's editor needs X11 (XWayland): DISPLAY isn't set".into());
            }
            if v.isPlatformTypeSupported(kPlatformTypeX11EmbedWindowID) != kResultTrue {
                return Err("the plug-in's editor doesn't support X11 embedding".into());
            }
            v.setFrame(self.frame.as_com_ref::<IPlugFrame>().unwrap().as_ptr());
            let mut r = ViewRect { left: 0, top: 0, right: 400, bottom: 300 };
            v.getSize(&mut r);
            let win = HostWindow::new(title, (r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32, v.canResize() == kResultTrue)?;
            if v.attached(win.id() as usize as *mut c_void, kPlatformTypeX11EmbedWindowID) != kResultOk {
                v.setFrame(std::ptr::null_mut());
                return Err("the plug-in's editor would not go in the window".into());
            }
            win.show();
            self.view = Some(v);
            self.window = Some(win);
        }
        Ok(())
    }

    fn view_close(&mut self) {
        if let Some(v) = self.view.take() {
            unsafe {
                v.removed();
                v.setFrame(std::ptr::null_mut());
            }
        }
        self.window = None;
    }

    pub fn editor_window(&self) -> Option<&HostWindow> {
        self.window.as_ref()
    }
}

impl Backend for Vst3Plugin {
    fn param_infos(&mut self) -> Vec<ParamInfo> {
        self.infos.clone()
    }
    fn get(&self, native: u32) -> f64 {
        self.index.get(&native).map_or(0.0, |&i| {
            let s = self.steps_of(i);
            if s > 0 { (self.norm[i] * s as f64).round() } else { self.norm[i] }
        })
    }
    fn set(&mut self, native: u32, v: f64) {
        let Some(&i) = self.index.get(&native) else { return };
        let s = self.steps_of(i);
        let n = if s > 0 { (v / s as f64).clamp(0.0, 1.0) } else { v.clamp(0.0, 1.0) };
        self.norm[i] = n;
        if let Some(c) = &self.controller {
            unsafe { c.setParamNormalized(native, n) };
        }
        self.pending.retain(|x| x.0 != native);
        self.pending.push((native, n));
    }
    fn text(&self, native: u32, v: f64) -> Option<String> {
        let c = self.controller.as_ref()?;
        let &i = self.index.get(&native)?;
        let s = self.steps_of(i);
        let n = if s > 0 { v / s as f64 } else { v };
        let mut out: String128 = [0; 128];
        if unsafe { c.getParamStringByValue(native, n, &mut out) } != kResultOk {
            return None;
        }
        let t = wstr(&out);
        let u = &self.units[i];
        Some(if u.is_empty() { t } else { format!("{t} {u}") })
    }
    fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2], events: &[Ev], tempo: f64) {
        let n = out[0].len().min(out[1].len()).min(MAX_BLOCK);
        if !self.active || n == 0 {
            return;
        }
        unsafe {
            if !self.processing {
                self.processor.setProcessing(1);
                self.processing = true;
            }
        }
        self.in_changes.clear();
        self.out_changes.clear();
        for (id, v) in self.pending.drain(..) {
            self.in_changes.add(id, 0, v);
        }
        {
            let mut evs = self.in_events.list.lock().unwrap_or_else(|e| e.into_inner());
            evs.clear();
            for e in events {
                let mut ev: Event = unsafe { std::mem::zeroed() };
                match *e {
                    Ev::NoteOn { key, velocity } => {
                        ev.r#type = Event_::EventTypes_::kNoteOnEvent as u16;
                        ev.__field0.noteOn = NoteOnEvent { channel: 0, pitch: key as i16, tuning: 0.0, velocity: velocity as f32 / 127.0, length: 0, noteId: -1 };
                    }
                    Ev::NoteOff { key } => {
                        ev.r#type = Event_::EventTypes_::kNoteOffEvent as u16;
                        ev.__field0.noteOff = NoteOffEvent { channel: 0, pitch: key as i16, velocity: 0.0, noteId: -1, tuning: 0.0 };
                    }
                    Ev::Cc { cc, value } => {
                        if let Some(id) = self.midi_param(cc as i16) {
                            self.in_changes.add(id, 0, value as f64 / 127.0);
                        }
                        continue;
                    }
                    Ev::Bend(b) => {
                        if let Some(id) = self.midi_param(ControllerNumbers_::kPitchBend as i16) {
                            self.in_changes.add(id, 0, (b as f64 + 8192.0) / 16383.0);
                        }
                        continue;
                    }
                }
                if evs.len() < evs.capacity() {
                    evs.push(ev);
                }
            }
        }
        self.out_events.list.lock().unwrap_or_else(|e| e.into_inner()).clear();
        // the buses: the main ones carry Studio's stereo, others scratch
        let [ol, or] = out;
        self.bufs.clear();
        let mut k = 0;
        let n_in = self.in_counts.len();
        let counts: Vec<i32> = self.in_counts.iter().chain(self.out_counts.iter()).copied().collect();
        for (bi, c) in counts.into_iter().enumerate() {
            let input = bi < n_in;
            let main = bi == 0 || bi == n_in;
            let first = k;
            for ch in 0..c.max(0) as usize {
                let buf = &mut self.scratch[k];
                self.ptrs[k] = match (input, main, ch) {
                    (true, true, 0 | 1) => {
                        buf[..n].copy_from_slice(&inp[ch][..n]);
                        buf.as_mut_ptr()
                    }
                    (false, true, 0) => ol.as_mut_ptr(),
                    (false, true, 1) => or.as_mut_ptr(),
                    _ => {
                        buf[..n].fill(0.0);
                        buf.as_mut_ptr()
                    }
                };
                k += 1;
            }
            let mut b: AudioBusBuffers = unsafe { std::mem::zeroed() };
            b.numChannels = c;
            b.__field0.channelBuffers32 = unsafe { self.ptrs.as_mut_ptr().add(first) };
            self.bufs.push(b);
        }
        let mut ctx: ProcessContext = unsafe { std::mem::zeroed() };
        ctx.state = (ProcessContext_::StatesAndFlags_::kTempoValid | ProcessContext_::StatesAndFlags_::kTimeSigValid) as u32;
        ctx.sampleRate = self.rate;
        ctx.tempo = tempo;
        ctx.timeSigNumerator = 4;
        ctx.timeSigDenominator = 4;
        ctx.continousTimeSamples = self.time;
        let (ins, outs) = self.bufs.split_at_mut(n_in);
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: n as i32,
            numInputs: ins.len() as i32,
            numOutputs: outs.len() as i32,
            inputs: if ins.is_empty() { std::ptr::null_mut() } else { ins.as_mut_ptr() },
            outputs: if outs.is_empty() { std::ptr::null_mut() } else { outs.as_mut_ptr() },
            inputParameterChanges: self.in_changes.as_com_ref::<IParameterChanges>().unwrap().as_ptr(),
            outputParameterChanges: self.out_changes.as_com_ref::<IParameterChanges>().unwrap().as_ptr(),
            inputEvents: if self.has_events { self.in_events.as_com_ref::<IEventList>().unwrap().as_ptr() } else { std::ptr::null_mut() },
            outputEvents: self.out_events.as_com_ref::<IEventList>().unwrap().as_ptr(),
            processContext: &mut ctx,
        };
        unsafe { self.processor.process(&mut data) };
        if self.out_counts.first() == Some(&1) {
            or[..n].copy_from_slice(&ol[..n]);
        }
        if self.out_counts.is_empty() {
            ol[..n].fill(0.0);
            or[..n].fill(0.0);
        }
        for (id, v) in self.out_changes.last_values() {
            if let Some(&i) = self.index.get(&id) {
                self.norm[i] = v;
                self.shared.dirty.store(true, Ordering::Release);
            }
        }
        self.time += n as i64;
    }
    fn latency(&self) -> u32 {
        self.latency
    }
    fn tail(&self) -> f32 {
        let t = unsafe { self.processor.getTailSamples() };
        if t == u32::MAX { 2.0 } else { (t as f64 / self.rate).min(10.0) as f32 }
    }
    fn reset(&mut self) {
        // (VST3 has no reset: processing off and on clears the plug-in's delay lines)
        if self.processing {
            unsafe {
                self.processor.setProcessing(0);
                self.processor.setProcessing(1);
            }
        }
    }
    fn save_state(&mut self) -> Option<Vec<u8>> {
        unsafe {
            let a = Stream::new(Vec::new());
            if self.component.getState(a.as_com_ref::<IBStream>().unwrap().as_ptr()) != kResultOk {
                return None;
            }
            let b = Stream::new(Vec::new());
            if let Some(c) = &self.controller {
                c.getState(b.as_com_ref::<IBStream>().unwrap().as_ptr());
            }
            let (a, b) = (a.bytes(), b.bytes());
            let mut out = b"BVS3".to_vec();
            out.extend((a.len() as u32).to_le_bytes());
            out.extend(a);
            out.extend(b);
            Some(out)
        }
    }
    fn load_state(&mut self, data: &[u8]) -> bool {
        if data.len() < 8 || &data[..4] != b"BVS3" {
            return false;
        }
        let n = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        if data.len() < 8 + n {
            return false;
        }
        unsafe {
            let a = Stream::new(data[8..8 + n].to_vec());
            let ap = a.as_com_ref::<IBStream>().unwrap().as_ptr();
            if self.component.setState(ap) != kResultOk {
                return false;
            }
            if let Some(c) = &self.controller {
                a.rewind();
                c.setComponentState(ap);
                if data.len() > 8 + n {
                    let b = Stream::new(data[8 + n..].to_vec());
                    c.setState(b.as_com_ref::<IBStream>().unwrap().as_ptr());
                }
                for (i, p) in self.infos.iter().enumerate() {
                    self.norm[i] = c.getParamNormalized(p.native);
                }
            }
        }
        self.pending.clear();
        true
    }
    fn idle(&mut self) -> Idle {
        let mut r = Idle::default();
        // the editor's edits: into the cache and on to the processor
        let edits = std::mem::take(&mut *self.shared.edits.lock().unwrap_or_else(|e| e.into_inner()));
        for (id, v) in edits {
            if let Some(&i) = self.index.get(&id) {
                self.norm[i] = v;
                self.pending.retain(|x| x.0 != id);
                self.pending.push((id, v));
            }
        }
        let flags = self.shared.restart.swap(0, Ordering::AcqRel);
        if flags != 0 {
            use RestartFlags_::*;
            if flags & (kLatencyChanged | kReloadComponent | kIoChanged) as i32 != 0 && self.active {
                self.deactivate();
                let _ = self.activate();
            }
            if flags & (kParamTitlesChanged as i32) != 0 {
                self.read_params();
                r.params_rescanned = true;
            } else if flags & (kParamValuesChanged as i32) != 0 {
                if let Some(c) = &self.controller {
                    for (i, p) in self.infos.iter().enumerate() {
                        self.norm[i] = unsafe { c.getParamNormalized(p.native) };
                    }
                }
                r.changed = true;
            }
        }
        if self.shared.dirty.swap(false, Ordering::AcqRel) {
            r.changed = true;
        }
        let due = self.shared.timers.lock().unwrap_or_else(|e| e.into_inner()).due();
        for t in due {
            let h = self.shared.timer_handlers.lock().unwrap_or_else(|e| e.into_inner()).get(&t).cloned();
            if let Some(h) = h {
                unsafe { h.onTimer() };
            }
        }
        let ready = self.shared.fds.lock().unwrap_or_else(|e| e.into_inner()).ready();
        for (fd, _, tag) in ready {
            let h = self.shared.fd_handlers.lock().unwrap_or_else(|e| e.into_inner()).get(&tag).cloned();
            if let Some(h) = h {
                unsafe { h.onFDIsSet(fd) };
            }
        }
        if let Some((w, h)) = self.shared.resize.lock().unwrap_or_else(|e| e.into_inner()).take() {
            if let (Some(win), Some(v)) = (self.window.as_mut(), &self.view) {
                win.resize(w.max(1) as u32, h.max(1) as u32);
                let mut rect = ViewRect { left: 0, top: 0, right: w, bottom: h };
                unsafe { v.onSize(&mut rect) };
            }
        }
        let evs = self.window.as_mut().map(|w| w.poll()).unwrap_or_default();
        for e in evs {
            match e {
                WinEvent::Closed => {
                    self.view_close();
                    r.editor_closed = true;
                }
                WinEvent::Resized(w, h) => {
                    if let Some(v) = &self.view {
                        let mut rect = ViewRect { left: 0, top: 0, right: w as i32, bottom: h as i32 };
                        unsafe {
                            if v.canResize() == kResultTrue {
                                v.checkSizeConstraint(&mut rect);
                                v.onSize(&mut rect);
                            }
                        }
                    }
                }
            }
        }
        r
    }
    fn has_editor(&self) -> bool {
        self.controller.is_some()
    }
    fn open_editor(&mut self, title: &str) -> Result<(), String> {
        if self.view.is_some() {
            return Ok(());
        }
        self.view_open(title)
    }
    fn close_editor(&mut self) {
        self.view_close();
    }
    fn editor_open(&self) -> bool {
        self.view.is_some()
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        self.view_close();
        self.deactivate();
        unsafe {
            if let Some((a, b)) = self.points.take() {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
            if let Some(c) = self.controller.take() {
                c.setComponentHandler(std::ptr::null_mut());
                if self.separate {
                    c.terminate();
                }
            }
            self.component.terminate();
        }
        self.shared.timer_handlers.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.shared.fd_handlers.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let _ = (&self.host, &self.frame);
    }
}

/// Every audio plug-in class in a bundle, each made once to read its buses, parameters,
/// latency and editor (the scan, in its own process).
pub fn describe(path: &Path) -> Result<Vec<PluginInfo>, String> {
    let m = module(path)?;
    let mut out = Vec::new();
    unsafe {
        let f = &m.factory;
        let f2 = f.cast::<IPluginFactory2>();
        let mut fi: PFactoryInfo = std::mem::zeroed();
        let fvendor = if f.getFactoryInfo(&mut fi) == kResultOk { cchars(&fi.vendor) } else { String::new() };
        for i in 0..f.countClasses() {
            let mut ci: PClassInfo = std::mem::zeroed();
            if f.getClassInfo(i, &mut ci) != kResultOk || cchars(&ci.category) != "Audio Module Class" {
                continue;
            }
            let (mut vendor, mut version, mut sub) = (fvendor.clone(), String::new(), String::new());
            if let Some(f2) = &f2 {
                let mut c2: PClassInfo2 = std::mem::zeroed();
                if f2.getClassInfo2(i, &mut c2) == kResultOk {
                    if !cchars(&c2.vendor).is_empty() {
                        vendor = cchars(&c2.vendor);
                    }
                    version = cchars(&c2.version);
                    sub = cchars(&c2.subCategories);
                }
            }
            let id = hex(&ci.cid);
            let kind = if sub.split('|').any(|s| s == "Instrument") { Kind::Instrument } else { Kind::Effect };
            let p = Vst3Plugin::open(path, &id, 48000.0)?;
            let has_editor = p.controller.as_ref().is_some_and(|c| {
                let v = c.createView(ViewType::kEditor);
                drop(ComPtr::<IPlugView>::from_raw(v));
                !v.is_null()
            });
            out.push(PluginInfo { format: Format::Vst3, id, name: cchars(&ci.name), vendor, version, kind, category: sub, path: path.to_path_buf(), inputs: p.in_counts.first().copied().unwrap_or(0).max(0) as u32, outputs: p.out_counts.first().copied().unwrap_or(0).max(0) as u32, params: p.infos.iter().filter(|x| !x.hidden).count() as u32, latency: p.latency, has_editor });
        }
    }
    Ok(out.into_iter().filter(|i| i.outputs > 0).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn class_ids_round_trip() {
        let t: super::TUID = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, -1];
        assert_eq!(super::unhex(&super::hex(&t)), Some(t));
    }
}
