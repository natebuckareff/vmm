use std::cell::OnceCell;

use crate::{image_manager::ImageManagerClient, logger::Logger, vmm_dirs::VmmDirs};

pub trait HasDirs {
    fn dirs(&self) -> &VmmDirs;
}

pub trait HasLogger {
    fn logger(&self) -> &Logger;
}

pub trait HasImageManager {
    fn image_manager(&self) -> &ImageManagerClient;
}

#[derive(Clone)]
pub struct Ctx {
    dirs: OnceCell<VmmDirs>,
    logger: OnceCell<Logger>,
    image_manager: Option<ImageManagerClient>,
}

impl Ctx {
    pub fn new(image_manager: Option<ImageManagerClient>) -> Self {
        Self {
            dirs: OnceCell::new(),
            logger: OnceCell::new(),
            image_manager,
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

impl HasImageManager for Ctx {
    fn image_manager(&self) -> &ImageManagerClient {
        self.image_manager
            .as_ref()
            .expect("image_mananger not set on context")
    }
}
