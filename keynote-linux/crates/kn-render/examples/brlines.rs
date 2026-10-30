//! cargo run -p kn-render --example brlines : line count of text ending in line breaks.
fn main() {
    let mut f = kn_render::fonts::Fonts::new();
    for t in ["abc", "abc\u{2028}", "abc\u{2028}\u{2028}", "abc\u{2028}def"] {
        let st = kn_model::CharStyle { size: 11.0, font_name: "Helvetica".into(), ..Default::default() };
        let tb = kn_render::simple_text_box(t, st, kn_model::Align::Left, kn_model::VAlign::Top);
        let tl = kn_render::text::layout(&mut f, &tb, 400.0, 1000.0, false, true);
        println!("{:?}: {} lines, height {:.1}", t, tl.lines.len(), tl.content_h);
    }
}
