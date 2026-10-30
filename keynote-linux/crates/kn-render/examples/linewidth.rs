//! cargo run -p kn-render --example linewidth -- "text" size [italic] : a line's width in Word mode.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut f = kn_render::fonts::Fonts::new();
    f.word = true;
    let st = kn_model::CharStyle { size: a[2].parse().unwrap(), font_name: "Helvetica".into(), italic: a.get(3).is_some(), ..Default::default() };
    let tb = kn_render::simple_text_box(&a[1], st, kn_model::Align::Left, kn_model::VAlign::Top);
    let tl = kn_render::text::layout(&mut f, &tb, 2000.0, 1000.0, true, true);
    let last = tl.glyphs.iter().filter(|g| !g.label).map(|g| g.x + g.w).fold(0.0f32, f32::max);
    println!("width {:.2} (line {:.2}), {} glyphs", last, tl.lines[0].width, tl.glyphs.len());
}
