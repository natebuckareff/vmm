use std::sync::Arc;

use dashmap::DashMap;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

pub struct TaskGroup<T> {
    cancel_token: CancellationToken,
    tasks: Arc<DashMap<TaskId, JoinHandle<TaskResult<T>>>>,
    next_task_id: TaskId,
}

pub enum TaskResult<T> {
    Completed(T),
    Cancelled,
}

impl<T> TaskGroup<T> {
    pub fn new(cancel_token: CancellationToken) -> Self {
        Self {
            cancel_token,
            tasks: Arc::new(DashMap::new()),
            next_task_id: TaskId(0),
        }
    }

    pub fn spawn<F>(&mut self, future: F) -> TaskId
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        let task_id = self.next_task_id;
        let cancel_token = self.cancel_token.clone();
        let tasks = self.tasks.clone();

        let task_handle = tokio::spawn(async move {
            let result = tokio::select! {
                result = future => TaskResult::Completed(result),
                _ = cancel_token.cancelled() => TaskResult::Cancelled
            };
            tasks.remove(&task_id);
            result
        });

        self.tasks.insert(task_id, task_handle);
        self.next_task_id = TaskId(self.next_task_id.0 + 1);

        task_id
    }

    pub async fn wait(&mut self) {
        let mut waited = vec![];

        for entry in self.tasks.iter() {
            waited.push(*entry.key());
        }

        for id in waited {
            if let Some((_, handle)) = self.tasks.remove(&id) {
                handle.await;
            }
        }
    }

    pub async fn cancel(&mut self) {
        self.cancel_token.cancel();
    }

    pub async fn abort_all(&mut self) {
        let mut aborted = vec![];

        for entry in self.tasks.iter() {
            let (id, handle) = entry.pair();
            handle.abort();
            aborted.push(*id);
        }

        for id in aborted {
            self.tasks.remove(&id);
        }
    }

    pub async fn abort_task(&mut self, task_id: TaskId) -> bool {
        if let Some((_, handle)) = self.tasks.remove(&task_id) {
            handle.abort();
            let _ = handle.await;
            true
        } else {
            false
        }
    }
}
