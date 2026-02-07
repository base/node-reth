#![allow(missing_docs)]
#![doc = include_str!("../README.md")]
#![doc(issue_tracker_base_url = "https://github.com/base/base/issues/")]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), allow(unused_crate_dependencies))]

mod config;
pub use config::BuilderConfig;

mod metrics;
pub use metrics::BuilderMetrics;

mod execution;
pub use execution::{ExecutionInfo, TxnExecutionError, TxnOutcome};

mod traits;
pub use traits::{ClientBounds, NodeBounds, PayloadTxsBounds, PoolBounds};

mod storage;
pub use storage::{BaseApiExtServer, StoreData, TxData, TxDataStore, TxDataStoreExt};

mod flashblocks;
pub use flashblocks::{
    FlashblocksConfig, FlashblocksExecutionInfo, FlashblocksExtraCtx, FlashblocksServiceBuilder,
    OpPayloadBuilderCtx, PayloadHandler,
};

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
