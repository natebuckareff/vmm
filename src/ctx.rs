use std::cell::OnceCell;

use crate::{logger::Logger, vmm_dirs::VmmDirs};

pub trait HasDirs {
    fn dirs(&self) -> &VmmDirs;
}

pub trait HasLogger {
    fn logger(&self) -> &Logger;
}

pub struct Ctx {
    dirs: OnceCell<VmmDirs>,
    logger: OnceCell<Logger>,
}

impl Ctx {
    pub fn new() -> Self {
        Self {
            dirs: OnceCell::new(),
            logger: OnceCell::new(),
        }
    }
}

impl HasDirs for Ctx {
    fn dirs(&self) -> &VmmDirs {
        self.dirs
            .get_or_init(|| VmmDirs::new().expect("failed to initialize vmm dirs"))
    }
}

impl HasLogger for Ctx {
    fn logger(&self) -> &Logger {
        self.logger.get_or_init(|| {
            let dirs = self.dirs().clone();
            Logger::new(dirs)
        })
    }
}
