//! Format > Font > Show Fonts (⌘T) and Show Colors (⇧⌘C): small panels in place of the Mac's
//! font and colour panels, applying a font, size or text colour to the selection (or to what's
//! typed next), stored as the run's font and colour as Notes keeps them.

use super::Editor;
use super::layout;

/// The crayon box (the Mac colour panel's pencils), by row.
const CRAYONS: [[u32; 8]; 6] = [
    [0x000000, 0x191919, 0x333333, 0x4C4C4C, 0x666666, 0x7F7F7F, 0x999999, 0xB2B2B2],
    [0xCCCCCC, 0xE5E5E5, 0xFFFFFF, 0x800000, 0x808000, 0x008000, 0x008080, 0x000080],
    [0x800080, 0x804000, 0x408000, 0x008040, 0x004080, 0x400080, 0x800040, 0xFF0000],
    [0xFFFF00, 0x00FF00, 0x00FFFF, 0x0000FF, 0xFF00FF, 0xFF8000, 0x80FF00, 0x00FF80],
    [0x0080FF, 0x8000FF, 0xFF0080, 0xFF6666, 0xFFFF66, 0x66FF66, 0x66FFFF, 0x6666FF],
    [0xFF66FF, 0xFFCC66, 0xCCFF66, 0x66FFCC, 0x66CCFF, 0xCC66FF, 0xFF66CC, 0x996633],
];

const SIZES: [f32; 14] = [9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 18.0, 24.0, 36.0, 48.0, 64.0, 72.0, 96.0, 144.0];

pub fn panels(ed: &mut Editor, ctx: &egui::Context) {
    if ed.view.fonts_panel {
        let mut open = true;
        let cur = ed.para(ed.head).fmt_at(ed.head.off);
        let mut pick: Option<Box<dyn FnOnce(&mut Editor)>> = None;
        egui::Window::new("Fonts").id(egui::Id::unique("nt-fonts")).open(&mut open).collapsible(false).resizable(false).default_width(320.0).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.weak("Family");
                    egui::ScrollArea::vertical().id_salt("fam").max_height(180.0).show(ui, |ui| {
                        let name = cur.font.as_ref().and_then(|f| f.name.clone());
                        if ui.selectable_label(name.is_none(), "Default").clicked() {
                            pick = Some(Box::new(|e: &mut Editor| e.set_chars(|f| if let Some(x) = f.font.as_mut() { x.name = None } )));
                        }
                        for (disp, _, _) in layout::markup_fonts() {
                            if ui.selectable_label(name.as_deref() == Some(disp), disp).clicked() {
                                let d = disp.to_string();
                                pick = Some(Box::new(move |e: &mut Editor| e.set_chars(|f| f.font.get_or_insert_with(Default::default).name = Some(d.clone()))));
                            }
                        }
                    });
                });
                ui.vertical(|ui| {
                    ui.weak("Typeface");
                    for (label, b, i) in [("Regular", false, false), ("Bold", true, false), ("Italic", false, true), ("Bold Italic", true, true)] {
                        if ui.selectable_label(cur.bold == b && cur.italic == i, label).clicked() {
                            pick = Some(Box::new(move |e: &mut Editor| e.set_chars(|f| {
                                f.bold = b;
                                f.italic = i;
                            })));
                        }
                    }
                });
                ui.vertical(|ui| {
                    ui.weak("Size");
                    egui::ScrollArea::vertical().id_salt("size").max_height(180.0).show(ui, |ui| {
                        let size = cur.font.as_ref().and_then(|f| f.size);
                        if ui.selectable_label(size.is_none(), "Default").clicked() {
                            pick = Some(Box::new(|e: &mut Editor| e.set_chars(|f| if let Some(x) = f.font.as_mut() { x.size = None } )));
                        }
                        for s in SIZES {
                            if ui.selectable_label(size == Some(s), format!("{s}")).clicked() {
                                pick = Some(Box::new(move |e: &mut Editor| e.set_chars(|f| f.font.get_or_insert_with(Default::default).size = Some(s))));
                            }
                        }
                    });
                });
            });
        });
        if let Some(f) = pick {
            f(ed);
            // (a font with nothing set is no font)
            ed.set_chars(|f| {
                if f.font.as_ref().is_some_and(|x| x.name.is_none() && x.size.is_none() && x.hints.is_none()) {
                    f.font = None;
                }
            });
            ed.changed = true;
        }
        if !open {
            ed.view.fonts_panel = false;
        }
    }
    if ed.view.colors_panel {
        let mut open = true;
        let mut pick: Option<Option<[f32; 4]>> = None;
        egui::Window::new("Colors").id(egui::Id::unique("nt-colors")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            for row in CRAYONS {
                ui.horizontal(|ui| {
                    for c in row {
                        let (r, g, b) = ((c >> 16) as u8, (c >> 8) as u8, c as u8);
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 18.0), egui::Sense::click());
                        ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
                        ui.painter().rect_stroke(rect, 3.0, egui::Stroke::new(1.0, egui::Color32::from_black_alpha(40)), egui::StrokeKind::Inside);
                        if resp.clicked() {
                            pick = Some(Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]));
                        }
                    }
                });
            }
            ui.add_space(4.0);
            if ui.button("Default Color").clicked() {
                pick = Some(None);
            }
        });
        if let Some(c) = pick {
            ed.set_chars(|f| f.color = c);
            ed.changed = true;
        }
        if !open {
            ed.view.colors_panel = false;
        }
    }
}
