pub const CLOCK: &[u8] = include_bytes!("../assets/fonts/fredoka/Fredoka-Medium.ttf");
pub const POLAROID: &[u8] = include_bytes!("../assets/fonts/caveat/Caveat[wght].ttf");

pub fn print_licenses() {
    println!(
        "Fredoka font license:\n{}\nCaveat font license:\n{}",
        include_str!("../assets/fonts/fredoka/OFL.txt"),
        include_str!("../assets/fonts/caveat/OFL.txt")
    );
}
