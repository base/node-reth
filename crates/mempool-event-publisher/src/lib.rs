//! Mempool Event Publisher
//!
//! This crate provides functionality to subscribe to Reth transaction pool events
//! and publish them to a Kafka topic for real-time mempool monitoring.

pub mod config;
pub mod error;
pub mod event;
pub mod lookup;
pub mod publisher;

pub use config::KafkaConfig;
pub use error::{Error, Result};
pub use event::MempoolEvent;
pub use lookup::{PoolWrapper, TransactionLookup};
pub use publisher::MempoolEventPublisher;
