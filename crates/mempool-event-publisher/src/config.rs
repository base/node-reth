use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KafkaConfig {
    /// Kafka broker addresses (comma-separated)
    pub brokers: String,
    /// Topic to publish mempool events to
    pub topic: String,
    /// Client ID for Kafka producer
    pub client_id: String,
    /// Enable message compression
    pub compression: Option<String>,
    /// Batch size for producer
    pub batch_size: Option<i32>,
    /// Linger time in milliseconds
    pub linger_ms: Option<i32>,
    /// Additional Kafka configuration
    pub additional_config: Option<std::collections::HashMap<String, String>>,
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self {
            brokers: "localhost:9092".to_string(),
            topic: "mempool-events".to_string(),
            client_id: "mempool-event-publisher".to_string(),
            compression: Some("gzip".to_string()),
            batch_size: Some(100),
            linger_ms: Some(5),
            additional_config: None,
        }
    }
}

impl KafkaConfig {
    pub fn new(brokers: impl Into<String>, topic: impl Into<String>) -> Self {
        Self {
            brokers: brokers.into(),
            topic: topic.into(),
            ..Default::default()
        }
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn with_compression(mut self, compression: impl Into<String>) -> Self {
        self.compression = Some(compression.into());
        self
    }

    pub fn with_batch_size(mut self, batch_size: i32) -> Self {
        self.batch_size = Some(batch_size);
        self
    }

    pub fn with_linger_ms(mut self, linger_ms: i32) -> Self {
        self.linger_ms = Some(linger_ms);
        self
    }
}
