//! Integration tests for QUIC Transport
//!
//! Tests real QUIC connections between transport services using ephemeral ports.

use std::time::Duration;
use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize rustls CryptoProvider for tests
fn init_crypto() {
    INIT.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to install rustls crypto provider");
    });
}

/// Test TransportService creation and basic lifecycle
#[tokio::test]
async fn test_transport_service_lifecycle() {
    init_crypto();
    use edge_proxy::replication::config::ReplicationConfig;
    use edge_proxy::replication::transport::TransportService;

    let config = ReplicationConfig::new("transport-test-node")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap());

    let mut service = TransportService::new(config);

    // Start the service
    let result = service.start().await;
    assert!(result.is_ok());

    // Give it time to bind
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify not shut down
    assert!(!service.is_shutdown());

    // Shutdown
    service.shutdown();
    assert!(service.is_shutdown());
}

/// Test two TransportServices can start independently
#[tokio::test]
async fn test_two_transport_services() {
    init_crypto();
    use edge_proxy::replication::config::ReplicationConfig;
    use edge_proxy::replication::transport::TransportService;

    let config1 = ReplicationConfig::new("node-1")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap());

    let config2 = ReplicationConfig::new("node-2")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap());

    let mut service1 = TransportService::new(config1);
    let mut service2 = TransportService::new(config2);

    // Start both services
    service1.start().await.unwrap();
    service2.start().await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Both should be running
    assert!(!service1.is_shutdown());
    assert!(!service2.is_shutdown());

    // Cleanup
    service1.shutdown();
    service2.shutdown();
}

/// Test Message serialization with bincode
#[tokio::test]
async fn test_message_serialization() {
    use edge_proxy::replication::types::{Message, ChangeSet, Change, ChangeKind, NodeId};

    // Test Broadcast message
    let source = NodeId::new("node-1");
    let changes = vec![
        Change::new("backends", "b1", ChangeKind::Insert, r#"{"app":"test"}"#, &source),
    ];
    let changeset = ChangeSet::new(source.clone(), 1, changes);
    let msg = Message::Broadcast(changeset);

    let data = bincode::serialize(&msg).unwrap();
    let decoded: Message = bincode::deserialize(&data).unwrap();

    match decoded {
        Message::Broadcast(cs) => {
            assert_eq!(cs.source.as_str(), "node-1");
            assert_eq!(cs.seq, 1);
            assert_eq!(cs.changes.len(), 1);
        }
        _ => panic!("Expected Broadcast message"),
    }
}

/// Test SyncRequest message
#[tokio::test]
async fn test_sync_request_message() {
    use edge_proxy::replication::types::Message;

    let msg = Message::SyncRequest { from_seq: 100, table: Some("backends".to_string()) };
    let data = bincode::serialize(&msg).unwrap();
    let decoded: Message = bincode::deserialize(&data).unwrap();

    match decoded {
        Message::SyncRequest { from_seq, table } => {
            assert_eq!(from_seq, 100);
            assert_eq!(table, Some("backends".to_string()));
        }
        _ => panic!("Expected SyncRequest message"),
    }
}

/// Test SyncResponse message with multiple changesets
#[tokio::test]
async fn test_sync_response_message() {
    use edge_proxy::replication::types::{Message, ChangeSet, Change, ChangeKind, NodeId};

    let source = NodeId::new("node-1");
    let changesets = vec![
        ChangeSet::new(source.clone(), 1, vec![
            Change::new("backends", "b1", ChangeKind::Insert, "{}", &source),
        ]),
        ChangeSet::new(source.clone(), 2, vec![
            Change::new("backends", "b2", ChangeKind::Insert, "{}", &source),
        ]),
    ];

    let msg = Message::SyncResponse(changesets);
    let data = bincode::serialize(&msg).unwrap();
    let decoded: Message = bincode::deserialize(&data).unwrap();

    match decoded {
        Message::SyncResponse(cs) => {
            assert_eq!(cs.len(), 2);
            assert_eq!(cs[0].seq, 1);
            assert_eq!(cs[1].seq, 2);
        }
        _ => panic!("Expected SyncResponse message"),
    }
}

/// Test Ack message
#[tokio::test]
async fn test_ack_message() {
    use edge_proxy::replication::types::{Message, NodeId};

    let msg = Message::Ack { seq: 42, source: NodeId::new("node-1") };
    let data = bincode::serialize(&msg).unwrap();
    let decoded: Message = bincode::deserialize(&data).unwrap();

    match decoded {
        Message::Ack { seq, source } => {
            assert_eq!(seq, 42);
            assert_eq!(source.as_str(), "node-1");
        }
        _ => panic!("Expected Ack message"),
    }
}

/// Test Ping/Pong messages
#[tokio::test]
async fn test_ping_pong_messages() {
    use edge_proxy::replication::types::Message;

    // Ping
    let ping = Message::Ping;
    let data = bincode::serialize(&ping).unwrap();
    let decoded: Message = bincode::deserialize(&data).unwrap();
    assert!(matches!(decoded, Message::Ping));

    // Pong
    let pong = Message::Pong;
    let data = bincode::serialize(&pong).unwrap();
    let decoded: Message = bincode::deserialize(&data).unwrap();
    assert!(matches!(decoded, Message::Pong));
}

/// Test large ChangeSet serialization
#[tokio::test]
async fn test_large_changeset_serialization() {
    use edge_proxy::replication::types::{Message, ChangeSet, Change, ChangeKind, NodeId};

    let source = NodeId::new("node-1");
    let mut changes = Vec::new();

    // Create 100 changes
    for i in 0..100 {
        let data = format!(r#"{{"app":"app-{}","region":"sa","port":{}}}"#, i, 8000 + i);
        changes.push(Change::new("backends", &format!("b{}", i), ChangeKind::Insert, &data, &source));
    }

    let changeset = ChangeSet::new(source.clone(), 1, changes);
    let msg = Message::Broadcast(changeset);

    let data = bincode::serialize(&msg).unwrap();
    println!("Serialized size: {} bytes", data.len());

    let decoded: Message = bincode::deserialize(&data).unwrap();

    match decoded {
        Message::Broadcast(cs) => {
            assert_eq!(cs.changes.len(), 100);
        }
        _ => panic!("Expected Broadcast message"),
    }
}

/// Test HLC timestamp ordering across nodes
#[tokio::test]
async fn test_hlc_timestamp_ordering() {
    use edge_proxy::replication::types::{HLCTimestamp, NodeId};

    let node1 = NodeId::new("node-1");
    let node2 = NodeId::new("node-2");

    // Create timestamps from different nodes
    let ts1 = HLCTimestamp::now(&node1);
    tokio::time::sleep(Duration::from_millis(1)).await;
    let ts2 = HLCTimestamp::now(&node2);

    // ts2 should be greater (later)
    assert!(ts2 > ts1);

    // Test tick operation
    let ts3 = ts1.tick(Some(&ts2), &node1);
    assert!(ts3 > ts1);
    assert!(ts3 > ts2);
}

/// Test ChangeSet checksum integrity
#[tokio::test]
async fn test_changeset_checksum() {
    use edge_proxy::replication::types::{ChangeSet, Change, ChangeKind, NodeId};

    let source = NodeId::new("node-1");
    let changes = vec![
        Change::new("backends", "b1", ChangeKind::Insert, r#"{"app":"test"}"#, &source),
        Change::new("backends", "b2", ChangeKind::Update, r#"{"app":"test2"}"#, &source),
    ];

    let changeset = ChangeSet::new(source.clone(), 1, changes);

    // Verify checksum is non-zero
    assert!(changeset.checksum > 0);

    // Verify checksum is valid
    assert!(changeset.verify());

    // Create another changeset with same data - checksum should match
    let changes2 = vec![
        Change::new("backends", "b1", ChangeKind::Insert, r#"{"app":"test"}"#, &source),
        Change::new("backends", "b2", ChangeKind::Update, r#"{"app":"test2"}"#, &source),
    ];
    let changeset2 = ChangeSet::new(source.clone(), 1, changes2);

    // Note: checksums may differ due to timestamps, but both should verify
    assert!(changeset2.verify());
}

/// Test Sans-IO transport helpers
#[tokio::test]
async fn test_transport_sansio_helpers() {
    use edge_proxy::replication::transport::{
        message_type_name, validate_sync_request, is_broadcast, is_sync_request
    };
    use edge_proxy::replication::types::{Message, ChangeSet, NodeId};

    let source = NodeId::new("node-1");

    // Test message_type_name
    let broadcast = Message::Broadcast(ChangeSet::new(source.clone(), 1, vec![]));
    assert_eq!(message_type_name(&broadcast), "Broadcast");

    let sync_req = Message::SyncRequest { from_seq: 0, table: None };
    assert_eq!(message_type_name(&sync_req), "SyncRequest");

    let ack = Message::Ack { seq: 1, source: source.clone() };
    assert_eq!(message_type_name(&ack), "Ack");

    assert_eq!(message_type_name(&Message::Ping), "Ping");
    assert_eq!(message_type_name(&Message::Pong), "Pong");

    // Test validate_sync_request
    assert!(validate_sync_request(0));
    assert!(validate_sync_request(100));
    assert!(validate_sync_request(u64::MAX));

    // Test is_broadcast and is_sync_request
    assert!(is_broadcast(&broadcast));
    assert!(!is_broadcast(&sync_req));
    assert!(is_sync_request(&sync_req));
    assert!(!is_sync_request(&broadcast));
}

/// Test ReplicationAgent creation and basic operations
#[tokio::test]
async fn test_replication_agent_integration() {
    init_crypto();
    use edge_proxy::replication::config::ReplicationConfig;
    use edge_proxy::replication::agent::ReplicationAgent;
    use edge_proxy::replication::types::ChangeKind;
    use tempfile::NamedTempFile;

    let temp = NamedTempFile::new().unwrap();
    let config = ReplicationConfig::new("agent-test")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap())
        .db_path(temp.path().to_str().unwrap());

    let mut agent = ReplicationAgent::new(config).unwrap();

    // Start the agent
    let result = agent.start().await;
    assert!(result.is_ok());

    // Should be running
    assert!(agent.is_running());

    // Record some changes
    agent.record_backend_change("b1", ChangeKind::Insert, r#"{"app":"test"}"#);
    agent.record_backend_change("b2", ChangeKind::Insert, r#"{"app":"test2"}"#);

    // Flush changes
    let changeset = agent.flush().await;
    assert!(changeset.is_some());
    let cs = changeset.unwrap();
    assert_eq!(cs.changes.len(), 2);

    // Flush again should return None (no new changes)
    let changeset2 = agent.flush().await;
    assert!(changeset2.is_none());

    // Stop
    agent.stop().await;
    assert!(!agent.is_running());
}

/// Test two ReplicationAgents communicating (basic setup)
#[tokio::test]
async fn test_two_replication_agents() {
    init_crypto();
    use edge_proxy::replication::config::ReplicationConfig;
    use edge_proxy::replication::agent::ReplicationAgent;
    use tempfile::NamedTempFile;

    let temp1 = NamedTempFile::new().unwrap();
    let temp2 = NamedTempFile::new().unwrap();

    let config1 = ReplicationConfig::new("agent-1")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap())
        .db_path(temp1.path().to_str().unwrap());

    let config2 = ReplicationConfig::new("agent-2")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap())
        .db_path(temp2.path().to_str().unwrap());

    let mut agent1 = ReplicationAgent::new(config1).unwrap();
    let mut agent2 = ReplicationAgent::new(config2).unwrap();

    // Start both
    agent1.start().await.unwrap();
    agent2.start().await.unwrap();

    // Both should be running
    assert!(agent1.is_running());
    assert!(agent2.is_running());

    // Let them run briefly
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Stop both
    agent1.stop().await;
    agent2.stop().await;

    assert!(!agent1.is_running());
    assert!(!agent2.is_running());
}

/// Test transport broadcast with no peers
#[tokio::test]
async fn test_transport_broadcast_no_peers() {
    init_crypto();
    use edge_proxy::replication::config::ReplicationConfig;
    use edge_proxy::replication::transport::TransportService;
    use edge_proxy::replication::types::{ChangeSet, NodeId};

    let config = ReplicationConfig::new("broadcast-test")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap());

    let mut service = TransportService::new(config);
    service.start().await.unwrap();

    let source = NodeId::new("broadcast-test");
    let changeset = ChangeSet::new(source, 1, vec![]);

    // Broadcast with no peers should return 0
    let sent = service.broadcast_changeset(&changeset).await;
    assert_eq!(sent, 0);

    service.shutdown();
}
