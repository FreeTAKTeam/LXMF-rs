include!("mod_parts/module_prelude.rs");

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
mod native;

#[cfg(target_os = "windows")]
mod windows;

include!("mod_parts/ble_startup_max_retry_attempts.rs");

include!("mod_parts/plannedfailure.rs");
