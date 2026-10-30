//! t40charts <in.key> <out.key>: insert a default column chart (value labels on) and a default
//! pie (series names on) on slide 1, for checking in Keynote.
use kn_model::chartdata::ChartFlag;
use kn_model::edit::History;
use kn_model::insert::NewObject;
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides()[0].slide;
    let mut h = History::default();
    d.begin(&mut h, "Charts");
    let col = d.insert(&mut h, s, &NewObject::Chart { pie: false }).unwrap();
    d.move_by(&mut h, col, -250.0, 0.0);
    assert!(d.set_chart_flag(&mut h, col, ChartFlag::ValueLabels(true)));
    let pie = d.insert(&mut h, s, &NewObject::Chart { pie: true }).unwrap();
    d.move_by(&mut h, pie, 300.0, 0.0);
    assert!(d.set_chart_flag(&mut h, pie, ChartFlag::SeriesNames(true)));
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
