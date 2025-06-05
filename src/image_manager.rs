use std::collections::HashMap;

use anyhow::{Context, Result};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use url::Url;

use crate::ctx::HasDirs;

pub type ImageHash = String;

pub enum ImageManagerMessage {
    GetImage((Url, Option<String>, oneshot::Sender<GetImageResult>)),
    DownloadCompleted(u64, GetImageResult),
}

#[derive(Clone)]
pub enum GetImageResult {
    ImageCached(ImageHash),
    InconsistentHash(ImageHash),
    DownloadFailed(reqwest::StatusCode),
    DownloadFailedToReadChunk,
}

#[derive(Clone)]
pub struct ImageManagerClient {
    sender: mpsc::Sender<ImageManagerMessage>,
}

impl ImageManagerClient {
    pub fn new(sender: mpsc::Sender<ImageManagerMessage>) -> Self {
        Self { sender }
    }

    pub async fn get_image(
        &self,
        url: Url,
        expected_hash: Option<String>,
    ) -> Result<GetImageResult> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(ImageManagerMessage::GetImage((url, expected_hash, sender)))
            .await?;
        Ok(receiver.await?)
    }
}

struct Download {
    url: Url,
    handle: JoinHandle<Result<()>>,
    senders: Vec<oneshot::Sender<GetImageResult>>,
}

pub struct ImageManager<Ctx: HasDirs> {
    ctx: Ctx,
    sender: mpsc::Sender<ImageManagerMessage>,
    receiver: mpsc::Receiver<ImageManagerMessage>,
    next_download_id: u64,
    url_images: HashMap<Url, ImageHash>,
    in_progress: HashMap<Url, u64>,
    downloads: HashMap<u64, Download>,
}

impl<Ctx: HasDirs> ImageManager<Ctx> {
    pub fn new(
        ctx: Ctx,
        sender: mpsc::Sender<ImageManagerMessage>,
        receiver: mpsc::Receiver<ImageManagerMessage>,
    ) -> Self {
        Self {
            ctx,
            sender,
            receiver,
            next_download_id: 0,
            url_images: HashMap::new(),
            in_progress: HashMap::new(),
            downloads: HashMap::new(),
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        while let Some(message) = self.receiver.recv().await {
            match message {
                ImageManagerMessage::GetImage(msg) => {
                    let (url, expected_hash, sender) = msg;
                    self.get_image(url, expected_hash, sender).await?;
                }
                ImageManagerMessage::DownloadCompleted(download_id, hash) => {
                    self.complete_download(download_id, hash).await?;
                }
            }
        }
        Ok(())
    }

    async fn get_image(
        &mut self,
        url: Url,
        expected_hash: Option<String>,
        sender: oneshot::Sender<GetImageResult>,
    ) -> Result<()> {
        if let Some(image_hash) = self.url_images.get(&url) {
            if let Some(expected_hash) = expected_hash {
                if expected_hash != *image_hash {
                    let _ = sender.send(GetImageResult::InconsistentHash(image_hash.clone()));
                    return Ok(());
                }
            }

            let _ = sender.send(GetImageResult::ImageCached(image_hash.clone()));
            return Ok(());
        }

        if let Some(download_id) = self.in_progress.get(&url) {
            let download_id = *download_id;
            let senders = self.downloads.get_mut(&download_id);

            match senders {
                Some(download) => {
                    download.senders.push(sender);
                    return Ok(());
                }
                None => {
                    self.in_progress.remove(&url);
                }
            }
        }

        self.start_download(url, expected_hash, sender)?;

        Ok(())
    }

    fn start_download(
        &mut self,
        url: Url,
        expected_hash: Option<String>,
        sender: oneshot::Sender<GetImageResult>,
    ) -> Result<()> {
        let download_id = self.next_download_id;

        self.next_download_id += 1;
        self.in_progress.insert(url.clone(), download_id);

        let download_image_path = self.ctx.dirs().get_image_download_path(download_id)?;
        let dirs = self.ctx.dirs().clone();

        let self_sender = self.sender.clone();

        let url_clone = url.clone();

        let download_task = tokio::spawn(async move {
            let url = url_clone;

            let client = reqwest::Client::new();
            let response = client
                .get(url.clone())
                .send()
                .await
                .context("failed to download image")
                .context(download_id)?;

            let status = response.status();
            if !status.is_success() {
                let _ = self_sender
                    .send(ImageManagerMessage::DownloadCompleted(
                        download_id,
                        GetImageResult::DownloadFailed(status),
                    ))
                    .await?;

                return Ok(());
            }

            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(download_image_path.clone())
                .await
                .context("failed to open image download file")
                .context(download_id)?;

            let mut hasher = Sha256::new();
            let mut stream = response.bytes_stream();

            while let Some(chunk_result) = stream.next().await {
                let Some(chunk) = chunk_result.ok() else {
                    let _ = self_sender
                        .send(ImageManagerMessage::DownloadCompleted(
                            download_id,
                            GetImageResult::DownloadFailedToReadChunk,
                        ))
                        .await?;

                    return Ok(());
                };

                hasher.update(&chunk);

                file.write_all(&chunk)
                    .await
                    .context("failed to write chunk to file")?;
            }

            let hash = hasher.finalize();
            let hash = format!("{:x}", hash);

            if let Some(expected_hash) = expected_hash {
                if expected_hash != hash {
                    let _ = self_sender
                        .send(ImageManagerMessage::DownloadCompleted(
                            download_id,
                            GetImageResult::InconsistentHash(hash),
                        ))
                        .await?;

                    return Ok(());
                }
            }

            let image_path = dirs.get_image_cache_path(&hash)?;

            tokio::fs::rename(download_image_path, &image_path).await?;

            self_sender
                .send(ImageManagerMessage::DownloadCompleted(
                    download_id,
                    GetImageResult::ImageCached(hash),
                ))
                .await?;

            Ok(())
        });

        self.downloads.insert(
            download_id,
            Download {
                url,
                handle: download_task,
                senders: vec![sender],
            },
        );

        Ok(())
    }

    async fn complete_download(&mut self, download_id: u64, result: GetImageResult) -> Result<()> {
        let download = self.downloads.remove(&download_id).unwrap();

        for sender in download.senders {
            let _ = sender.send(result.clone());
        }

        let _ = download.handle.await?;

        self.in_progress.remove(&download.url);
        self.downloads.remove(&download_id);

        Ok(())
    }
}
