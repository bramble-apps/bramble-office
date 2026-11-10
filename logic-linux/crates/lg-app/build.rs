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
    let edit = std::fs::read_to_string("src/edit.rs").unwrap_or_default();
    let body = format!("{body}\n{edit}");
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
