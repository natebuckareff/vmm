use tokio_util::sync::CancellationToken;

use crate::{
    image_cache::ImageCacheClient, logger::Logger, progress_router::ProgressRouterClient,
    vmm_dirs::VmmDirs,
};

#[derive(Clone)]
pub struct Ctx {
    cancel_token: CancellationToken,
    dirs: VmmDirs,
    logger: Logger,
    image_manager: Option<ImageCacheClient>,
    progress_router: Option<ProgressRouterClient>,
}

impl Ctx {
    pub fn new() -> Self {
        let dirs = VmmDirs::new().expect("failed to initialize vmm dirs");
        Self {
            cancel_token: CancellationToken::new(),
            dirs: dirs.clone(),
            logger: Logger::new(dirs),
            image_manager: None,
            progress_router: None,
        }
    }

    pub fn with_image_manager(self, image_manager: ImageCacheClient) -> Self {
        Self {
            image_manager: Some(image_manager),
            ..self
        }
    }

    pub fn with_progress_router(self, progress_router: ProgressRouterClient) -> Self {
        Self {
            progress_router: Some(progress_router),
            ..self
        }
    }

    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel_token
    }

    pub fn dirs(&self) -> &VmmDirs {
        &self.dirs
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn image_manager(&self) -> &ImageCacheClient {
        self.image_manager
            .as_ref()
            .expect("image_mananger not set on context")
    }

    pub fn progress_router(&self) -> &ProgressRouterClient {
        self.progress_router
            .as_ref()
            .expect("progress_tracker not set on context")
    }
}
