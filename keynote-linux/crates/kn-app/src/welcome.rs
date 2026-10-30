//! The start screen (no presentation open), after Keynote's theme chooser: kn_ui::welcome with
//! the recent presentations and the themes.

use crate::app::App;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let themes = crate::themes::available();
    let templates: Vec<kn_ui::welcome::Template> = themes.iter().map(|(name, path)| kn_ui::welcome::Template { name: name.clone(), preview: crate::themes::preview(&ctx, name, path) }).collect();
    let recent = App::recent_files();
    let spec = kn_ui::welcome::Spec {
        app: "Keydeck",
        id: "keydeck",
        heading: "Choose a Theme",
        section: "Themes",
        all: "All Themes",
        hint: "or drop a .key file here",
        recent: &recent,
        templates: &templates,
        default: "Daylight",
        tile: egui::vec2(176.0, 99.0),
        draw: &|_, _, _| {},
        cancel: false,
        loading: app.loading(),
        render: None,
    };
    match kn_ui::welcome::show(ui, &spec) {
        Some(kn_ui::welcome::Action::Open(p)) => app.open_path(p),
        Some(kn_ui::welcome::Action::Browse) => app.pick_open(),
        Some(kn_ui::welcome::Action::Create(i)) => {
            if let Some((n, p)) = themes.get(i).cloned() {
                crate::themes::create_from(app, &ctx, &n, p);
            }
        }
        Some(kn_ui::welcome::Action::Cancel) | None => {}
    }
}
