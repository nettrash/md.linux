//! A future polled by hand — md.win's `Task` whose `IsCompleted` a test checks after advancing a
//! fake scheduler. The fakes complete synchronously or through a timer the test fires, so polling
//! with a no-op waker after each step is exactly "the continuation ran inline".

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

pub struct Task<'a, T> {
    future: Option<Pin<Box<dyn Future<Output = T> + 'a>>>,
    output: Option<T>,
}

impl<'a, T> Task<'a, T> {
    /// Starts the future: it is polled once now, as calling an `async` C# method runs it to its
    /// first await.
    pub fn start(future: impl Future<Output = T> + 'a) -> Self {
        let mut task = Task { future: Some(Box::pin(future)), output: None };
        task.poll();
        task
    }

    /// Polls once more (when not finished) and says whether it has finished.
    pub fn poll(&mut self) -> bool {
        if let Some(future) = self.future.as_mut() {
            let mut context = Context::from_waker(Waker::noop());
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                self.output = Some(output);
                self.future = None;
            }
        }
        self.future.is_none()
    }

    /// Whether it had finished at its last poll, without polling.
    pub fn is_completed(&self) -> bool {
        self.future.is_none()
    }

    /// The output.
    ///
    /// # Panics
    ///
    /// When it has not finished.
    pub fn output(self) -> T {
        self.output.expect("the task has not finished")
    }
}
