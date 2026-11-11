//! MIDI outputs: the ports Environment instrument objects, the MIDI click, control surface
//! feedback and synchronization (MIDI Clock, MTC, MMC) send to, opened when first used, plus
//! Studio's own virtual output (Bramble Studio Virtual Out) that other apps can listen to.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The open outputs, shared with the sync thread.
#[derive(Clone, Default)]
pub struct Outs(pub Arc<Mutex<Inner>>);

#[derive(Default)]
pub struct Inner {
    conns: HashMap<String, midir::MidiOutputConnection>,
    /// Every output port found (with the virtual one first on Linux).
    pub names: Vec<String>,
    scanned: bool,
    /// What went out last (for tests and the MIDI activity display): (port, bytes).
    pub sent: Vec<(String, Vec<u8>)>,
}

pub const VIRTUAL: &str = "Bramble Studio Virtual Out";

fn disabled() -> bool {
    std::env::var_os("LG_SNAP").is_some() || std::env::var_os("LG_NO_MIDI").is_some()
}

impl Inner {
    /// The port list (scanned once; `rescan` again).
    pub fn ports(&mut self) -> Vec<String> {
        if !self.scanned {
            self.rescan();
        }
        self.names.clone()
    }

    pub fn rescan(&mut self) {
        self.scanned = true;
        let mut v = Vec::new();
        if cfg!(unix) {
            v.push(VIRTUAL.to_string());
        }
        if !disabled() {
            if let Ok(o) = midir::MidiOutput::new("Bramble Studio") {
                for p in o.ports() {
                    let n = o.port_name(&p).unwrap_or_default();
                    if n.contains("Midi Through") || n.contains("Bramble Studio") {
                        continue;
                    }
                    v.push(n);
                }
            }
        }
        self.names = v;
    }

    fn open(&mut self, name: &str) -> bool {
        if self.conns.contains_key(name) {
            return true;
        }
        if disabled() {
            return false;
        }
        let Ok(o) = midir::MidiOutput::new("Bramble Studio") else { return false };
        #[cfg(unix)]
        if name == VIRTUAL {
            use midir::os::unix::VirtualOutput;
            if let Ok(c) = o.create_virtual(VIRTUAL) {
                self.conns.insert(name.into(), c);
                return true;
            }
            return false;
        }
        let Some(p) = o.ports().into_iter().find(|p| o.port_name(p).ok().as_deref() == Some(name)) else { return false };
        match o.connect(&p, "bramble-studio-out") {
            Ok(c) => {
                self.conns.insert(name.into(), c);
                true
            }
            Err(_) => false,
        }
    }

    /// Sends bytes to a port ("*": every port; "": nowhere).
    pub fn send(&mut self, port: &str, bytes: &[u8]) {
        if port.is_empty() || bytes.is_empty() {
            return;
        }
        self.sent.push((port.to_string(), bytes.to_vec()));
        if self.sent.len() > 256 {
            self.sent.drain(..128);
        }
        let targets: Vec<String> = if port == "*" { self.ports() } else { vec![port.to_string()] };
        for t in targets {
            if self.open(&t) {
                if let Some(c) = self.conns.get_mut(&t) {
                    let _ = c.send(bytes);
                }
            }
        }
    }
}

impl Outs {
    pub fn send(&self, port: &str, bytes: &[u8]) {
        if let Ok(mut i) = self.0.lock() {
            i.send(port, bytes);
        }
    }
    pub fn ports(&self) -> Vec<String> {
        self.0.lock().map(|mut i| i.ports()).unwrap_or_default()
    }
    pub fn rescan(&self) {
        if let Ok(mut i) = self.0.lock() {
            i.rescan();
        }
    }
}
