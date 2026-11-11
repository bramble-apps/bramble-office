use crate::app::App;
pub fn piano(app: &mut App, ctx: &egui::Context, t: usize) {
    crate::keyswin::generic(app, ctx, t, "Studio Piano", "Studio Piano");
}
pub fn ensemble(app: &mut App, ctx: &egui::Context, t: usize) {
    crate::keyswin::generic(app, ctx, t, "Studio Strings", "Studio Strings");
}
pub fn bass(app: &mut App, ctx: &egui::Context, t: usize) {
    crate::keyswin::generic(app, ctx, t, "Studio Bass", "Studio Bass");
}
