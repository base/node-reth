use std::fmt::Debug;

use alloy_eips::BlockNumberOrTag;
use async_trait::async_trait;
use base_common_genesis::RollupConfig;
use base_consensus_engine::{EngineQueries, EngineState};
use base_consensus_rpc::EngineRpcClient;
use base_protocol::{L2BlockInfo, OutputRoot};
use derive_more::Constructor;
use jsonrpsee::{
    core::RpcResult,
    types::{ErrorCode, ErrorObject},
};
use tokio::sync::{
    mpsc::{self, error::TrySendError},
    oneshot, watch,
};

use crate::EngineRpcRequest;

/// Queue-based implementation of the [`EngineRpcClient`] trait. This handles all channel-based
/// operations, providing a nice facade for callers.
#[derive(Clone, Constructor, Debug)]
pub struct QueuedEngineRpcClient {
    /// A channel to use to send engine RPC requests.
    pub engine_rpc_request_tx: mpsc::Sender<EngineRpcRequest>,
}

impl QueuedEngineRpcClient {
    /// Attempts to enqueue an engine query without waiting for channel capacity.
    ///
    /// Public RPC requests fail fast under load so they cannot block consensus-critical work.
    pub fn try_enqueue_engine_query(&self, query: EngineQueries) -> RpcResult<()> {
        self.engine_rpc_request_tx.try_send(EngineRpcRequest::EngineQuery(Box::new(query))).map_err(
            |error| match error {
                TrySendError::Full(_) => {
                    warn!(target: "block_engine", "Engine RPC request queue full");
                    ErrorObject::from(ErrorCode::ServerIsBusy)
                }
                TrySendError::Closed(_) => {
                    error!(target: "block_engine", "Failed to enqueue engine RPC request");
                    ErrorObject::from(ErrorCode::InternalError)
                }
            },
        )
    }
}

// Only request/reply plumbing is generated. Admission remains fail-fast and handlers own
// execution; dropping a caller drops its reply receiver without cancelling an admitted query.
macro_rules! engine_queries {
    ($(async fn $name:ident($($arg:ident: $arg_ty:ty),*) -> $result:ty => |$sender:ident| $query:expr, $error:literal;)*) => {
        #[async_trait]
        impl EngineRpcClient for QueuedEngineRpcClient {
            $(
                async fn $name(&self, $($arg: $arg_ty),*) -> RpcResult<$result> {
                    let ($sender, rx) = oneshot::channel();
                    self.try_enqueue_engine_query($query)?;
                    rx.await.map_err(|_| {
                        error!(target: "block_engine", $error);
                        ErrorObject::from(ErrorCode::InternalError)
                    })
                }
            )*
        }
    };
}

engine_queries! {
    async fn get_config() -> RollupConfig => |sender|
        EngineQueries::Config(sender), "Failed to receive config from engine rpc";
    async fn get_state() -> EngineState => |sender|
        EngineQueries::State(sender), "Failed to receive state from engine rpc";
    async fn output_at_block(block: BlockNumberOrTag) -> (L2BlockInfo, OutputRoot, EngineState) => |sender|
        EngineQueries::OutputAtBlock { block, sender }, "Failed to receive output at block from engine rpc";
    async fn dev_get_task_queue_length() -> usize => |sender|
        EngineQueries::TaskQueueLength(sender), "Failed to receive task queue length from engine rpc";
    async fn dev_subscribe_to_engine_queue_length() -> watch::Receiver<usize> => |sender|
        EngineQueries::QueueLengthReceiver(sender), "Failed to receive queue length receiver from engine rpc";
    async fn dev_subscribe_to_engine_state() -> watch::Receiver<EngineState> => |sender|
        EngineQueries::StateReceiver(sender), "Failed to receive state receiver from engine rpc";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_dropped_request_and_reply() {
        let (request_tx, request_rx) = mpsc::channel(1);
        drop(request_rx);
        let error = QueuedEngineRpcClient::new(request_tx).get_config().await.unwrap_err();
        assert_eq!(error.code(), ErrorCode::InternalError.code());

        let (request_tx, mut request_rx) = mpsc::channel(1);
        let client = QueuedEngineRpcClient::new(request_tx);
        let call = tokio::spawn(async move { client.get_config().await });
        drop(request_rx.recv().await.unwrap());
        let error = call.await.unwrap().unwrap_err();
        assert_eq!(error.code(), ErrorCode::InternalError.code());
    }

    #[tokio::test]
    async fn full_queue_fails_fast_and_argument_is_dispatched() {
        let (request_tx, mut request_rx) = mpsc::channel(1);
        let client = QueuedEngineRpcClient::new(request_tx);
        let (tx, _rx) = oneshot::channel();
        client.try_enqueue_engine_query(EngineQueries::Config(tx)).unwrap();
        assert_eq!(client.get_state().await.unwrap_err().code(), ErrorCode::ServerIsBusy.code());
        drop(request_rx.recv().await);

        let block = BlockNumberOrTag::Number(42);
        let call = tokio::spawn(async move { client.output_at_block(block).await });
        match request_rx.recv().await.unwrap() {
            EngineRpcRequest::EngineQuery(query) => match *query {
                EngineQueries::OutputAtBlock { block: actual, sender } => {
                    assert_eq!(actual, block);
                    drop(sender);
                }
                other => panic!("unexpected query: {other:?}"),
            },
        }
        assert_eq!(call.await.unwrap().unwrap_err().code(), ErrorCode::InternalError.code());
    }
}
