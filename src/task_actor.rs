use std::time::Duration;

use anyhow::Result;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_util::{sync::CancellationToken, time::DelayQueue};

use crate::task_group::TaskGroup;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub enum TaskActorTimer<Timer> {
    Timer(Timer),
    ShutdownTimeout,
}

#[derive(Debug)]
pub enum TaskActorEvent<Message, Timer> {
    Message(Message),
    Timer(Timer),
    Stopped(TaskActorStopReason),
}

#[derive(Debug)]
pub enum TaskActorStopReason {
    Closed,
    Aborted,
    Cancelled,
}

pub struct TaskActor<Message, Timer, Return> {
    shutdown: bool,
    cancel_token: CancellationToken,
    receiver: mpsc::Receiver<Message>,
    timers: DelayQueue<TaskActorTimer<Timer>>,
    tasks: TaskGroup<Return>,
}

impl<Message, Timer, Return> TaskActor<Message, Timer, Return> {
    pub fn new(cancel_token: CancellationToken, receiver: mpsc::Receiver<Message>) -> Self {
        let tasks_cancel_token = CancellationToken::new();
        Self {
            shutdown: false,
            cancel_token,
            receiver,
            timers: DelayQueue::new(),
            tasks: TaskGroup::new(tasks_cancel_token),
        }
    }

    pub async fn update(&mut self) -> Result<TaskActorEvent<Message, Timer>> {
        while self.is_running() {
            tokio::select! {
                message = self.receiver.recv() => {
                    println!("task actor received message");

                    match message {
                        Some(message) => {
                            return Ok(TaskActorEvent::Message(message));
                        }
                        None => {
                            self.shutdown().await;
                            let reason = TaskActorStopReason::Closed;
                            return Ok(TaskActorEvent::Stopped(reason));
                        }
                    }
                }
                timer = self.timers.next() => {
                    if let Some(timer) = timer {
                        match timer.into_inner() {
                            TaskActorTimer::Timer(timer) => {
                                return Ok(TaskActorEvent::Timer(timer));
                            }
                            TaskActorTimer::ShutdownTimeout => {
                                self.tasks.abort_all().await;
                                let reason = TaskActorStopReason::Aborted;
                                return Ok(TaskActorEvent::Stopped(reason));
                            }
                        }
                    }
                }
                _ = self.cancel_token.cancelled() => {
                    self.shutdown().await;
                    let reason = TaskActorStopReason::Cancelled;
                    return Ok(TaskActorEvent::Stopped(reason));
                }
            }
        }

        println!("!!!!! task actor stopped");

        Ok(TaskActorEvent::Stopped(TaskActorStopReason::Closed))
    }

    pub fn is_running(&self) -> bool {
        !(self.shutdown
            && self.receiver.is_closed()
            && self.receiver.is_empty()
            && self.timers.is_empty())
    }

    pub async fn shutdown(&mut self) {
        if self.shutdown {
            return;
        }

        let timer = TaskActorTimer::ShutdownTimeout;

        self.shutdown = true;
        self.receiver.close();
        self.timers.insert(timer, SHUTDOWN_TIMEOUT);
        self.tasks.cancel().await;
        self.tasks.wait().await;
    }

    pub fn receiver(&self) -> &mpsc::Receiver<Message> {
        &self.receiver
    }

    pub fn insert_timer(
        &mut self,
        value: Timer,
        timeout: Duration,
    ) -> tokio_util::time::delay_queue::Key {
        let value = TaskActorTimer::Timer(value);
        self.timers.insert(value, timeout)
    }

    pub fn remove_timer(&mut self, key: tokio_util::time::delay_queue::Key) {
        self.timers.remove(&key);
    }

    pub fn tasks(&mut self) -> &mut TaskGroup<Return> {
        &mut self.tasks
    }
}
