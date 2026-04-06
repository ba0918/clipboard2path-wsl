//! Domain layer: pure business logic with no I/O dependencies.

pub mod cleanup;
pub mod cli;
pub mod clipboard_change;
pub mod image_convert;
pub mod path_gen;
pub mod path_validate;
pub mod runtime_dir;
pub mod shell_detect;
pub mod shell_hook;
pub mod systemd_unit;
pub mod wl_paste_wrapper;
pub mod wsl_detect;
