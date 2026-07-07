// Tab implementations — each tab's types, rendering, and input handling

pub(super) mod audio;
pub(super) mod depth;
pub(super) mod device_info;
pub(super) mod key_mapping;
#[cfg(feature = "notify")]
pub(super) mod notify;
pub(super) mod remaps;
pub(super) mod screen;
pub(super) mod triggers;

/// True if `led_mode` is a host-driven reactive mode (music visualizer or screen
/// sync). These are entered temporarily; we remember the mode selected before
/// them so the keyboard can be returned to it on exit.
pub(in crate::tui) fn is_reactive(led_mode: u8) -> bool {
    audio::is_music_mode(led_mode) || screen::is_screen_mode(led_mode)
}
