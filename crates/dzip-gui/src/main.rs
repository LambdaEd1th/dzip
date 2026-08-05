#![cfg_attr(feature = "bundle", windows_subsystem = "windows")]

#[cfg(all(feature = "desktop", feature = "web"))]
compile_error!("features `desktop` and `web` are mutually exclusive");
#[cfg(not(any(feature = "desktop", feature = "web")))]
compile_error!("enable either the `desktop` or `web` feature");

mod app;
mod app_i18n;
mod archive_input;
mod background;
mod browser;
mod components;
mod file_drop;
mod i18n;
mod icons;
mod platform;
mod preferences;
mod state;
mod task;
#[cfg(feature = "web")]
mod worker_client;

fn main() {
    app::launch();
}
