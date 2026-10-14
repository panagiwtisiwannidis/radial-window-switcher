fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if target_windows && cfg!(windows) {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/app.ico")
            .set("ProductName", "Radial Switcher")
            .set("FileDescription", "Radial Switcher")
            .set("CompanyName", "Panagiotis Ioannidis")
            .set(
                "LegalCopyright",
                "© 2026 Panagiotis Ioannidis. All rights reserved.",
            );
        resource.compile().expect("compile Windows resources");
    }
}
