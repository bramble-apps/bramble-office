//! Insert > Equation: LaTeX typeset with the system's TeX (pdflatex + pdftoppm), inserted as a
//! picture that remembers its LaTeX source (equation_source_text, as Keynote stores it), so it
//! can be edited again here or in Keynote.

use egui::RichText;

use kn_ui::theme;

use crate::app::App;

#[derive(Default)]
pub struct EquationDialog {
    pub open: bool,
    pub source: String,
    pub error: Option<String>,
    /// Editing the equation picture `id` (its source shown, the picture replaced).
    pub editing: Option<nm_core::kn_format::ObjId>,
}

/// PNG of a LaTeX formula (math mode), or a readable error.
fn typeset(latex: &str) -> Result<Vec<u8>, String> {
    let dir = std::env::temp_dir().join(format!("nm-eq-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = format!("\\documentclass[border=2pt]{{standalone}}\n\\usepackage{{amsmath,amssymb}}\n\\begin{{document}}\n$\\displaystyle {latex}$\n\\end{{document}}\n");
    std::fs::write(dir.join("eq.tex"), doc).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("pdflatex")
        .args(["-interaction=nonstopmode", "-halt-on-error", "eq.tex"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| "Equations need TeX: install it with  sudo apt install texlive-latex-extra".to_string())?;
    if !out.status.success() {
        let log = String::from_utf8_lossy(&out.stdout);
        let msg = log.lines().find(|l| l.starts_with('!')).unwrap_or("LaTeX couldn't typeset this").to_string();
        return Err(msg);
    }
    let st = std::process::Command::new("pdftoppm")
        .args(["-png", "-r", "600", "-singlefile", "eq.pdf", "eq"])
        .current_dir(&dir)
        .status()
        .map_err(|e| format!("pdftoppm: {e}"))?;
    if !st.success() {
        return Err("couldn't convert the equation to a picture".into());
    }
    let png = std::fs::read(dir.join("eq.png")).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(png)
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    if !app.equation.open {
        return;
    }
    let mut open = true;
    let mut insert = false;
    egui::Window::new("Equation").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).default_width(420.0).open(&mut open).show(ctx, |ui| {
        ui.label("Enter a LaTeX equation:");
        ui.add(egui::TextEdit::multiline(&mut app.equation.source).code_editor().desired_rows(3).desired_width(f32::INFINITY).hint_text("\\frac{a}{b} = \\sqrt{x^2 + y^2}"));
        if let Some(e) = &app.equation.error {
            ui.label(RichText::new(e).color(egui::Color32::from_rgb(0xff, 0x45, 0x3a)));
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Typeset with pdflatex").small().color(theme::TEXT_DIM()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if app.equation.editing.is_some() { "Update" } else { "Insert" };
                insert = ui.add_enabled(!app.equation.source.trim().is_empty(), egui::Button::new(label)).clicked();
            });
        });
    });
    app.equation.open = open;
    if insert {
        insert_now(app);
    }
}

/// Insert / Update: the typed LaTeX as a picture (where TeX isn't there, as Android, drawn
/// as readable text: x^2 -> x², \alpha -> α).
pub fn insert_now(app: &mut App) {
    let src = app.equation.source.trim().to_string();
    let typeset = if cfg!(target_os = "android") { drawn(app, &src).ok_or_else(|| "Couldn't draw the equation".to_string()) } else { typeset(&src) };
    match typeset {
        Ok(png) => {
            let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
            // an equation being edited: a new picture where the old one was
            let old = app.equation.editing.and_then(|id| app.drawables.iter().find(|d| d.id == id).map(|d| (id, d.geom.x, d.geom.y)));
            let (x, y) = old.map(|o| (o.1, o.2)).unwrap_or_else(|| app.free_spot_pub());
            // (typeset at 600 dpi: shown at the formula's size, about a sixth of its pixels)
            let res = app.edit(if old.is_some() { "Edit Equation" } else { "Insert Equation" }, |d, h| -> Result<nm_core::kn_format::ObjId, nm_core::kn_format::Error> {
                let id = nm_core::sheets::insert_image(d, h, sheet, png.clone(), "equation.png", x, y)?;
                if let Some(mut g) = d.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry) {
                    if let Some((w, hh)) = nm_core::kn_format::objects::image_size(&png) {
                        g.w = w as f32 / 6.0;
                        g.h = hh as f32 / 6.0;
                        d.set_frame(h, id, g);
                    }
                }
                d.set_equation_source(h, id, &src);
                if let Some((o, _, _)) = old {
                    nm_core::sheets::detach_drawables(d, h, sheet, &[o]);
                }
                Ok(id)
            });
            match res {
                Some(Ok(id)) => {
                    app.sel = None;
                    app.canvas.obj = Some(id);
                    app.equation = EquationDialog::default();
                }
                Some(Err(e)) => app.equation.error = Some(e.to_string()),
                None => {}
            }
        }
        Err(e) => app.equation.error = Some(e),
    }
}

/// The equation as text in a picture (PNG, 600 dpi like a typeset one): where there's no TeX.
fn drawn(app: &mut App, src: &str) -> Option<Vec<u8>> {
    use nm_core::kn_model::{Align, CharStyle, VAlign};
    let text = latex_to_text(src);
    let style = CharStyle { size: 13.0, italic: true, font_name: "TimesNewRomanPS-ItalicMT".into(), ..Default::default() };
    let tb = kn_render::simple_text_box(&text, style, Align::Left, VAlign::Top);
    let r = app.canvas.tiles.renderer_mut();
    let tl = kn_render::text::layout(&mut r.fonts, &tb, 2000.0, 200.0, true, true);
    let (w, h) = ((tl.content_w + 4.0).ceil(), (tl.content_h + 4.0).ceil());
    let k = 600.0 / 72.0;
    let mut pm = tiny_skia::Pixmap::new((w * k) as u32, (h * k) as u32)?;
    r.draw_text(&tl, &mut pm, tiny_skia::Transform::from_scale(k, k).pre_translate(2.0, 2.0));
    let img = image::RgbaImage::from_raw(pm.width(), pm.height(), pm.data().to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// A readable rendering of simple LaTeX (Letters'): x^2 -> x², \alpha -> α, \frac{a}{b} -> a⁄b.
pub fn latex_to_text(s: &str) -> String {
    let sup = |c: char| match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹', '+' => '⁺', '-' => '⁻', 'n' => 'ⁿ', 'i' => 'ⁱ', _ => c,
    };
    let sub = |c: char| match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉', '+' => '₊', '-' => '₋', _ => c,
    };
    let names = [
        ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"), ("\\epsilon", "ε"), ("\\theta", "θ"), ("\\lambda", "λ"), ("\\mu", "μ"), ("\\pi", "π"), ("\\sigma", "σ"), ("\\phi", "φ"), ("\\omega", "ω"),
        ("\\Delta", "Δ"), ("\\Sigma", "Σ"), ("\\Omega", "Ω"), ("\\infty", "∞"), ("\\pm", "±"), ("\\times", "×"), ("\\cdot", "·"), ("\\leq", "≤"), ("\\geq", "≥"), ("\\neq", "≠"), ("\\approx", "≈"),
        ("\\sum", "∑"), ("\\int", "∫"), ("\\sqrt", "√"), ("\\rightarrow", "→"), ("\\leftarrow", "←"), ("\\partial", "∂"), ("\\in", "∈"),
    ];
    let mut t = s.to_string();
    for (a, b) in names {
        t = t.replace(a, b);
    }
    // \frac{a}{b}
    while let Some(i) = t.find("\\frac{") {
        let rest = &t[i + 6..];
        let Some(e1) = rest.find('}') else { break };
        let num = rest[..e1].to_string();
        let rest2 = &rest[e1 + 1..];
        let (den, used) = if let Some(r) = rest2.strip_prefix('{') { r.find('}').map(|e| (r[..e].to_string(), e + 2)).unwrap_or((String::new(), 0)) } else { (String::new(), 0) };
        t = format!("{}{}⁄{}{}", &t[..i], num, den, &rest2[used..]);
    }
    let mut out = String::new();
    let cs: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if (c == '^' || c == '_') && i + 1 < cs.len() {
            let map: &dyn Fn(char) -> char = if c == '^' { &sup } else { &sub };
            if cs[i + 1] == '{' {
                let mut j = i + 2;
                while j < cs.len() && cs[j] != '}' {
                    out.push(map(cs[j]));
                    j += 1;
                }
                i = j + 1;
            } else {
                out.push(map(cs[i + 1]));
                i += 2;
            }
            continue;
        }
        if c != '{' && c != '}' && c != '\\' {
            out.push(c);
        }
        i += 1;
    }
    out
}
