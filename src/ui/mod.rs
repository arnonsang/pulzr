pub mod tui;

#[cfg(not(target_arch = "wasm32"))]
pub mod webui;

pub use tui::*;
