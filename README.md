WebRTC Mix-Net Prototype

A high-performance, constant-rate mix-net proof-of-concept written in Rust. This project demonstrates how to combine multi-hop onion routing, peer-to-peer WebRTC transport, and continuous traffic shaping to defeat network observers and traffic correlation attacks.

It is currently implemented as a local 3-node test harness that proves the core cryptographic and networking engines work together seamlessly.

🚀 Currently Implemented Features

WebRTC Transport Layer: Uses webrtc-rs to establish peer-to-peer DataChannels with STUN/ICE for NAT traversal.

Sphinx Onion Routing: Integrates sphinx-packet to wrap payloads in indistinguishable, multi-layer cryptographic onions. Every packet on the wire is padded to exactly 1372 bytes, ensuring size uniformity.

Noise Protocol Link Security: Upgrades WebRTC DataChannels with the Noise_XX_25519_ChaChaPoly_SHA256 handshake (snow crate). This ensures mutually authenticated and encrypted P2P links before Sphinx packets are transmitted.

Constant-Rate Traffic Shaping (Loopix-style): A dedicated run_sender async loop fires exactly one packet every 250ms (4Hz). If no real user message is queued, the node dynamically generates and fires a randomized multi-hop dummy packet to maintain a constant traffic footprint.

Replay Attack Protection: Ephemeral Sphinx header keys (shared secrets) are extracted and cached in a thread-safe HashSet. Intercepted and replayed packets are instantly detected and dropped.

Per-Hop Delays: Packets are held at middle nodes for a cryptographically secure, randomized duration before forwarding, destroying temporal correlation.

💻 Running the Demo

The current main.rs acts as a local test harness. It spins up 3 nodes and connects them in a linear topology (Node 1 <-> Node 2 <-> Node 3) by passing WebRTC SDP offers directly in memory.

To run the simulation:

cargo run


What you will see:

Bootstrapping: The nodes gather ICE candidates and negotiate WebRTC connections.

Handshakes: The Noise_XX protocol secures the links.

Idle Phase: The network idles for 5 seconds. You will see traffic stats print out, proving that nodes are actively generating and sending ~20 dummy packets to each other even when no real messages are sent.

Message Delivery: A real message is sent from Node 1, delayed and routed through Node 2, and decrypted at Node 3.

Burst Handling: A burst of 30 messages is queued at once. The stats will show the node strictly adhering to the 4-packets-per-second limit, masking the burst from network observers.

🚧 Future Roadmap & Next Steps

This project is an evolving prototype. The core cryptography and routing logic are complete, but the following features are planned to make it a fully decentralized, production-ready network:

[ ] Directory Authority (Bootstrapping): Replace the in-memory SDP exchange in main.rs with a lightweight HTTP server (axum). Nodes will publish their public keys and WebRTC offers to this directory so clients can discover them over the internet.

[ ] Payload Type-Tagging: Prepend a byte tag (e.g., 0x00 vs 0x01) to payloads so receiving nodes can cleanly distinguish dummy cover traffic from real messages (preventing the terminal from printing decrypted dummy bytes).

[ ] Strict Guard Node Pinning: Activate the guards architecture so client nodes securely lock in 3 trusted entry points, ensuring their IP address is never exposed to the wider swarm.

[ ] Bounded Memory Caches: Upgrade the unbounded HashSet used for replay protection to an LRU cache or a time-windowed Bloom Filter to cap memory usage over long uptimes.

[ ] CLI Chat Interface: Separate the nodes into a terminal-based UI, allowing users to spin up relays or clients and type real-time messages to each other.

⚠️ Disclaimer

This is a prototype built for educational and architectural demonstration purposes. While it uses audited cryptographic primitives, the overarching network protocol is experimental. Do not use this for critical anonymity or operational security.



ONLY THE README AND MAIN.RS WHICH JUST RUNS THE NETWORK IS AI MADE
