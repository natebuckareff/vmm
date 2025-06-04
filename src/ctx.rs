use crate::{logger::Logger, vmm_dirs::VmmDirs};

pub trait HasDirs {
    fn dirs(&self) -> &VmmDirs;
}

pub trait HasLogger {
    fn logger(&self) -> &Logger;
}
