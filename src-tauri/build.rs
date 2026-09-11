fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/pastebox/macos.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("hvr_pastebox");
        for framework in [
            "Cocoa",
            "ApplicationServices",
            "Carbon",
            "UserNotifications",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rerun-if-changed=src/pastebox/macos.m");
    }
    tauri_build::build()
}
