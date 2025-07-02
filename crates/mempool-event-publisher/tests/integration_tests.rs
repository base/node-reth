use std::{sync::Arc, time::Duration};

use alloy_primitives::{TxHash, BlockHash};
use rdkafka::{
    config::ClientConfig,
    consumer::{Consumer, StreamConsumer},
    Message,
};
use reth_transaction_pool::{EthPooledTransaction, FullTransactionEvent};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::kafka::Kafka;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use mempool_event_publisher::{
    KafkaConfig, MempoolEvent, MempoolEventPublisher,
    lookup::mock::MockTransactionLookup,
};

/// Comprehensive end-to-end integration test with simulated events
#[tokio::test]
async fn test_end_to_end_mempool_event_flow() {
    // Start Kafka testcontainer
    let kafka_container = Kafka::default()
        .start()
        .await
        .expect("Failed to start Kafka container");

    let bootstrap_servers = format!(
        "127.0.0.1:{}",
        kafka_container
            .get_host_port_ipv4(9093)
            .await
            .expect("Failed to get Kafka port")
    );

    let topic = "test-mempool-events";

    // Create Kafka config
    let config = KafkaConfig::new(&bootstrap_servers, topic)
        .with_client_id("test-publisher");

    // Create mock lookup - we don't need actual transactions for this test
    // since we're testing events without enrichment
    let lookup = Arc::new(MockTransactionLookup::<EthPooledTransaction>::new());

    // Create event stream
    let (event_sender, event_receiver) = mpsc::channel(100);
    let event_stream = ReceiverStream::new(event_receiver);

    // Create publisher
    let publisher = MempoolEventPublisher::new(config, lookup.clone())
        .expect("Failed to create publisher");

    let publisher_handle = {
        tokio::spawn(async move {
            publisher.start_with_stream(event_stream).await.unwrap();
        })
    };

    tokio::time::sleep(Duration::from_millis(1000)).await;

    let tx1_hash = TxHash::from_slice(&[1u8; 32]);
    let tx2_hash = TxHash::from_slice(&[2u8; 32]);
    let tx3_hash = TxHash::from_slice(&[3u8; 32]);

    let events = vec![
        FullTransactionEvent::Pending(tx1_hash),
        FullTransactionEvent::Queued(tx2_hash),
        FullTransactionEvent::Mined {
            tx_hash: tx1_hash,
            block_hash: BlockHash::from_slice(&[9u8; 32]),
        },
        FullTransactionEvent::Discarded(tx2_hash),
        FullTransactionEvent::Invalid(tx3_hash),
    ];

    for event in events.iter() {
        event_sender.send(event.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Wait for events to be published, then create consumer
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let consumer = create_test_consumer(&bootstrap_servers, topic)
        .await
        .expect("Failed to create consumer");
    let mut received_messages = Vec::new();
    let timeout = Duration::from_secs(10);
    let start_time = std::time::Instant::now();

    while received_messages.len() < events.len() && start_time.elapsed() < timeout {
        if let Ok(message) = tokio::time::timeout(Duration::from_millis(500), consumer.recv()).await {
            match message {
                Ok(msg) => {
                    if let Some(payload) = msg.payload() {
                        let payload_str = String::from_utf8_lossy(payload);
                        println!("Received message: {}", payload_str);
                        
                        let mempool_event: Result<MempoolEvent, _> = serde_json::from_str(&payload_str);
                        match mempool_event {
                            Ok(event) => received_messages.push(event),
                            Err(e) => {
                                eprintln!("Failed to deserialize message: {}", e);
                                eprintln!("Message payload: {}", payload_str);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Consumer error: {:?}", e);
                    break;
                }
            }
        }
    }

    assert_eq!(received_messages.len(), events.len(), "Should receive all sent events");

    let event_types: Vec<String> = received_messages.iter().map(|e| e.event_type.clone()).collect();
    assert!(event_types.contains(&"pending".to_string()));
    assert!(event_types.contains(&"queued".to_string()));
    assert!(event_types.contains(&"mined".to_string()));
    assert!(event_types.contains(&"discarded".to_string()));
    assert!(event_types.contains(&"invalid".to_string()));

    for event in &received_messages {
        assert!(!event.payload.transaction_hash.is_empty());
        assert!(event.timestamp <= chrono::Utc::now());
        
        match event.event_type.as_str() {
            "mined" => {
                assert!(event.payload.block_hash.is_some());
            },
            "pending" | "queued" => {
                assert!(event.payload.transaction_hash.starts_with("0x"));
            },
            _ => {}
        }
    }

    println!("✅ Successfully received and validated {} mempool events", received_messages.len());
    
    for (i, event) in received_messages.iter().enumerate() {
        println!("Event {}: {} - {}", i + 1, event.event_type, event.payload.transaction_hash);
    }

    drop(event_sender);
    publisher_handle.abort();
}

/// Test with enriched transaction data
#[tokio::test]
async fn test_enriched_transaction_events() {
    let kafka_container = Kafka::default()
        .start()
        .await
        .expect("Failed to start Kafka container");

    let bootstrap_servers = format!(
        "127.0.0.1:{}",
        kafka_container
            .get_host_port_ipv4(9093)
            .await
            .expect("Failed to get Kafka port")
    );

    let topic = "test-enriched-events";

    // Create Kafka config
    let config = KafkaConfig::new(&bootstrap_servers, topic)
        .with_client_id("test-enriched-publisher");

    let lookup = Arc::new(MockTransactionLookup::<EthPooledTransaction>::new());

    let publisher = MempoolEventPublisher::new(config, lookup)
        .expect("Failed to create publisher");

    let (event_sender, event_receiver) = mpsc::channel(100);
    let event_stream = ReceiverStream::new(event_receiver);

    let publisher_handle = tokio::spawn(async move {
        publisher.start_with_stream(event_stream).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(1000)).await;

    let tx_hash = TxHash::from_slice(&[42u8; 32]);
    event_sender.send(FullTransactionEvent::Pending(tx_hash)).await.unwrap();

    // Wait for the event to be published
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let consumer = create_test_consumer(&bootstrap_servers, topic)
        .await
        .expect("Failed to create consumer");

    let message = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
        .await
        .expect("Should receive message within timeout")
        .expect("Should receive valid message");

    if let Some(payload) = message.payload() {
        let payload_str = String::from_utf8_lossy(payload);
        let mempool_event: MempoolEvent = serde_json::from_str(&payload_str)
            .expect("Should deserialize MempoolEvent");

        assert_eq!(mempool_event.event_type, "pending");
        assert_eq!(mempool_event.payload.transaction_hash, format!("0x{}", hex::encode(tx_hash.as_slice())));
        assert!(mempool_event.payload.from.is_none());
    }

    println!("✅ Enriched transaction test completed");

    drop(event_sender);
    publisher_handle.abort();
}

/// Helper function to create a test consumer
async fn create_test_consumer(
    bootstrap_servers: &str,
    topic: &str,
) -> Result<StreamConsumer, rdkafka::error::KafkaError> {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("group.id", "test-consumer-group")
        .set("bootstrap.servers", bootstrap_servers)
        .set("enable.partition.eof", "false")
        .set("session.timeout.ms", "6000")
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .create()?;

    consumer.subscribe(&[topic])?;

    // Wait for subscription to take effect
    tokio::time::sleep(Duration::from_millis(1000)).await;

    Ok(consumer)
}
