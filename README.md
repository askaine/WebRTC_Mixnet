WebRTC Mix-Net
A high-performance, fully decentralized mix-net prototype written in Rust. This project implements cryptographic onion routing over peer-to-peer WebRTC DataChannels, heavily inspired by modern privacy networks like Nym and Tor.

It is designed to defeat both local network watchers and Global Passive Adversaries (GPAs) using strict constant-rate traffic shaping, dummy cover traffic, and multi-layer encryption.

🚀 Core Architecture & Features
WebRTC Transport Layer: Uses webrtc-rs for direct peer-to-peer DataChannels. Bypasses the need for centralized routing by utilizing STUN/ICE for seamless NAT traversal.

Sphinx Onion Routing: Integrates sphinx-packet to wrap payloads in indistinguishable, constant-size cryptographic layers. Middle nodes only know the previous and next hops.

Noise Protocol Link Security: Upgrades WebRTC DataChannels with the Noise_XX_25519_ChaChaPoly_SHA256 handshake (snow crate), ensuring mutually authenticated and encrypted P2P links before Sphinx packets are even processed.

Loopix-Style Traffic Shaping: Defeats timing correlation and traffic analysis. The network operates on a strict 250ms tick interval. If no real message is ready, nodes automatically generate and fire perfectly indistinguishable dummy cover packets (0x00 payload tag).

Replay Attack Protection: Ephemeral Sphinx header keys (shared secrets) are cached in a thread-safe HashSet. Any intercepted and replayed packet is instantly rejected.

Guard Node Pinning: Clients select 3 static entry guards to prevent Eclipse attacks and IP leakage to the wider swarm.

🛠️ Tech Stack
Language: Rust (Strict tokio async/await environment)

Cryptography: sphinx-packet (Onion Routing), snow (Noise Framework), rand

Networking: webrtc (DataChannels, ICE/STUN)

🧠 How It Works
Bootstrapping: Nodes publish their WebRTC SDP offers to a lightweight HTTP Directory Authority.

Circuit Building: A client fetches the directory, randomly pins 3 Guard Nodes, and trades SDP answers to establish P2P WebRTC tunnels.

Continuous Flow: Once connected, the run_sender loop begins ticking at 4Hz. It guarantees exactly one packet of identical size (1372 bytes) is fired down the wire every tick, utilizing single/multi-hop cover traffic when idle.

Onion Decryption: As a packet traverses the network, each node peels one layer of Sphinx encryption, delays the packet by a randomized offset to break temporal tracking, and routes it to the next peer.

🚧 Current Status & Roadmap
This project is currently a working Proof-of-Concept (PoC) demonstrating the core cryptographic and transport engines.

Next Steps / To-Do:

[ ] Directory Authority: Implement a minimal axum HTTP server for initial SDP offer/answer exchange to replace in-memory bootstrapping.

[ ] CLI Chat Interface: Build a clean terminal UI to spin up local topologies, send real messages (0x01 tag), and monitor noise_sent vs real_sent statistics in real-time.

[ ] LRU Replay Cache: Upgrade the unbounded HashSet replay cache to a time-windowed Bloom Filter or LRU cache to cap memory usage.

⚠️ Disclaimer
This is a prototype built for educational and architectural demonstration purposes. While it uses audited cryptographic primitives, the overarching network protocol has not been formally audited. Do not use this for critical anonymity or life-and-death operational security.




ONLY THE README AND MAIN.RS WHICH JUST RUNS THE NETWORK IS AI MADE
