//! Insert > Equation: LaTeX typeset with the system's TeX (pdflatex + pdftoppm), inserted as a
//! picture that remembers its LaTeX source (equation_source_text, as Keynote stores it), so it
//! can be edited again here or in Keynote.

use egui::RichText;

use crate::app::App;
use crate::theme;

#[derive(Default)]
pub struct EquationDialog {
    pub open: bool,
    pub source: String,
    error: Option<String>,
}

/// PNG of a LaTeX formula (math mode), or a readable error.
fn typeset(latex: &str) -> Result<Vec<u8>, String> {
    let dir = std::env::temp_dir().join(format!("kn-eq-{}", std::process::id()));
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
                insert = ui.add_enabled(!app.equation.source.trim().is_empty(), egui::Button::new("Insert")).clicked();
            });
        });
    });
    app.equation.open = open;
    if insert {
        let src = app.equation.source.trim().to_string();
        match typeset(&src) {
            Ok(png) => {
                crate::editing::insert_object(app, kn_model::insert::NewObject::Image(png, "equation.png".into()));
                if let (Some(&id), Some(doc)) = (app.sel.first(), app.doc.as_mut()) {
                    doc.begin(&mut app.hist, "Insert Equation");
                    doc.set_equation_source(&mut app.hist, id, &src);
                    doc.end(&mut app.hist);
                }
                app.equation = EquationDialog::default();
            }
            Err(e) => app.equation.error = Some(e),
        }
    }
}
