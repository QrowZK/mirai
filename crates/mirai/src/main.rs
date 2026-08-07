//! Mirai: an extremely lightweight, privacy-focused browser built on Servo.

mod app;
mod delegate;
mod ui;

use std::error::Error;

use winit::event_loop::EventLoop;

use crate::app::{App, WakerEvent};

const DEFAULT_URL: &str = "https://duckduckgo.com/";

fn main() -> Result<(), Box<dyn Error>> {
    // Logging is installed by `servo.setup_logging()` once Servo starts; it
    // honours RUST_LOG and would conflict with a logger installed here.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_URL.into());
    let url = url::Url::parse(&url).or_else(|_| url::Url::parse(&format!("https://{url}")))?;

    let event_loop = EventLoop::<WakerEvent>::with_user_event().build()?;
    let mut app = App::new(&event_loop, url);
    event_loop.run_app(&mut app)?;
    Ok(())
}
