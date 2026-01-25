//! TUI (Terminal User Interface) for the joystick mapper
//!
//! Provides live visualization of axis values, key depths,
//! and configuration editing.

pub mod app;
pub mod keyboard_layout;
pub mod render;

pub use app::{App, AppMode, SelectedElement};
