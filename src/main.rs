mod network;

use network::{P2PNode, CELL_SIZE};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    println!("Initializing WebRTC mix-net (3 nodes)...");
    let node1 = P2PNode::new();
    let node2 = P2PNode::new();
    let node3 = P2PNode::new();

    // sanity check: every packet we can build must be exactly CELL_SIZE bytes
    for path in [vec![node2.public_key], vec![node2.public_key, node3.public_key]] {
        let len = P2PNode::build_sphinx_packet(b"x", path.clone()).len();
        assert_eq!(len, CELL_SIZE, "a {}-hop packet is {len} bytes, not CELL_SIZE", path.len());
    }
    println!("packet size check passed: {CELL_SIZE} bytes for 1-hop and 2-hop routes");

    // node1 <-> node2
    let (pc1, rx1) = node1.create_connection(node2.public_key.clone()).await;
    let (pc2, rx2) = node2.create_connection(node1.public_key.clone()).await;
    let (offer, _dc1) = node1.generate_offer(&pc1, rx1, Arc::new(Notify::new()), node2.public_key.clone()).await;
    let answer = node2.accept_offer_and_answer(&pc2, rx2, &offer).await;
    node1.finalize_connection(&pc1, &answer).await;

    // node2 <-> node3
    let (pc2b, rx2b) = node2.create_connection(node3.public_key.clone()).await;
    let (pc3, rx3) = node3.create_connection(node2.public_key.clone()).await;
    let (offer, _dc2) = node2.generate_offer(&pc2b, rx2b, Arc::new(Notify::new()), node3.public_key.clone()).await;
    let answer = node3.accept_offer_and_answer(&pc3, rx3, &offer).await;
    node2.finalize_connection(&pc2b, &answer).await;

    node1.wait_for_link(&node2.public_key, Duration::from_secs(15)).await;
    node2.wait_for_link(&node3.public_key, Duration::from_secs(15)).await;
    node3.wait_for_link(&node2.public_key, Duration::from_secs(15)).await;

    println!("\n=== CHECKPOINT 1: idle for 5 seconds (expect ~20 noise cells per link at 250 ms) ===");
    sleep(Duration::from_secs(5)).await;
    node1.print_stats("node1").await;
    node2.print_stats("node2").await;
    node3.print_stats("node3").await;

    println!("\n=== CHECKPOINT 2: one real message ===");
    let packet = P2PNode::build_sphinx_packet(b"hello through the mix-net", vec![node2.public_key, node3.public_key]);
    node1.send_to(&node2.public_key, &packet).await;
    sleep(Duration::from_secs(3)).await;
    node1.print_stats("node1").await;

    println!("\n=== CHECKPOINT 3: 20 messages queued at once (expect ~4 real cells per second) ===");
    for i in 0..30 {
        let p = P2PNode::build_sphinx_packet(format!("burst {i}").as_bytes(), vec![node2.public_key, node3.public_key]);
        node1.send_to(&node2.public_key, &p).await;
    }
    for sec in 1..=7 {
        sleep(Duration::from_secs(1)).await;
        print!("t+{sec}s  ");
        node1.print_stats("node1").await;
    }
}