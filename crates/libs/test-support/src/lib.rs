use std::path::Path;
use std::sync::OnceLock;
use std::sync::{Mutex, MutexGuard};

#[cfg(test)]
pub mod sdk_conformance;
#[cfg(test)]
pub mod sdk_matrix;
#[cfg(test)]
pub mod sdk_migration;
#[cfg(test)]
pub mod sdk_schema;

static LXMF_CONFIG_ROOT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn config_root_lock() -> &'static Mutex<()> {
    LXMF_CONFIG_ROOT_LOCK.get_or_init(|| Mutex::new(()))
}

pub struct ConfigRootGuard {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for ConfigRootGuard {
    fn drop(&mut self) {
        std::env::remove_var("LXMF_CONFIG_ROOT");
    }
}

pub fn lock_config_root(path: &Path) -> ConfigRootGuard {
    let _lock = config_root_lock().lock().expect("LXMF config root lock poisoned");
    std::env::set_var("LXMF_CONFIG_ROOT", path);
    ConfigRootGuard { _lock }
}
