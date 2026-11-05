//! The demo library for the website's Bramble Notes page and the store screenshots: a spring trip,
//! with brochure PDFs, photos, checklists and tags.
//! site_demo <empty library dir> <folder with the PDFs and photos>
use nt_core::model::*;
use nt_core::store::Store;
use std::path::Path;

fn p(style: ParaStyle, t: &str) -> Paragraph {
    Paragraph::plain(style, t)
}
fn check(t: &str, done: bool) -> Paragraph {
    Paragraph { checked: done, ..p(ParaStyle::Checklist, t) }
}
fn rich(style: ParaStyle, parts: &[(&str, bool, Option<u8>)]) -> Paragraph {
    let spans = parts.iter().map(|(t, b, h)| Span { text: t.to_string(), bold: *b, highlight: *h, ..Span::default() }).collect();
    Paragraph { style, spans, ..Paragraph::default() }
}

/// What goes in a note: paragraphs, and attachments by file name.
enum Bit {
    P(Paragraph),
    File(&'static str),
}

fn main() {
    let mut a = std::env::args().skip(1);
    let dir = a.next().expect("library dir");
    let media = a.next().expect("media dir");
    let mut st = Store::open_library(Path::new(&dir)).unwrap();
    let trip = st.create_folder("Spring Trip", None).unwrap();
    let kitchen = st.create_folder("Kitchen", None).unwrap();
    let _ideas = st.create_folder("Ideas", None).unwrap();
    use Bit::*;
    use ParaStyle::{Bulleted, Dashed, Heading, Title};
    let body_ = ParaStyle::Body;

    let notes: Vec<(&str, &str, Vec<Bit>, bool, &[&str])> = vec![
        ("Kitchen", "Green market salad", vec![
            P(p(Title, "Green market salad")),
            P(p(body_, "From the stall at the Rialto. Serves four.")),
            File("photos/Daylight/photo-6273.jpg"),
            P(p(Heading, "You'll need")),
            P(p(Bulleted, "Cherry tomatoes, avocado and cucumber")),
            P(p(Bulleted, "Feta, black olives, red onion")),
            P(p(Bulleted, "Olive oil, lemon, oregano")),
            P(rich(body_, &[("Tip: ", true, None), ("dress it at the table, not before.", false, Some(4))])),
        ], false, &["recipes"]),
        ("Ideas", "Photo book layout", vec![
            P(p(Title, "Photo book layout")),
            P(p(body_, "One spread per city, the canal shots full bleed.")),
            P(p(Dashed, "Cover: Grand Canal in the morning")),
            P(p(Dashed, "Tulip rows as the endpapers")),
            P(p(Dashed, "Milan at dusk for the back")),
        ], false, &["ideas"]),
        ("Spring Trip", "Tulip season", vec![
            P(p(Title, "Tulip season")),
            P(p(body_, "The field guide from the visitor centre, and the best spots we found.")),
            File("pdfs/Tulip-Season.pdf"),
            P(p(Heading, "Plan")),
            P(check("Bikes booked for Saturday", true)),
            P(check("Arrive before nine for the light", false)),
            P(check("Picnic by the canal benches", false)),
        ], false, &["travel", "spring"]),
        ("Spring Trip", "Packing list", vec![
            P(p(Title, "Packing list")),
            P(check("Passports", true)),
            P(check("Rail passes, printed", true)),
            P(check("Comfortable shoes", true)),
            P(check("Light jacket for the evenings", false)),
            P(check("Adapter, type L", false)),
            P(check("Room for olive oil", false)),
        ], true, &["travel"]),
        ("Spring Trip", "Venice & Milan", vec![
            P(p(Title, "Venice & Milan")),
            P(p(body_, "Five days, two cities. The full itinerary is attached, and the bookings are below.")),
            File("pdfs/Venice-and-Milan.pdf"),
            P(p(Heading, "Bookings")),
            P(check("Ca' Sagredo, three nights", true)),
            P(check("Frecciarossa to Milan, 9:25", true)),
            P(check("Duomo rooftops, 9:00", false)),
            P(rich(body_, &[("Dinner on day 2: ", true, None), ("Antiche Carampane at 20:30.", false, Some(3))])),
            File("photos/Gallery/photo-6.jpg"),
        ], true, &["travel", "italy"]),
    ];
    for (folder, title, bits, pinned, tags) in notes {
        let fid = if folder == "Kitchen" { kitchen.clone() } else if folder == "Spring Trip" { trip.clone() } else { _ideas.clone() };
        let mut body = Body { paragraphs: vec![p(Title, title)] };
        let n = st.create_note(Some(&fid), &mut body).unwrap();
        let mut paras = Vec::new();
        for b in bits {
            match b {
                P(par) => paras.push(par),
                File(f) => {
                    let att = st.add_attachment(&n.id, &Path::new(&media).join(f)).unwrap();
                    paras.push(Paragraph { spans: vec![Span::token(&att.id, &att.uti)], ..Paragraph::default() });
                }
            }
        }
        let mut body = Body { paragraphs: paras };
        st.update_note(&n.id, &mut body).unwrap();
        if pinned {
            st.set_pinned(&n.id, true).unwrap();
        }
        for t in tags {
            st.add_tag(&n.id, t).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(1100));
        eprintln!("{title}");
    }
}
