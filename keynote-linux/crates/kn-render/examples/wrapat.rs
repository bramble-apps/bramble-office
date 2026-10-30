//! cargo run -p kn-render --example wrapat -- "text" size width [italic] : lines in Word mode.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut f = kn_render::fonts::Fonts::new();
    f.word = true;
    let st = kn_model::CharStyle { size: a[2].parse().unwrap(), font_name: "Helvetica".into(), italic: a.get(4).is_some(), ..Default::default() };
    let mut tb = kn_render::simple_text_box(&a[1], st, kn_model::Align::Left, kn_model::VAlign::Top);
    tb.padding = [0.0; 4];
    let tl = kn_render::text::layout(&mut f, &tb, a[3].parse().unwrap(), 1000.0, false, true);
    for l in &tl.lines {
        println!("{:7.2} {:?}", l.width, &tb.text[l.start..l.end]);
    }
}
