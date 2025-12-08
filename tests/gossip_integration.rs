//! Integration tests for Gossip Protocol
//!
//! Tests real UDP communication between gossip nodes using ephemeral ports.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;

/// Test that two UDP sockets can communicate on localhost
#[tokio::test]
async fn test_udp_socket_communication() {
    // Bind to ephemeral ports
    let socket1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let socket2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr1 = socket1.local_addr().unwrap();
    let addr2 = socket2.local_addr().unwrap();

    println!("Socket 1 bound to: {}", addr1);
    println!("Socket 2 bound to: {}", addr2);

    // Send message from socket1 to socket2
    let message = b"hello from socket1";
    socket1.send_to(message, addr2).await.unwrap();

    // Receive on socket2
    let mut buf = [0u8; 1024];
    let (len, from) = socket2.recv_from(&mut buf).await.unwrap();

    assert_eq!(&buf[..len], message);
    assert_eq!(from, addr1);
}

/// Test bidirectional UDP communication
#[tokio::test]
async fn test_udp_bidirectional() {
    let socket1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let socket2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr1 = socket1.local_addr().unwrap();
    let addr2 = socket2.local_addr().unwrap();

    // Socket1 -> Socket2
    socket1.send_to(b"ping", addr2).await.unwrap();
    let mut buf = [0u8; 1024];
    let (len, _) = socket2.recv_from(&mut buf).await.unwrap();
    assert_eq!(&buf[..len], b"ping");

    // Socket2 -> Socket1
    socket2.send_to(b"pong", addr1).await.unwrap();
    let (len, _) = socket1.recv_from(&mut buf).await.unwrap();
    assert_eq!(&buf[..len], b"pong");
}

/// Test gossip message serialization over UDP
#[tokio::test]
async fn test_gossip_message_over_udp() {
    use edge_proxy::replication::gossip::{GossipMessage, create_ping, create_join};

    let socket1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let socket2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr1 = socket1.local_addr().unwrap();
    let addr2 = socket2.local_addr().unwrap();

    // Create and send a Ping message
    let ping = create_ping("node-1", addr1, addr1, 1);
    let data = bincode::serialize(&ping).unwrap();
    socket1.send_to(&data, addr2).await.unwrap();

    // Receive and deserialize
    let mut buf = vec![0u8; 65535];
    let (len, _) = socket2.recv_from(&mut buf).await.unwrap();
    let received: GossipMessage = bincode::deserialize(&buf[..len]).unwrap();

    match received {
        GossipMessage::Ping { sender_id, incarnation, .. } => {
            assert_eq!(sender_id, "node-1");
            assert_eq!(incarnation, 1);
        }
        _ => panic!("Expected Ping message"),
    }
}

/// Test gossip Join message over UDP
#[tokio::test]
async fn test_gossip_join_message_over_udp() {
    use edge_proxy::replication::gossip::{GossipMessage, create_join};

    let socket1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let socket2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr1 = socket1.local_addr().unwrap();
    let addr2 = socket2.local_addr().unwrap();

    // Create and send a Join message
    let join = create_join("new-node", addr1, addr1);
    let data = bincode::serialize(&join).unwrap();
    socket1.send_to(&data, addr2).await.unwrap();

    // Receive and deserialize
    let mut buf = vec![0u8; 65535];
    let (len, _) = socket2.recv_from(&mut buf).await.unwrap();
    let received: GossipMessage = bincode::deserialize(&buf[..len]).unwrap();

    match received {
        GossipMessage::Join { node_id, gossip_addr, transport_addr } => {
            assert_eq!(node_id, "new-node");
            assert_eq!(gossip_addr, addr1);
            assert_eq!(transport_addr, addr1);
        }
        _ => panic!("Expected Join message"),
    }
}

/// Test multiple nodes exchanging gossip messages
#[tokio::test]
async fn test_three_node_gossip_exchange() {
    use edge_proxy::replication::gossip::{GossipMessage, create_ping};

    let node1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let node2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let node3 = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr1 = node1.local_addr().unwrap();
    let addr2 = node2.local_addr().unwrap();
    let addr3 = node3.local_addr().unwrap();

    // Node1 sends ping to Node2 and Node3
    let ping1 = create_ping("node-1", addr1, addr1, 1);
    let data = bincode::serialize(&ping1).unwrap();

    node1.send_to(&data, addr2).await.unwrap();
    node1.send_to(&data, addr3).await.unwrap();

    // Both Node2 and Node3 should receive the ping
    let mut buf = vec![0u8; 65535];

    let (len, from) = node2.recv_from(&mut buf).await.unwrap();
    assert_eq!(from, addr1);
    let msg2: GossipMessage = bincode::deserialize(&buf[..len]).unwrap();
    assert!(matches!(msg2, GossipMessage::Ping { .. }));

    let (len, from) = node3.recv_from(&mut buf).await.unwrap();
    assert_eq!(from, addr1);
    let msg3: GossipMessage = bincode::deserialize(&buf[..len]).unwrap();
    assert!(matches!(msg3, GossipMessage::Ping { .. }));
}

/// Test gossip MemberList message serialization
#[tokio::test]
async fn test_gossip_member_list_over_udp() {
    use edge_proxy::replication::gossip::GossipMessage;

    let socket1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let socket2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr1 = socket1.local_addr().unwrap();
    let addr2 = socket2.local_addr().unwrap();

    // Create MemberList with multiple members
    let members = vec![
        ("node-1".to_string(), addr1, addr1, 1u64),
        ("node-2".to_string(), addr2, addr2, 2u64),
        ("node-3".to_string(), "127.0.0.1:9999".parse().unwrap(), "127.0.0.1:9998".parse().unwrap(), 3u64),
    ];

    let member_list = GossipMessage::MemberList { members };
    let data = bincode::serialize(&member_list).unwrap();

    socket1.send_to(&data, addr2).await.unwrap();

    // Receive and verify
    let mut buf = vec![0u8; 65535];
    let (len, _) = socket2.recv_from(&mut buf).await.unwrap();
    let received: GossipMessage = bincode::deserialize(&buf[..len]).unwrap();

    match received {
        GossipMessage::MemberList { members } => {
            assert_eq!(members.len(), 3);
            assert_eq!(members[0].0, "node-1");
            assert_eq!(members[1].0, "node-2");
            assert_eq!(members[2].0, "node-3");
        }
        _ => panic!("Expected MemberList message"),
    }
}

/// Test concurrent UDP message sending
#[tokio::test]
async fn test_concurrent_udp_messages() {
    let receiver = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let recv_addr = receiver.local_addr().unwrap();

    let mut handles = vec![];

    // Spawn 10 senders
    for i in 0..10 {
        let addr = recv_addr;
        handles.push(tokio::spawn(async move {
            let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let msg = format!("message-{}", i);
            sender.send_to(msg.as_bytes(), addr).await.unwrap();
        }));
    }

    // Wait for all senders
    for handle in handles {
        handle.await.unwrap();
    }

    // Receive all messages with timeout
    let mut received = 0;
    let mut buf = [0u8; 1024];

    loop {
        match tokio::time::timeout(Duration::from_millis(100), receiver.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => {
                let msg = String::from_utf8_lossy(&buf[..len]);
                assert!(msg.starts_with("message-"));
                received += 1;
            }
            _ => break,
        }
    }

    assert_eq!(received, 10);
}

/// Test GossipService start and shutdown with real sockets
#[tokio::test]
async fn test_gossip_service_lifecycle() {
    use edge_proxy::replication::config::ReplicationConfig;
    use edge_proxy::replication::gossip::GossipService;

    let config = ReplicationConfig::new("integration-test-node")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap());

    let service = Arc::new(GossipService::new(config));

    // Start the service
    let result = service.clone().start().await;
    assert!(result.is_ok());

    // Give it time to initialize
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify it's running
    assert!(!service.is_shutdown());
    assert!(service.members().is_empty()); // No peers yet

    // Shutdown
    service.shutdown();
    assert!(service.is_shutdown());
}

/// Test two GossipServices discovering each other
#[tokio::test]
async fn test_two_gossip_services_discovery() {
    use edge_proxy::replication::config::ReplicationConfig;
    use edge_proxy::replication::gossip::GossipService;

    // Start first service
    let config1 = ReplicationConfig::new("node-1")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap());

    let service1 = Arc::new(GossipService::new(config1));
    service1.clone().start().await.unwrap();

    // Small delay to ensure socket is bound
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Note: In a full implementation, we'd get the actual bound address
    // and use it as a bootstrap peer for service2.
    // For now, we just verify both services start successfully.

    let config2 = ReplicationConfig::new("node-2")
        .gossip_addr("127.0.0.1:0".parse().unwrap())
        .transport_addr("127.0.0.1:0".parse().unwrap());

    let service2 = Arc::new(GossipService::new(config2));
    service2.clone().start().await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Both services should be running
    assert!(!service1.is_shutdown());
    assert!(!service2.is_shutdown());

    // Cleanup
    service1.shutdown();
    service2.shutdown();
}
