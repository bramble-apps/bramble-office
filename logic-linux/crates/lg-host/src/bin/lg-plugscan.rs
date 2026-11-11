//! Bramble Studio's plug-in scanner process: `lg-plugscan --scan-plugin FORMAT PATH` loads one
//! plug-in file and prints what's in it (see lg_host::scan). Studio's own binary does the same
//! with the same arguments; this one is for checks and for running the scan by hand.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = if args.first().is_some_and(|a| a == "--scan-plugin") { &args[1..] } else { &args[..] };
    lg_host::scan::child_main(rest)
}
