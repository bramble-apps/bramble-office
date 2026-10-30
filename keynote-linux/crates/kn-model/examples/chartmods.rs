//! Mac test: change chart types / legend in the catalog's chart slides (91 column -> bar + legend,
//! 92 stacked column -> pie, 93 bar -> 3D column + legend). cargo run --example chartmods -- in.key out.key
use kn_model::DrawableKind;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "t");
    for (slide, ty, legend) in [(91, "barChartType2D", true), (92, "pieChartType2D", false), (93, "columnChartType3D", true)] {
        let c = d.scene(&s, slide - 1).unwrap().drawables.into_iter().find(|x| matches!(x.kind, DrawableKind::Chart(_))).unwrap().id;
        assert!(d.set_chart_type(&mut h, c, ty));
        d.set_chart_legend(&mut h, c, legend);
    }
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
