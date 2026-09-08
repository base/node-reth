//! The [`EngineActor`].

use async_trait::async_trait;
use derive_more::Constructor;
use tokio::sync::mpsc;
use tokio_util::{
    sync::{CancellationToken, WaitForCancellationFuture},
    task::AbortOnDropHandle,
};

use crate::{
    EngineActorRequest, EngineError, EngineRequestReceiver, NodeActor, actors::CancellableContext,
};

/// Supervises serialized engine processing on the inbound request queue.
/// No relay queue sits between producers and the processor's bounded receiver.
#[derive(Constructor, Debug)]
pub struct EngineActor<EngineRequestReceiver_>
where
    EngineRequestReceiver_: EngineRequestReceiver,
{
    /// The cancellation token shared by all tasks.
    cancellation_token: CancellationToken,
    /// The inbound request channel.
    inbound_request_rx: mpsc::Receiver<EngineActorRequest>,
    /// The processor for engine requests
    engine_receiver: EngineRequestReceiver_,
}

impl<EngineRequestReceiver_> CancellableContext for EngineActor<EngineRequestReceiver_>
where
    EngineRequestReceiver_: EngineRequestReceiver,
{
    fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.cancellation_token.cancelled()
    }
}

#[async_trait]
impl<EngineRequestReceiver_> NodeActor for EngineActor<EngineRequestReceiver_>
where
    EngineRequestReceiver_: EngineRequestReceiver + 'static,
{
    type Error = EngineError;
    type StartData = ();

    async fn start(self, _: Self::StartData) -> Result<(), Self::Error> {
        // Aborting the supervisor must not detach engine work either.
        let mut processing =
            AbortOnDropHandle::new(self.engine_receiver.start(self.inbound_request_rx));
        let result = tokio::select! {
            biased;
            _ = self.cancellation_token.cancelled() => {
                processing.abort();
                let _ = processing.await;
                return Ok(());
            }
            result = &mut processing => result,
        };
        self.cancellation_token.cancel();
        result.map_err(|error| {
            error!(target: "engine", ?error, "Engine processing task failed");
            EngineError::ChannelClosed
        })?
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::task::JoinHandle;

    use super::*;

    mockall::mock! {
        pub Receiver {}
        impl EngineRequestReceiver for Receiver {
            fn start(self, requests: mpsc::Receiver<EngineActorRequest>) -> JoinHandle<Result<(), EngineError>>;
        }
    }

    #[tokio::test]
    async fn processor_failure_cancels_node_without_waiting_for_another_request() {
        let token = CancellationToken::new();
        let (_tx, rx) = mpsc::channel(1);
        let mut receiver = MockReceiver::new();
        receiver
            .expect_start()
            .return_once(|_requests| tokio::spawn(async { Err(EngineError::ShadowInternalReset) }));
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            EngineActor::new(token.clone(), rx, receiver).start(()),
        )
        .await
        .unwrap();
        assert!(matches!(result, Err(EngineError::ShadowInternalReset)));
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn cancellation_joins_processor_and_closes_the_inbound_queue() {
        let token = CancellationToken::new();
        let (tx, rx) = mpsc::channel(1);
        let mut receiver = MockReceiver::new();
        receiver.expect_start().return_once(|mut requests| {
            tokio::spawn(async move {
                requests.recv().await;
                Ok(())
            })
        });
        token.cancel();
        EngineActor::new(token, rx, receiver).start(()).await.unwrap();
        assert!(tx.is_closed(), "shutdown must drop the processor, not detach it");
    }
}
