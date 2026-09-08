//! The RPC server for the sequencer actor.
//! Mostly handles queries from the admin rpc.

use alloy_primitives::B256;
use async_trait::async_trait;
use base_consensus_rpc::{SequencerAdminAPIClient, SequencerAdminAPIError};
use derive_more::Constructor;
use tokio::sync::{mpsc, oneshot};

use crate::SequencerAdminQuery;

/// Queued implementation of [`SequencerAdminAPIClient`] that handles requests by sending them to
/// a handler via the contained sender.
#[derive(Debug, Clone, Constructor)]
pub struct QueuedSequencerAdminAPIClient {
    /// Queue used to relay admin queries
    request_tx: mpsc::Sender<SequencerAdminQuery>,
}

// Admin operations wait for queue capacity and return the handler's result unchanged.
// Unlike public engine queries, they must not fail fast when the queue is temporarily full.
macro_rules! admin_queries {
    ($(async fn $name:ident($($arg:ident: $arg_ty:ty),*) -> $result:ty => |$tx:ident| $query:expr;)*) => {
        #[async_trait]
        impl SequencerAdminAPIClient for QueuedSequencerAdminAPIClient {
            $(
                async fn $name(&self, $($arg: $arg_ty),*) -> Result<$result, SequencerAdminAPIError> {
                    let ($tx, rx) = oneshot::channel();
                    self.request_tx.send($query).await.map_err(|_| {
                        SequencerAdminAPIError::RequestError("request channel closed".to_string())
                    })?;
                    rx.await.map_err(|_| SequencerAdminAPIError::ResponseError)?
                }
            )*
        }
    };
}

admin_queries! {
    async fn is_sequencer_active() -> bool => |tx| SequencerAdminQuery::SequencerActive(tx);
    async fn is_conductor_enabled() -> bool => |tx| SequencerAdminQuery::ConductorEnabled(tx);
    async fn is_recovery_mode() -> bool => |tx| SequencerAdminQuery::RecoveryMode(tx);
    async fn start_sequencer(unsafe_head: B256) -> () => |tx|
        SequencerAdminQuery::StartSequencer(unsafe_head, tx);
    async fn stop_sequencer() -> B256 => |tx| SequencerAdminQuery::StopSequencer(tx);
    async fn set_recovery_mode(mode: bool) -> () => |tx| SequencerAdminQuery::SetRecoveryMode(mode, tx);
    async fn override_leader() -> () => |tx| SequencerAdminQuery::OverrideLeader(tx);
    async fn reset_derivation_pipeline() -> () => |tx|
        SequencerAdminQuery::ResetDerivationPipeline(tx);
}

#[cfg(test)]
mod tests {
    use std::{pin::pin, task::Poll};

    use futures::poll;

    use super::*;

    #[tokio::test]
    async fn reports_dropped_request_and_reply() {
        let (request_tx, request_rx) = mpsc::channel(1);
        drop(request_rx);
        assert!(matches!(
            QueuedSequencerAdminAPIClient::new(request_tx).is_sequencer_active().await,
            Err(SequencerAdminAPIError::RequestError(message)) if message == "request channel closed"
        ));

        let (request_tx, mut request_rx) = mpsc::channel(1);
        let client = QueuedSequencerAdminAPIClient::new(request_tx);
        let call = tokio::spawn(async move { client.is_sequencer_active().await });
        drop(request_rx.recv().await.unwrap());
        assert!(matches!(call.await.unwrap(), Err(SequencerAdminAPIError::ResponseError)));
    }

    #[tokio::test]
    async fn full_queue_backpressures_and_dispatches_argument() {
        let (request_tx, mut request_rx) = mpsc::channel(1);
        let client = QueuedSequencerAdminAPIClient::new(request_tx.clone());
        let (occupied_tx, _occupied_rx) = oneshot::channel();
        request_tx.send(SequencerAdminQuery::OverrideLeader(occupied_tx)).await.unwrap();

        let head = B256::repeat_byte(42);
        let mut call = pin!(client.start_sequencer(head));
        assert!(matches!(poll!(&mut call), Poll::Pending));
        drop(request_rx.recv().await.unwrap());
        assert!(matches!(poll!(&mut call), Poll::Pending));
        match request_rx.recv().await.unwrap() {
            SequencerAdminQuery::StartSequencer(actual, tx) => {
                assert_eq!(actual, head);
                tx.send(Ok(())).unwrap();
            }
            other => panic!("unexpected query: {other:?}"),
        }
        assert!(call.await.is_ok());
    }
}
