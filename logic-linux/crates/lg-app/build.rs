//! Lists the command names `App::key_command` handles (every string literal in its body), so
//! the menus enable exactly what works: OUT_DIR/handled.rs = `pub const HANDLED: &[&str]`.
fn main() {
    println!("cargo:rerun-if-changed=src/app.rs");
    println!("cargo:rerun-if-changed=src/edit.rs");
    let src = std::fs::read_to_string("src/app.rs").unwrap_or_default();
    let body = src.find("pub fn key_command").map(|i| {
        let rest = &src[i..];
        let end = rest[10..].find("\n    pub fn ").map_or(rest.len(), |j| j + 10);
        &rest[..end]
    }).unwrap_or("");
    // (edit.rs: Logic's editing commands, all of it)
    println!("cargo:rerun-if-changed=src/liveloops.rs");
    println!("cargo:rerun-if-changed=src/midifunc.rs");
    println!("cargo:rerun-if-changed=src/mixercmd.rs");
    println!("cargo:rerun-if-changed=src/recording.rs");
    println!("cargo:rerun-if-changed=src/takes.rs");
    // (recording.rs, takes.rs: their COMMANDS lists only)
    let list = |f: &str| {
        let s = std::fs::read_to_string(f).unwrap_or_default();
        s.find("pub const COMMANDS").map(|i| s[i..i + s[i..].find("];").unwrap_or(0)].to_string()).unwrap_or_default()
    };
    let edit = std::fs::read_to_string("src/edit.rs").unwrap_or_default() + &std::fs::read_to_string("src/liveloops.rs").unwrap_or_default() + &std::fs::read_to_string("src/midifunc.rs").unwrap_or_default() + &std::fs::read_to_string("src/mixercmd.rs").unwrap_or_default() + &list("src/recording.rs") + &list("src/takes.rs");
    // (any other file's `pub const COMMANDS` list: new areas add theirs without touching this)
    println!("cargo:rerun-if-changed=src");
    let mut more = String::new();
    if let Ok(dir) = std::fs::read_dir("src") {
        let mut files: Vec<_> = dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();
        files.sort();
        for p in files {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if ["recording.rs", "takes.rs", "audioedit.rs"].contains(&name) {
                continue;
            }
            more += &list(p.to_str().unwrap_or(""));
        }
    }
    let body = format!("{body}\n{edit}\n{more}");
    let body = body.as_str();
    let mut names: Vec<String> = Vec::new();
    let mut chars = body.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut s = String::new();
        let mut esc = false;
        for (_, c) in chars.by_ref() {
            if esc {
                s.push(c);
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                break;
            } else {
                s.push(c);
            }
        }
        if !s.is_empty() && !s.contains('{') && !names.contains(&s) {
            names.push(s);
        }
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("handled.rs");
    let list: Vec<String> = names.iter().map(|n| format!("{n:?}")).collect();
    std::fs::write(out, format!("pub const HANDLED: &[&str] = &[{}];\n", list.join(", "))).unwrap();
}
