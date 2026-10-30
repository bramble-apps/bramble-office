//! Insert > Formula > Stock Quote: STOCK / STOCKH formulas and their quotes. Quotes come from
//! Yahoo's free chart service, fetched only when a quote is inserted or refreshed (never on
//! open or in the background), off the UI thread, with a short timeout; only the ticker is sent.
//! The values go in the document's own quote store (kn_model::remotedata), like Numbers keeps
//! them, so formulas show the last fetched value until the next refresh.

use std::sync::mpsc::{channel, Receiver};

use nm_core::kn_model::formula::{STOCK_ARGS, STOCK_ATTRIBUTES};
use nm_core::kn_model::remotedata::Quote;

use crate::app::App;

/// A quote wanted: (function 298 STOCK / 299 STOCKH, symbol, attribute, STOCKH day since 1970).
pub type Want = (u32, String, u32, Option<i64>);

#[derive(Default)]
pub struct StockQuote {
    /// The dialog is open.
    pub open: bool,
    symbol: String,
    attr: usize,
    /// Put the keyboard in the symbol field (just opened).
    focus: bool,
    /// Quotes being fetched.
    busy: Option<Receiver<Vec<(Want, Result<Quote, String>)>>>,
}

impl StockQuote {
    /// Quotes are being fetched.
    pub fn busy(&self) -> bool {
        self.busy.is_some()
    }
}

/// Open the dialog (for the selected cell).
pub fn open(app: &mut App) {
    if app.sel.is_none() {
        app.notify("Select a cell for the stock quote first");
        return;
    }
    app.stocks.open = true;
    app.stocks.focus = true;
    if app.stocks.symbol.is_empty() {
        app.stocks.symbol = "AAPL".into();
    }
}

/// Refresh every quote the document holds.
pub fn refresh(app: &mut App, ctx: &egui::Context) {
    let Some(d) = app.doc() else { return };
    let mut wants: Vec<Want> = d.remote_values().into_keys().map(|(f, s, a, date)| (f, s, a, date.map(|k| k / 86_400 + 11_323))).collect();
    if wants.is_empty() {
        app.notify("There are no stock quotes to refresh");
        return;
    }
    wants.sort();
    fetch(app, ctx, wants);
}

fn fetch(app: &mut App, ctx: &egui::Context, wants: Vec<Want>) {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(fetch_all(&wants));
        ctx.request_repaint();
    });
    app.stocks.busy = Some(rx);
}

/// The dialog, and quotes arriving (call every frame).
pub fn window(app: &mut App, ctx: &egui::Context) {
    if let Some(got) = app.stocks.busy.as_ref().and_then(|rx| rx.try_recv().ok()) {
        app.stocks.busy = None;
        apply(app, got);
    }
    if !app.stocks.open {
        return;
    }
    let mut open = true;
    let mut insert = false;
    let mut refresh_all = false;
    let busy = app.stocks.busy.is_some();
    let s = &mut app.stocks;
    egui::Window::new("Stock Quote").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).open(&mut open).show(ctx, |ui| {
        egui::Grid::new("stock").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
            ui.label("Symbol:");
            let r = ui.add(egui::TextEdit::singleline(&mut s.symbol).hint_text("AAPL").desired_width(160.0));
            if std::mem::take(&mut s.focus) {
                r.request_focus();
            }
            // Return inserts (and stays with the dialog: the table doesn't move down)
            if r.lost_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                insert = true;
            }
            ui.end_row();
            ui.label("Attribute:");
            // Numbers' stock popover list, in its order (the formula gets the number)
            egui::ComboBox::from_id_salt("stock-attr").width(160.0).selected_text(STOCK_ARGS[s.attr].1).show_ui(ui, |ui| {
                for (i, (_, n, id)) in STOCK_ARGS.iter().enumerate() {
                    ui.add_enabled_ui(id.is_some(), |ui| ui.selectable_value(&mut s.attr, i, *n));
                }
            });
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Quotes come from Yahoo Finance; only the symbol is sent.").small().weak());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Refresh All")).clicked() {
                refresh_all = true;
            }
            if busy {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!s.symbol.trim().is_empty() && !busy, egui::Button::new("Insert")).clicked() {
                    insert = true;
                }
                if ui.button("Cancel").clicked() {
                    s.open = false;
                }
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.open = false;
        }
    });
    if !open {
        app.stocks.open = false;
    }
    if refresh_all {
        refresh(app, ctx);
    }
    if insert && !app.stocks.symbol.trim().is_empty() {
        insert_quote(app, ctx);
    }
}

/// Put =STOCK("SYM","attribute") in the selected cell and fetch its quote.
fn insert_quote(app: &mut App, ctx: &egui::Context) {
    let Some(sel) = app.sel else { return };
    crate::canvas::commit(app);
    let symbol = app.stocks.symbol.trim().to_uppercase();
    let (k, _, Some(attr)) = STOCK_ARGS[app.stocks.attr] else { return };
    let (r, c) = sel.cursor;
    let Some(row) = app.mrow(sel.table, r) else { return };
    // as Numbers' picker writes it: the attribute's number (0 = price)
    let f = format!("=STOCK(\"{symbol}\",{k})");
    if let Some(Err(e)) = app.edit("Stock Quote", |d, h| d.set_formula_text(h, sel.table, row, c, &f)) {
        app.notify(format!("Stock Quote: {e}"));
        return;
    }
    app.stocks.open = false;
    fetch(app, ctx, vec![(298, symbol, attr, None)]);
}

/// Store the quotes that arrived (one undo step); failures keep the last value and say so.
fn apply(app: &mut App, got: Vec<(Want, Result<Quote, String>)>) {
    let mut fails: Vec<String> = Vec::new();
    let ok: Vec<(Want, Quote)> = got
        .into_iter()
        .filter_map(|(w, q)| match q {
            Ok(q) => Some((w, q)),
            Err(e) => {
                if !fails.contains(&e) {
                    fails.push(e);
                }
                None
            }
        })
        .collect();
    if !ok.is_empty() {
        let res = app.edit("Stock Quote", |d, h| ok.iter().try_for_each(|((f, s, a, date), q)| d.set_remote_value(h, *f, s, *a, *date, q)));
        if let Some(Err(e)) = res {
            fails.push(format!("{e}"));
        }
    }
    if !fails.is_empty() {
        app.notify(format!("Stock quotes not updated (the last values are kept): {}", fails.join("; ")));
    }
}

// ---------------------------------------------------------------------------------- Yahoo

fn fetch_all(wants: &[Want]) -> Vec<(Want, Result<Quote, String>)> {
    // one request per symbol (and per STOCKH day)
    let mut cache: Vec<((String, Option<i64>), Result<String, String>)> = Vec::new();
    wants
        .iter()
        .map(|w| {
            let k = (w.1.clone(), w.3);
            let body = match cache.iter().find(|(c, _)| *c == k) {
                Some((_, b)) => b.clone(),
                None => {
                    let b = get(&w.1, w.3);
                    cache.push((k, b.clone()));
                    b
                }
            };
            let q = body.and_then(|b| quote(&b, w.2, w.3.is_some()));
            (w.clone(), q)
        })
        .collect()
}

/// The chart JSON for a symbol (the last five days, or one day for STOCKH).
fn get(symbol: &str, day: Option<i64>) -> Result<String, String> {
    if symbol.is_empty() || !symbol.chars().all(|c| c.is_ascii_alphanumeric() || ".-^=".contains(c)) {
        return Err(format!("“{symbol}” isn't a stock symbol"));
    }
    let q = match day {
        Some(d) => format!("period1={}&period2={}&interval=1d", d * 86_400, (d + 1) * 86_400),
        None => "range=5d&interval=1d".into(),
    };
    let url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?{q}");
    let out = std::process::Command::new("curl")
        .args(["-s", "-f", "--max-time", "8", "-A", "Mozilla/5.0", &url])
        .output()
        .map_err(|_| "curl isn't installed".to_string())?;
    if !out.status.success() {
        return Err(if out.status.code() == Some(22) { format!("Yahoo doesn't know “{symbol}”") } else { "couldn't reach Yahoo Finance".into() });
    }
    let body = String::from_utf8_lossy(&out.stdout).to_string();
    if !body.contains("\"meta\"") {
        return Err(format!("Yahoo doesn't know “{symbol}”"));
    }
    Ok(body)
}

/// A number field of the response ("key":123.4).
fn num(b: &str, key: &str) -> Option<f64> {
    let i = b.find(&format!("\"{key}\":"))? + key.len() + 3;
    let s = &b[i..];
    let end = s.find([',', '}', ']']).unwrap_or(s.len());
    s[..end].trim().parse().ok()
}

/// A text field ("key":"text").
fn text(b: &str, key: &str) -> Option<String> {
    let i = b.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let s = &b[i..];
    Some(s[..s.find('"')?].replace("\\u0026", "&"))
}

/// A daily series ("close":[1.0,null,…]).
fn series(b: &str, key: &str) -> Vec<Option<f64>> {
    let Some(i) = b.find(&format!("\"{key}\":[")) else { return Vec::new() };
    let s = &b[i + key.len() + 4..];
    s[..s.find(']').unwrap_or(0)].split(',').map(|v| v.trim().parse().ok()).collect()
}

fn last(v: &[Option<f64>]) -> Option<f64> {
    v.iter().rev().flatten().next().copied()
}

/// One attribute from the chart response.
fn quote(b: &str, attr: u32, history: bool) -> Result<Quote, String> {
    let cur = text(b, "currency");
    let money = |x: Option<f64>| x.map(|x| Quote::Number(x, cur.clone()));
    let plain = |x: Option<f64>| x.map(|x| Quote::Number(x, None));
    let price = num(b, "regularMarketPrice");
    // the close before today's (today's is null until the market closes)
    let closes = series(b, "close");
    let prev = {
        let n = closes.len();
        let upto = if closes.last().is_some_and(|c| c.is_none()) || n < 2 { n.saturating_sub(1) } else { n - 1 };
        last(&closes[..upto])
    };
    let name = STOCK_ARGS.iter().find(|(_, _, k)| *k == Some(attr)).map(|(_, n, _)| *n).or_else(|| STOCK_ATTRIBUTES.iter().find(|(_, k)| *k == attr).map(|(n, _)| *n)).unwrap_or("that attribute");
    let q = if history {
        match attr {
            23 | 24 => money(last(&closes)),
            20 => money(last(&series(b, "open"))),
            9 => money(last(&series(b, "high"))),
            10 => money(last(&series(b, "low"))),
            25 => plain(last(&series(b, "volume"))),
            _ => None,
        }
    } else {
        match attr {
            24 => money(price),
            // previous close
            23 => money(prev),
            7 => money(price.zip(prev).map(|(p, c)| p - c)),
            22 => price.zip(prev).filter(|(_, c)| *c != 0.0).map(|(p, c)| Quote::Percent((p - c) / c)),
            20 => money(last(&series(b, "open"))),
            9 => money(num(b, "regularMarketDayHigh")),
            10 => money(num(b, "regularMarketDayLow")),
            25 => plain(num(b, "regularMarketVolume")),
            15 => money(num(b, "fiftyTwoWeekHigh")),
            16 => money(num(b, "fiftyTwoWeekLow")),
            18 => text(b, "longName").or_else(|| text(b, "shortName")).map(Quote::Text),
            14 => text(b, "fullExchangeName").or_else(|| text(b, "exchangeName")).map(Quote::Text),
            8 => cur.clone().map(Quote::Text),
            26 => text(b, "symbol").map(Quote::Text),
            _ => None,
        }
    };
    q.ok_or_else(|| format!("Yahoo's free quotes don't include {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{"chart":{"result":[{"meta":{"currency":"USD","symbol":"AAPL","exchangeName":"NMS","fullExchangeName":"NasdaqGS","regularMarketPrice":342.94,"fiftyTwoWeekHigh":348.15,"fiftyTwoWeekLow":243.42,"regularMarketDayHigh":345.99,"regularMarketDayLow":340.115,"regularMarketVolume":34524344,"longName":"Apple Inc.","shortName":"Apple Inc.","chartPreviousClose":340.97},"timestamp":[1,2,3],"indicators":{"quote":[{"volume":[1,2,3],"high":[1,2,3],"low":[1,2,3],"open":[344.67,346.95,340.975],"close":[342.0,343.46,null]}],"adjclose":[{"adjclose":[341.0,342.0,null]}]}}],"error":null}}"#;

    #[test]
    fn reads_yahoo() {
        let usd = Some("USD".to_string());
        assert_eq!(quote(BODY, 24, false), Ok(Quote::Number(342.94, usd.clone())));
        assert_eq!(quote(BODY, 18, false), Ok(Quote::Text("Apple Inc.".into())));
        assert_eq!(quote(BODY, 14, false), Ok(Quote::Text("NasdaqGS".into())));
        assert_eq!(quote(BODY, 20, false), Ok(Quote::Number(340.975, usd.clone())));
        match quote(BODY, 7, false) {
            Ok(Quote::Number(x, _)) => assert!((x - (342.94 - 343.46)).abs() < 1e-9),
            q => panic!("{q:?}"),
        }
        assert!(matches!(quote(BODY, 22, false), Ok(Quote::Percent(p)) if (p + 0.001514).abs() < 1e-5));
        assert!(quote(BODY, 17, false).unwrap_err().contains("Market Cap"));
        assert_eq!(quote(BODY, 23, true), Ok(Quote::Number(343.46, usd)));
        assert!(get("A B", None).is_err());
    }
}
