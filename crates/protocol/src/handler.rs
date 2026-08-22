use std::{
    collections::VecDeque,
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    task::{Context as TaskContext, Poll},
};

use async_trait::async_trait;
use futures_util::Stream;
use thiserror::Error;
use tokio::task::{JoinError, JoinSet};

/// Determines whether a handler must finish before the next inbound item is
/// dispatched on the same connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {
    Ordered,
    Concurrent,
}

/// Handles one strongly typed inbound value.
#[async_trait]
pub trait Handler<Context>: Sized + Send + Sync + 'static
where
    Context: Send + 'static,
{
    type Inbound: Send + 'static;
    type Outbound: Send + 'static;
    type Error: Send + 'static;

    const MODE: DispatchMode;

    async fn handle(
        &self,
        context: Context,
        inbound: Self::Inbound,
    ) -> Result<Vec<Self::Outbound>, Self::Error>;
}

/// An object-safe handler used after a concrete handler type has been erased.
#[async_trait]
pub trait ErasedHandler<Context, Outbound, Error>: Send + 'static
where
    Context: Send + 'static,
    Outbound: Send + 'static,
    Error: Send + 'static,
{
    fn mode(&self) -> DispatchMode;

    async fn handle(self: Box<Self>, context: Context) -> Result<Vec<Outbound>, Error>;
}

/// An error returned by a handler or its concurrent task.
#[derive(Debug, Error)]
pub enum DispatchError<HandlerError> {
    #[error("handler error: {0}")]
    Handler(#[source] HandlerError),

    #[error("handler task error: {0}")]
    Task(#[source] JoinError),
}

/// Runs ordered handlers inline and bounds concurrent handler tasks.
///
/// Its [`Stream`] yields completed concurrent handlers and remains pending
/// while idle; dropping the dispatcher cancels any remaining tasks.
pub struct Dispatcher<Outbound, HandlerError> {
    max_concurrent: NonZeroUsize,
    tasks: JoinSet<Result<Vec<Outbound>, HandlerError>>,
    completed: VecDeque<Result<Vec<Outbound>, DispatchError<HandlerError>>>,
}

impl<Outbound, HandlerError> Unpin for Dispatcher<Outbound, HandlerError> {}

impl<Outbound, HandlerError> Dispatcher<Outbound, HandlerError>
where
    Outbound: Send + 'static,
    HandlerError: Send + 'static,
{
    pub fn new(max_concurrent: NonZeroUsize) -> Self {
        Self {
            max_concurrent,
            tasks: JoinSet::new(),
            completed: VecDeque::new(),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.tasks.is_empty() && self.completed.is_empty()
    }

    /// Dispatches one item according to its handler's scheduling mode.
    ///
    /// An ordered handler returns its outbound values immediately. A concurrent
    /// handler returns `None` and later yields its outbound values through the
    /// dispatcher's [`Stream`] implementation.
    ///
    /// If the concurrent task limit has been reached, this waits for one task
    /// and retains its result for the [`Stream`] implementation before queuing
    /// the new item.
    pub async fn dispatch<H, Context>(
        &mut self,
        handler: &'static H,
        context: Context,
        inbound: H::Inbound,
    ) -> Result<Option<Vec<Outbound>>, DispatchError<HandlerError>>
    where
        H: Handler<Context, Outbound = Outbound, Error = HandlerError>,
        Context: Send + 'static,
    {
        self.schedule(H::MODE, handler.handle(context, inbound))
            .await
    }

    /// Dispatches a handler whose concrete type was erased during decoding.
    pub async fn dispatch_erased<Context>(
        &mut self,
        handler: Box<dyn ErasedHandler<Context, Outbound, HandlerError>>,
        context: Context,
    ) -> Result<Option<Vec<Outbound>>, DispatchError<HandlerError>>
    where
        Context: Send + 'static,
    {
        let mode = handler.mode();
        self.schedule(mode, handler.handle(context)).await
    }

    async fn schedule<HandlerFuture>(
        &mut self,
        mode: DispatchMode,
        future: HandlerFuture,
    ) -> Result<Option<Vec<Outbound>>, DispatchError<HandlerError>>
    where
        HandlerFuture: Future<Output = Result<Vec<Outbound>, HandlerError>> + Send + 'static,
    {
        match mode {
            DispatchMode::Ordered => future.await.map(Some).map_err(DispatchError::Handler),
            DispatchMode::Concurrent => {
                if self.tasks.len() >= self.max_concurrent.get() {
                    let result = self
                        .tasks
                        .join_next()
                        .await
                        .expect("a task exists at the concurrency limit");
                    self.completed.push_back(map_task_result(result));
                }

                self.tasks.spawn(future);
                Ok(None)
            }
        }
    }
}

impl<Outbound, HandlerError> Stream for Dispatcher<Outbound, HandlerError>
where
    Outbound: Send + 'static,
    HandlerError: Send + 'static,
{
    type Item = Result<Vec<Outbound>, DispatchError<HandlerError>>;

    fn poll_next(self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if let Some(result) = this.completed.pop_front() {
            return Poll::Ready(Some(result));
        }

        if this.tasks.is_empty() {
            return Poll::Pending;
        }

        this.tasks
            .poll_join_next(context)
            .map(|result| result.map(|result| map_task_result(result)))
    }
}

fn map_task_result<Outbound, HandlerError>(
    result: Result<Result<Vec<Outbound>, HandlerError>, JoinError>,
) -> Result<Vec<Outbound>, DispatchError<HandlerError>> {
    match result {
        Ok(result) => result.map_err(DispatchError::Handler),
        Err(error) => Err(DispatchError::Task(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use futures_util::StreamExt;

    use super::{DispatchMode, Dispatcher, Handler};

    struct Number(u8);
    struct AddHandler {
        offset: u8,
    }
    static ADD_HANDLER: AddHandler = AddHandler { offset: 1 };

    #[async_trait::async_trait]
    impl Handler<u8> for AddHandler {
        type Inbound = Number;
        type Outbound = u8;
        type Error = std::convert::Infallible;

        const MODE: DispatchMode = DispatchMode::Ordered;

        async fn handle(
            &self,
            context: u8,
            inbound: Self::Inbound,
        ) -> Result<Vec<Self::Outbound>, Self::Error> {
            Ok(vec![inbound.0 + context + self.offset])
        }
    }

    #[tokio::test]
    async fn handles_items_with_owned_context() {
        assert_eq!(ADD_HANDLER.handle(3, Number(2)).await.unwrap(), [6]);
        assert_eq!(<AddHandler as Handler<u8>>::MODE, DispatchMode::Ordered);
    }

    struct MultiplyHandler;
    static MULTIPLY_HANDLER: MultiplyHandler = MultiplyHandler;

    #[async_trait::async_trait]
    impl Handler<u8> for MultiplyHandler {
        type Inbound = Number;
        type Outbound = u8;
        type Error = std::convert::Infallible;

        const MODE: DispatchMode = DispatchMode::Concurrent;

        async fn handle(
            &self,
            context: u8,
            inbound: Self::Inbound,
        ) -> Result<Vec<Self::Outbound>, Self::Error> {
            Ok(vec![inbound.0 * context])
        }
    }

    #[tokio::test]
    async fn dispatches_ordered_and_concurrent_handlers() {
        let mut dispatcher = Dispatcher::new(NonZeroUsize::new(1).unwrap());

        assert_eq!(
            dispatcher
                .dispatch(&ADD_HANDLER, 3, Number(2))
                .await
                .unwrap(),
            Some(vec![6])
        );
        assert_eq!(
            dispatcher
                .dispatch(&MULTIPLY_HANDLER, 4, Number(2))
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            dispatcher
                .dispatch(&MULTIPLY_HANDLER, 4, Number(3))
                .await
                .unwrap(),
            None
        );
        assert_eq!(dispatcher.next().await.unwrap().unwrap(), [8]);
        assert_eq!(dispatcher.next().await.unwrap().unwrap(), [12]);
        assert!(dispatcher.is_idle());
    }
}
