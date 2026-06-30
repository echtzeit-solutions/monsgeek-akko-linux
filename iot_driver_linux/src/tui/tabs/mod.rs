// Tab implementations — each tab's types, rendering, and input handling

pub(super) mod audio;
pub(super) mod depth;
pub(super) mod device_info;
#[cfg(feature = "notify")]
pub(super) mod notify;
pub(super) mod remaps;
pub(super) mod screen;
pub(super) mod triggers;
