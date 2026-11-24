//! Create a clean 1-slide blank `.key` presentation from a Keynote theme file.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err(format!("usage: {} <theme.key> <out.key>", args[0]).into());
    }
    let mut doc = kn_model::Document::new_from_theme(kn_format::KeyFile::open(&args[1])?)?;
    doc.save_as(&args[2])?;
    Ok(())
}
