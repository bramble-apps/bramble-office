//! Values: a Numbers-compatible spreadsheet app for Linux, reading and writing
//! real `.numbers` files through Keydeck's iWork crates.
//!
//! Automated checks with no human present (the app screenshots itself and quits):
//! `NM_SNAP=out.png [NM_SHEET=n] [NM_CELL=row,col[,row2,col2]] [NM_EDIT=text] [NM_INSPECTOR=table|cell|text|arrange] [NM_ZOOM=0.75] [NM_CMD=insert-table,add-sheet,undo,redo] [NM_SCRIPT="click x y; drag x1 y1 x2 y2; type text; key Enter; wait"] numdeck file.numbers`
// Windows: a window app, without a console window behind it
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result {
    nm_app::run()
}
