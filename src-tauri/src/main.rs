#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use log;

fn main() {
    std::env::set_var("RUST_LOG", "info");
    env_logger::init();
    log::info!("Starting hit-vvc application...");
    voice_vibe_local_lib::run();
}
