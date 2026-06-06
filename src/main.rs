// Entry point. Wires the Slint UI to the config store, system sampler and
// SSH / RDP session managers.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod i18n;
mod rdp;
mod rdp_ffi;
mod sftp;
mod ssh;
mod system;

fn main() -> anyhow::Result<()> {
    // Initialise tracing — honour RUST_LOG but default to info.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    app::run()
}
