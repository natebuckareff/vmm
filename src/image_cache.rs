use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, oneshot},
};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    ctx::Ctx,
    progress_router::ProgressMessage,
    task_actor::{TaskActor, TaskActorEvent},
    task_group::{TaskGroup, TaskId},
};

pub fn create_image_cache(ctx: Ctx, task_group: &mut TaskGroup<Result<()>>) -> ImageCacheClient {
    let (sender, receiver) = mpsc::channel(100);
    let image_cache = ImageCache::new(
        ctx.clone(),
        sender.clone(),
        receiver,
        ctx.cancel_token().clone(),
    );

    task_group.spawn(async move {
        image_cache.run().await?;
        Ok(())
    });

    ImageCacheClient::new(sender)
}

pub type ImageHash = String;

#[derive(Debug)]
pub enum ImageCacheMessage {
    GetImageHash {
        url: Url,
        expected_hash: Option<ImageHash>,
        response: oneshot::Sender<GetImageHashResult>,
    },
    GetImageHashResult(Url, GetImageHashResult),
}

#[derive(Debug, Clone)]
pub enum GetImageHashResult {
    ImageCached(ImageHash),
    DownloadNoContentLength,
    DownloadFailed(reqwest::StatusCode),
    DownloadFailedToReadChunk,
    DownloadCancelled,
    UnknownError,
}

// TODO: move
enum Either<T, U> {
    Left(T),
    Right(U),
}

#[derive(Clone)]
pub struct ImageCacheClient {
    sender: mpsc::Sender<ImageCacheMessage>,
}

impl ImageCacheClient {
    pub fn new(sender: mpsc::Sender<ImageCacheMessage>) -> Self {
        Self { sender }
    }

    pub async fn get_image_hash(
        &self,
        ctx: &Ctx,
        url: Url,
        expected_hash: Option<ImageHash>,
    ) -> Result<GetImageHashResult> {
        if let Some(expected_hash) = &expected_hash {
            let image_cache_path = ctx.dirs().get_image_cache_path(expected_hash)?;
            if image_cache_path.exists() {
                return Ok(GetImageHashResult::ImageCached(expected_hash.clone()));
            }
        }

        let (response_sender, response_receiver) = oneshot::channel();

        let message = ImageCacheMessage::GetImageHash {
            url,
            expected_hash,
            response: response_sender,
        };

        self.sender.send(message).await?;

        Ok(response_receiver.await?)
    }
}

struct Download {
    id: u64,
    subscribers: Vec<Subscriber>,
    timer_key: tokio_util::time::delay_queue::Key,
    hash: Option<ImageHash>,
}

struct Subscriber {
    expected_hash: Option<ImageHash>,
    response: Option<oneshot::Sender<GetImageHashResult>>,
}

#[derive(Debug)]
enum Timer {
    DownloadTimeout(TaskId, Url),
    UrlHashExpired(Url),
}

pub struct ImageCache {
    ctx: Ctx,
    sender: mpsc::Sender<ImageCacheMessage>,
    cancel_token: CancellationToken,
    downloads: HashMap<Url, Download>,
    next_download_id: u64,
    task_actor: TaskActor<ImageCacheMessage, Timer, ()>,
}

impl ImageCache {
    pub fn new(
        ctx: Ctx,
        sender: mpsc::Sender<ImageCacheMessage>,
        receiver: mpsc::Receiver<ImageCacheMessage>,
        cancel_token: CancellationToken,
    ) -> Self {
        let tasks_cancel_token = cancel_token.clone();
        Self {
            ctx,
            sender,
            cancel_token,
            downloads: HashMap::new(),
            next_download_id: 0,
            task_actor: TaskActor::new(tasks_cancel_token, receiver),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        loop {
            println!("task_actor.update()");
            match self.task_actor.update().await? {
                TaskActorEvent::Message(message) => {
                    println!("handle_message");
                    dbg!(&message);
                    self.handle_message(message).await?;
                }
                TaskActorEvent::Timer(timer) => {
                    println!("handle_timer");
                    dbg!(&timer);
                    self.handle_timer(timer).await;
                }
                TaskActorEvent::Stopped(reason) => {
                    println!("handle_stopped: {:?}", reason);
                    break;
                }
            }
        }
        println!("exiting");
        Ok(())
    }

    async fn handle_message(&mut self, message: ImageCacheMessage) -> Result<()> {
        match message {
            ImageCacheMessage::GetImageHash {
                url,
                expected_hash,
                response,
            } => {
                self.handle_get_image_hash(url, expected_hash, response)
                    .await?;
            }
            ImageCacheMessage::GetImageHashResult(download_id, result) => {
                if let Some(download) = self.downloads.get_mut(&download_id) {
                    self.task_actor.remove_timer(download.timer_key);

                    for mut subscriber in download.subscribers.drain(..) {
                        if let Some(response) = subscriber.response.take() {
                            let _ = response.send(result.clone());
                        }
                    }

                    if let GetImageHashResult::ImageCached(hash) = result {
                        download.hash = Some(hash);
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_get_image_hash(
        &mut self,
        url: Url,
        expected_hash: Option<ImageHash>,
        response: oneshot::Sender<GetImageHashResult>,
    ) -> Result<()> {
        // Check if there are any already in-progress downloads
        if let Some(download) = self.downloads.get_mut(&url) {
            match &download.hash {
                Some(hash) => {
                    // If there is already a __finished download__ for the
                    // requested url
                    if let Some(expected_hash) = &expected_hash {
                        if expected_hash == hash {
                            // If the hashes match, return the cached hash back
                            // to the caller
                            let _ = response.send(GetImageHashResult::ImageCached(hash.clone()));
                            return Ok(());
                        } else {
                            // If the hashes don't match, invalidate the
                            // download and continue to start a new one
                            self.downloads.remove(&url);
                        }
                    }
                }
                None => {
                    // If there is already an __in-progress download__ for the
                    // requested url, add the caller as a subscriber
                    download.subscribers.push(Subscriber {
                        expected_hash,
                        response: Some(response),
                    });
                    return Ok(());
                }
            }
        }

        // Start new download

        let download_id = self.next_download_id;
        self.next_download_id += 1;

        let url2 = url.clone();
        let ctx = self.ctx.clone();
        let cancel_token = self.cancel_token.clone();
        let sender = self.sender.clone();

        let task_id = self.task_actor.tasks().spawn(async move {
            tokio::select! {
                result = get_image_hash(&ctx, download_id, url2.clone()) => {
                    match result {
                        Ok(result) => {
                            let msg = ImageCacheMessage::GetImageHashResult(url2.clone(), result);
                            println!("sending msg: {:?}", &msg);
                            let _ = sender.send(msg).await;
                        }
                        Err(e) => {
                            // TODO: some kind of error to correlate
                            eprintln!("error: {:?}", e);
                            let msg = ImageCacheMessage::GetImageHashResult(url2.clone(), GetImageHashResult::UnknownError);
                            let _ = sender.send(msg).await;
                        }
                    }
                }
                _ = cancel_token.cancelled() => {
                    let msg = ImageCacheMessage::GetImageHashResult(url2.clone(), GetImageHashResult::DownloadCancelled);
                    let _ = sender.send(msg).await;
                }
            };
        });

        let timer_key = self.task_actor.insert_timer(
            Timer::DownloadTimeout(task_id, url.clone()),
            Duration::from_secs(60),
        );

        let download = Download {
            id: download_id,
            subscribers: vec![Subscriber {
                expected_hash,
                response: Some(response),
            }],
            timer_key,
            hash: None,
        };

        self.downloads.insert(url.clone(), download);

        Ok(())
    }

    async fn handle_timer(&mut self, timer: Timer) {
        match timer {
            Timer::DownloadTimeout(task_id, url) => {
                self.task_actor.tasks().abort_task(task_id).await;
                self.downloads.remove(&url);
            }
            Timer::UrlHashExpired(url) => {
                self.downloads.remove(&url);
            }
        }
    }
}

async fn get_image_hash(ctx: &Ctx, download_id: u64, url: Url) -> Result<GetImageHashResult> {
    let client = reqwest::Client::new();
    let response = client
        .get(url.clone())
        .send()
        .await
        .context("failed to download image")
        .context(url.clone())?;

    let Some(content_length) = response.content_length() else {
        return Ok(GetImageHashResult::DownloadFailed(response.status()));
    };

    let status = response.status();
    if !status.is_success() {
        return Ok(GetImageHashResult::DownloadFailed(status));
    }

    let download_image_path = ctx.dirs().get_image_download_path(download_id)?;

    tokio::fs::create_dir_all(
        download_image_path
            .parent()
            .ok_or(anyhow!("invalid path"))?,
    )
    .await?;

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(download_image_path.clone())
        .await
        .context("failed to open image download file")
        .context(download_id)?;

    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();

    let progress_id = format!("download/{}", download_id);

    ctx.progress_router()
        .send(ProgressMessage::Start(
            progress_id.clone(),
            Some(content_length),
        ))
        .await;

    while let Some(chunk_result) = stream.next().await {
        let Some(chunk) = chunk_result.ok() else {
            return Ok(GetImageHashResult::DownloadFailedToReadChunk);
        };

        ctx.progress_router()
            .send(ProgressMessage::Update(
                progress_id.clone(),
                chunk.len() as u64,
            ))
            .await;

        hasher.update(&chunk);

        file.write_all(&chunk)
            .await
            .context("failed to write chunk to file")?;
    }

    ctx.progress_router()
        .send(ProgressMessage::Finish(progress_id))
        .await;

    let hash = hasher.finalize();
    let hash = format!("{:x}", hash);

    let image_cache_path = ctx.dirs().get_image_cache_path(&hash)?;

    tokio::fs::create_dir_all(image_cache_path.parent().ok_or(anyhow!("invalid path"))?).await?;

    tokio::fs::rename(download_image_path, image_cache_path).await?;

    return Ok(GetImageHashResult::ImageCached(hash));
}
