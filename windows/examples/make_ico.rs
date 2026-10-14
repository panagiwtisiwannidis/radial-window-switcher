fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "assets/app.ico".into());
    std::fs::write(
        &path,
        radial_switcher::icon::ico_file(&[16, 24, 32, 48, 64, 256]),
    )
    .expect("write ico");
    println!("wrote {path}");
}
