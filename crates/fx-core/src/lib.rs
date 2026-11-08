//! Cœur de Clicksy : configuration, raccourcis, chemins portables, état partagé.

pub mod color;
pub mod config;
pub mod hotkey;
pub mod paths;
pub mod state;

pub use color::Color;
pub use config::*;
pub use hotkey::Hotkey;
pub use state::{Banner, DrawState, FxState, KeyShow, Modes, Ripple, Shape, Shared};
