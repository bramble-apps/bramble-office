//! The window a plug-in's own editor lives in: an X11 top-level window Studio makes (through
//! XWayland on a Wayland desktop) that the editor is embedded in, as Logic's plug-in window
//! holds an Audio Unit's view. The formats' Linux editors run on the host's event loop, so the
//! timers and file descriptors they register are kept here and served from Studio's main
//! thread (`idle`).

use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt as _, EventMask, PropMode, WindowClass};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// An X display can be reached (DISPLAY set: X11, or XWayland on Wayland).
pub fn x11_available() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinEvent {
    /// The window's close button.
    Closed,
    /// The user resized it.
    Resized(u32, u32),
}

/// The host window.
pub struct HostWindow {
    conn: RustConnection,
    win: u32,
    wm_delete: u32,
    pub size: (u32, u32),
}

impl HostWindow {
    pub fn new(title: &str, w: u32, h: u32, resizable: bool) -> Result<HostWindow, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| format!("no X display for the plug-in's editor ({e})"))?;
        let s = conn.setup().roots[screen].clone();
        let win = conn.generate_id().map_err(|e| e.to_string())?;
        let (w, h) = (w.clamp(16, 8192), h.clamp(16, 8192));
        let aux = xproto::CreateWindowAux::new().background_pixel(s.black_pixel).event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::EXPOSURE);
        conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, win, s.root, 0, 0, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux).map_err(|e| e.to_string())?;
        let atom = |n: &str| conn.intern_atom(false, n.as_bytes()).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.atom);
        let (protocols, delete, net_name, utf8) = (atom("WM_PROTOCOLS"), atom("WM_DELETE_WINDOW"), atom("_NET_WM_NAME"), atom("UTF8_STRING"));
        let _ = conn.change_property32(PropMode::REPLACE, win, protocols, xproto::AtomEnum::ATOM, &[delete]);
        let _ = conn.change_property8(PropMode::REPLACE, win, xproto::AtomEnum::WM_NAME, xproto::AtomEnum::STRING, title.as_bytes());
        let _ = conn.change_property8(PropMode::REPLACE, win, net_name, utf8, title.as_bytes());
        let _ = conn.change_property8(PropMode::REPLACE, win, xproto::AtomEnum::WM_CLASS, xproto::AtomEnum::STRING, b"bramble-studio\0Bramble Studio\0");
        if !resizable {
            // WM_NORMAL_HINTS: min = max = the size
            let mut hints = [0u32; 18];
            hints[0] = 16 | 32; // PMinSize | PMaxSize
            hints[5] = w;
            hints[6] = h;
            hints[7] = w;
            hints[8] = h;
            let _ = conn.change_property32(PropMode::REPLACE, win, xproto::AtomEnum::WM_NORMAL_HINTS, xproto::AtomEnum::WM_SIZE_HINTS, &hints);
        }
        conn.flush().map_err(|e| e.to_string())?;
        Ok(HostWindow { conn, win, wm_delete: delete, size: (w, h) })
    }

    pub fn id(&self) -> u32 {
        self.win
    }

    pub fn show(&self) {
        let _ = self.conn.map_window(self.win);
        let _ = self.conn.flush();
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        let (w, h) = (w.clamp(16, 8192), h.clamp(16, 8192));
        self.size = (w, h);
        let _ = self.conn.configure_window(self.win, &xproto::ConfigureWindowAux::new().width(w).height(h));
        let _ = self.conn.flush();
    }

    /// The window's events since the last call.
    pub fn poll(&mut self) -> Vec<WinEvent> {
        let mut out = Vec::new();
        while let Ok(Some(ev)) = self.conn.poll_for_event() {
            match ev {
                Event::ClientMessage(m) if m.data.as_data32()[0] == self.wm_delete => out.push(WinEvent::Closed),
                Event::ConfigureNotify(c) if c.window == self.win && (c.width as u32, c.height as u32) != self.size => {
                    self.size = (c.width as u32, c.height as u32);
                    out.push(WinEvent::Resized(self.size.0, self.size.1));
                }
                _ => {}
            }
        }
        out
    }

    /// The windows directly inside this one (the embedded editor), for checks.
    pub fn children(&self) -> Vec<u32> {
        self.conn.query_tree(self.win).ok().and_then(|c| c.reply().ok()).map(|r| r.children).unwrap_or_default()
    }
}

impl Drop for HostWindow {
    fn drop(&mut self) {
        let _ = self.conn.destroy_window(self.win);
        let _ = self.conn.flush();
    }
}

/// Timers a plug-in registered (CLAP timer-support, VST3 IRunLoop).
#[derive(Default)]
pub struct Timers {
    list: Vec<(u64, Duration, Instant)>,
    next: u64,
}

impl Timers {
    pub fn register(&mut self, ms: u32) -> u64 {
        self.next += 1;
        let period = Duration::from_millis(ms.max(1) as u64);
        self.list.push((self.next, period, Instant::now() + period));
        self.next
    }
    /// Registers under an id the caller chose (VST3's handler pointers).
    pub fn register_as(&mut self, id: u64, ms: u32) {
        let period = Duration::from_millis(ms.max(1) as u64);
        self.list.retain(|t| t.0 != id);
        self.list.push((id, period, Instant::now() + period));
    }
    pub fn unregister(&mut self, id: u64) -> bool {
        let n = self.list.len();
        self.list.retain(|t| t.0 != id);
        self.list.len() != n
    }
    /// The timers due now (each rescheduled).
    pub fn due(&mut self) -> Vec<u64> {
        let now = Instant::now();
        let mut out = Vec::new();
        for t in &mut self.list {
            if now >= t.2 {
                out.push(t.0);
                t.2 = now + t.1;
            }
        }
        out
    }
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// File descriptors a plug-in asked the host to watch (bits: 1 read, 2 write, 4 error).
#[derive(Default)]
pub struct Fds {
    pub list: Vec<(i32, u32, u64)>,
}

impl Fds {
    pub fn register(&mut self, fd: i32, flags: u32, tag: u64) {
        self.list.retain(|x| x.0 != fd || x.2 != tag);
        self.list.push((fd, flags, tag));
    }
    pub fn unregister(&mut self, fd: i32, tag: Option<u64>) -> bool {
        let n = self.list.len();
        self.list.retain(|x| !(x.0 == fd && tag.is_none_or(|t| t == x.2)));
        self.list.len() != n
    }
    /// The descriptors ready now: (fd, what happened, tag).
    pub fn ready(&self) -> Vec<(i32, u32, u64)> {
        if self.list.is_empty() {
            return Vec::new();
        }
        let mut p: Vec<libc::pollfd> = self.list.iter().map(|&(fd, f, _)| libc::pollfd { fd, events: (if f & 1 != 0 { libc::POLLIN } else { 0 }) | (if f & 2 != 0 { libc::POLLOUT } else { 0 }), revents: 0 }).collect();
        let n = unsafe { libc::poll(p.as_mut_ptr(), p.len() as libc::nfds_t, 0) };
        if n <= 0 {
            return Vec::new();
        }
        p.iter().zip(&self.list).filter(|(q, _)| q.revents != 0).map(|(q, x)| {
            let mut f = 0;
            if q.revents & libc::POLLIN != 0 {
                f |= 1;
            }
            if q.revents & libc::POLLOUT != 0 {
                f |= 2;
            }
            if q.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                f |= 4;
            }
            (x.0, f, x.2)
        }).collect()
    }
}
