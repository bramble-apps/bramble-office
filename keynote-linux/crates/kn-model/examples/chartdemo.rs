//! chartdemo in.key out.key: style every chart as a check of the chart properties (background,
//! preset 3, Avenir two sizes up, border, rounded corners 10 %, gaps, series 1 red, Y axis 0-100 in 4 steps with minor gridlines and
//! ticks, angled category labels, value labels with 2 decimals, pie wedge 1 pulled out).
use kn_model::chartstyle::{ChartPart, ChartValue};
use kn_model::format::{Dash, FillSpec, StrokeSpec};
use kn_model::{chartdata::ChartFlag, Color, Document};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let mut h = kn_model::edit::History::default();
    let charts: Vec<_> = d.file.objects_of("TSCH.ChartDrawableArchive").map(|(id, _)| id).collect();
    d.begin(&mut h, "Style charts");
    let presets = d.chart_presets();
    for c in charts {
        // preset 3 and Avenir two sizes up first (they replace the chart's styles)
        if let Some(p) = presets.get(2) {
            d.apply_chart_preset(&mut h, c, *p);
        }
        d.set_chart_font(&mut h, c, Some("Avenir-Book"), 1.11 * 1.11);
        let grey = Color::rgba(0.92, 0.92, 0.92, 1.0);
        let set = |d: &mut Document, h: &mut kn_model::edit::History, part, name: &str, v: ChartValue| {
            if !d.set_chart_prop(h, c, part, name, &v) {
                eprintln!("not set: {name}");
            }
        };
        d.set_chart_background(&mut h, c, &FillSpec::Color(grey));
        d.set_chart_flag(&mut h, c, ChartFlag::Border(true));
        set(&mut d, &mut h, ChartPart::Chart, "tschchartinfodefaultborderstroke", ChartValue::Stroke(Some(StrokeSpec { color: Color::rgba(0.2, 0.2, 0.2, 1.0), width: 2.0, dash: Dash::Solid })));
        set(&mut d, &mut h, ChartPart::Chart, "tschchartinfodefaultroundedcornerradius", ChartValue::Float(0.1));
        set(&mut d, &mut h, ChartPart::Chart, "tschchartinfodefaultintersetgap", ChartValue::Float(50.0));
        d.set_series_fill(&mut h, c, 0, &FillSpec::Color(Color::rgba(0.9, 0.1, 0.1, 1.0)));
        set(&mut d, &mut h, ChartPart::ValueAxis(0), "tschchartaxisdefaultusermin", ChartValue::Number(0.0));
        set(&mut d, &mut h, ChartPart::ValueAxis(0), "tschchartaxisdefaultusermax", ChartValue::Number(100.0));
        set(&mut d, &mut h, ChartPart::ValueAxis(0), "tschchartaxisvaluenumberofmajorgridlines", ChartValue::Int(4));
        set(&mut d, &mut h, ChartPart::ValueAxis(0), "tschchartaxisvalueshowminorgridlines", ChartValue::Bool(true));
        set(&mut d, &mut h, ChartPart::ValueAxis(0), "tschchartaxisvalueshowmajortickmarks", ChartValue::Bool(true));
        set(&mut d, &mut h, ChartPart::CategoryAxis, "tschchartaxiscategorylabelsorientation", ChartValue::Float(45.0));
        d.set_chart_flag(&mut h, c, ChartFlag::ValueLabels(true));
        let two = kn_model::tablefmt::CellFormat::Number { decimals: Some(2), thousands: false, negative: 0 }.msg();
        for i in 0..2 {
            d.set_series_number_format(&mut h, c, i, &two);
        }
        set(&mut d, &mut h, ChartPart::Series(0), "tschchartseriespiewedgeexplosion", ChartValue::Float(0.15));
    }
    d.end(&mut h);
    std::fs::write(&a[2], d.file.to_zip().unwrap()).unwrap();
    println!("saved {}", a[2]);
}
