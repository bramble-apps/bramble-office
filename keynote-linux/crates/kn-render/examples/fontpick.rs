fn main() {
    let mut f = kn_render::fonts::Fonts::new();
    for n in std::env::args().skip(1) {
        println!("{n} -> {:?}", f.choose(&n, false, false));
    }
}
