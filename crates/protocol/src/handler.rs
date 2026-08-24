use std::{future::Future, num::NonZeroUsize, pin::Pin};

use thiserror::Error;
use tokio::task::{JoinError, JoinSet};

/// Determines whether a handler must finish before the next inbound item is
/// dispatched on the same connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {
    Ordered,
    Concurrent,
}

/// Handles one strongly typed inbound value using an explicit outbound channel.
pub trait Handler<Context, Sender>: Sized + Send + Sync + 'static
where
    Context: Send + 'static,
    Sender: Send + 'static,
{
    type Inbound: Send + 'static;
    type Error: Send + 'static;

    const MODE: DispatchMode = DispatchMode::Ordered;

    fn handle(
        &self,
        context: Context,
        inbound: Self::Inbound,
        outbound: Sender,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// An object-safe handler used after a concrete handler type has been erased.
pub trait ErasedHandler<Context, Sender, Error>: Send + 'static
where
    Context: Send + 'static,
    Sender: Send + 'static,
    Error: Send + 'static,
{
    fn mode(&self) -> DispatchMode;

    fn handle(
        self: Box<Self>,
        context: Context,
        outbound: Sender,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'static>>;
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
/// Dropping the dispatcher cancels any remaining tasks.
pub struct Dispatcher<HandlerError> {
    max_concurrent: NonZeroUsize,
    tasks: JoinSet<Result<(), HandlerError>>,
}

impl<HandlerError> Dispatcher<HandlerError>
where
    HandlerError: Send + 'static,
{
    pub fn new(max_concurrent: NonZeroUsize) -> Self {
        Self {
            max_concurrent,
            tasks: JoinSet::new(),
        }
    }

    /// Dispatches a decoded handler according to its scheduling mode.
    ///
    /// If the concurrent task limit has been reached, this waits for one task
    /// before queuing the new item.
    pub async fn dispatch<Context, Sender>(
        &mut self,
        handler: Box<dyn ErasedHandler<Context, Sender, HandlerError>>,
        context: Context,
        outbound: Sender,
    ) -> Result<(), DispatchError<HandlerError>>
    where
        Context: Send + 'static,
        Sender: Send + 'static,
    {
        let mode = handler.mode();
        self.schedule(mode, handler.handle(context, outbound)).await
    }

    async fn schedule<HandlerFuture>(
        &mut self,
        mode: DispatchMode,
        future: HandlerFuture,
    ) -> Result<(), DispatchError<HandlerError>>
    where
        HandlerFuture: Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        match mode {
            DispatchMode::Ordered => future.await.map_err(DispatchError::Handler),
            DispatchMode::Concurrent => {
                if self.tasks.len() >= self.max_concurrent.get() {
                    map_task_result(
                        self.tasks
                            .join_next()
                            .await
                            .expect("a task exists at the concurrency limit"),
                    )?;
                }

                self.tasks.spawn(future);
                Ok(())
            }
        }
    }

    /// Waits for all concurrent handlers to finish.
    pub async fn finish(&mut self) -> Result<(), DispatchError<HandlerError>> {
        while let Some(result) = self.tasks.join_next().await {
            map_task_result(result)?;
        }

        Ok(())
    }
}

fn map_task_result<HandlerError>(
    result: Result<Result<(), HandlerError>, JoinError>,
) -> Result<(), DispatchError<HandlerError>> {
    match result {
        Ok(result) => result.map_err(DispatchError::Handler),
        Err(error) => Err(DispatchError::Task(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    use super::{DispatchMode, Dispatcher, Handler};

    struct Number(u8);
    struct AddHandler {
        offset: u8,
    }
    static ADD_HANDLER: AddHandler = AddHandler { offset: 1 };

    impl Handler<u8, UnboundedSender<u8>> for AddHandler {
        type Inbound = Number;
        type Error = std::convert::Infallible;

        async fn handle(
            &self,
            context: u8,
            inbound: Self::Inbound,
            outbound: UnboundedSender<u8>,
        ) -> Result<(), Self::Error> {
            outbound.send(inbound.0 + context + self.offset).unwrap();
            Ok(())
        }
    }

    #[tokio::test]
    async fn handles_items_with_owned_context() {
        let (sender, mut receiver) = unbounded_channel();

        ADD_HANDLER.handle(3, Number(2), sender).await.unwrap();

        assert_eq!(receiver.recv().await, Some(6));
        assert_eq!(
            <AddHandler as Handler<u8, UnboundedSender<u8>>>::MODE,
            DispatchMode::Ordered
        );
    }

    struct MultiplyHandler;
    static MULTIPLY_HANDLER: MultiplyHandler = MultiplyHandler;

    impl Handler<u8, UnboundedSender<u8>> for MultiplyHandler {
        type Inbound = Number;
        type Error = std::convert::Infallible;

        const MODE: DispatchMode = DispatchMode::Concurrent;

        async fn handle(
            &self,
            context: u8,
            inbound: Self::Inbound,
            outbound: UnboundedSender<u8>,
        ) -> Result<(), Self::Error> {
            outbound.send(inbound.0 * context).unwrap();
            Ok(())
        }
    }

    #[tokio::test]
    async fn dispatches_ordered_and_concurrent_handlers() {
        let mut dispatcher = Dispatcher::new(NonZeroUsize::new(1).unwrap());
        let (sender, mut receiver) = unbounded_channel();

        dispatcher
            .schedule(
                DispatchMode::Ordered,
                ADD_HANDLER.handle(3, Number(2), sender.clone()),
            )
            .await
            .unwrap();
        dispatcher
            .schedule(
                DispatchMode::Concurrent,
                MULTIPLY_HANDLER.handle(4, Number(2), sender.clone()),
            )
            .await
            .unwrap();
        dispatcher
            .schedule(
                DispatchMode::Concurrent,
                MULTIPLY_HANDLER.handle(4, Number(3), sender),
            )
            .await
            .unwrap();

        dispatcher.finish().await.unwrap();

        assert_eq!(receiver.recv().await, Some(6));
        assert_eq!(receiver.recv().await, Some(8));
        assert_eq!(receiver.recv().await, Some(12));
        assert_eq!(receiver.recv().await, None);
    }
}
