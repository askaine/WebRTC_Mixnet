use std::collections::{HashMap,HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify,mpsc};
use webrtc::peer_connection::{
    PeerConnectionBuilder,
    PeerConnectionEventHandler,
    PeerConnection,
    RTCPeerConnectionState,
    RTCIceServer,
	RTCConfigurationBuilder,
	RTCSessionDescription,
	RTCIceGatheringState,
};
use sphinx_packet::crypto::{PrivateKey, PublicKey};
use sphinx_packet::packet::{SphinxPacket, ProcessedPacketData};
use sphinx_packet::route::{Node, NodeAddressBytes, Destination, DestinationAddressBytes};
use sphinx_packet::header::delays::Delay;
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use bytes::{Bytes, BytesMut};
use std::time::Duration;
use snow::{Builder, HandshakeState, TransportState};
use rand::Rng;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::MissedTickBehavior;
use rand::prelude::IteratorRandom;

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
pub type RoutingTable = Arc<Mutex<HashMap<[u8; 32], Arc<SecureLink>>>>;
pub type ReplayCache = Arc<Mutex<HashSet<[u8; 32]>>>;
pub const CELL_SIZE: usize = 1372;
pub const TICK: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct LinkStats {
	pub real_sent: AtomicU64,
	pub noise_sent: AtomicU64,
	pub received: AtomicU64,
	pub not_for_us: AtomicU64,
}

pub struct SecureLink{
	dc: Arc<dyn DataChannel>,
	transport: Mutex<TransportState>,
	out_tx: mpsc::UnboundedSender<Vec<u8>>,
	pub stats: LinkStats,
}

impl SecureLink{
	
	pub async fn send(&self, cell: &[u8]) -> bool{
		if cell.len() != CELL_SIZE { return false; }
		self.out_tx.send(cell.to_vec()).is_ok()
	}
	
	async fn decrypt(&self, ciphertext: &[u8]) -> Option<Vec<u8>>{
		let mut transport = self.transport.lock().await;
		let mut out = vec![0u8; ciphertext.len()];
		let n = transport.read_message(ciphertext,&mut out).ok()?;
		out.truncate(n);
		Some(out)
		
	}
	
	async fn run_sender(link: Arc<SecureLink>, mut out_rx: mpsc::UnboundedReceiver<Vec<u8>>,routing_table: RoutingTable){
		let mut tick = tokio::time::interval(TICK);
		tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
		loop {
			let cell_noise = noise_cell(routing_table.clone()).await;
			tick.tick().await;
			let (cell,is_real) = match out_rx.try_recv(){
				Ok(cell) => (cell,true),
				Err(_) => (cell_noise.clone(),false),
			};
			let wire = {
				let mut transport = link.transport.lock().await;
				let mut buf = vec![0u8; cell.len() + 16];
				match transport.write_message(&cell, &mut buf) {
					Ok(n) => { buf.truncate(n); buf }
					Err(_) => break,
				}
			};
			if link.dc.send(BytesMut::from(&wire[..])).await.is_err() { break; }
			let counter = if is_real { &link.stats.real_sent } else { &link.stats.noise_sent };
			counter.fetch_add(1, Ordering::Relaxed);
		}
	}
}


async fn noise_cell(routing_table: RoutingTable) -> Vec<u8> {
	let mut cell = vec![0u8; 100];
	rand::thread_rng().fill(&mut cell[1..]);
	let random_peers: Vec<PublicKey> = {
                    let table = routing_table.lock().await;
                    let mut rng = rand::thread_rng();
                    table.keys()
                        .choose_multiple(&mut rng, 3)
                        .into_iter()
                        .map(|key_bytes| PublicKey::from(*key_bytes))
                        .collect()
    };
	P2PNode::build_sphinx_packet(&cell, random_peers)
}

#[derive(Clone)]
struct NodeEventHandler{
	pub gather_complete_tx: mpsc::Sender<()>,
	pub private_key: PrivateKey,
	pub routing_table: RoutingTable,
	pub replay_cache: ReplayCache,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for NodeEventHandler {
	
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        println!("Connection State: {:?}", state);
    }

    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
		println!("ICE gathering: {:?}",state);
		if state == RTCIceGatheringState::Complete {
            let _ = self.gather_complete_tx.send(()).await;
        }
    }	
	
	async fn on_data_channel(&self,data_channel: Arc<dyn DataChannel>){
		println!("Node accepted an incoming DataChannel from a remote peer!");
		
		
		let private_key = self.private_key.clone();
		let routing_table = self.routing_table.clone();
		let replay_cache = self.replay_cache.clone();
		tokio::spawn(async move {
            P2PNode::listen_to_data_channel(data_channel, Arc::new(Notify::new()), private_key, routing_table,false,None,replay_cache).await;
        });
	
	}
    
}





pub struct P2PNode {
    pub private_key: PrivateKey,
    pub public_key: PublicKey,
	pub routing_table: RoutingTable,
	pub replay_cache: ReplayCache,
	pub guards: ReplayCache,
}

impl P2PNode {
    pub fn new() -> Self {
        let private_key = PrivateKey::random(); 
        let public_key = PublicKey::from(&private_key);
			
		
        Self {
            private_key,
            public_key,
			routing_table: Arc::new(Mutex::new(HashMap::new())),
			replay_cache: Arc::new(Mutex::new(HashSet::new())),
			guards: Arc::new(Mutex::new(HashSet::new())),
        }
    }

	pub fn build_sphinx_packet(msg: &[u8], peer_pub_keys: Vec<PublicKey>) -> Vec<u8>{
		let mut route = Vec::new();
		
		for pub_key in &peer_pub_keys{
			route.push(Node{
				address: NodeAddressBytes::from_bytes(*pub_key.as_bytes()),
				pub_key: *pub_key,
			});
		}
		
		let dest = Destination::new(DestinationAddressBytes::from_bytes([9u8;32]),[0u8;16]);
		
		
		let mut delays = vec![];
		
		let mut rng = rand::thread_rng();
		for _ in &peer_pub_keys{
			delays.push(Delay::new_from_millis(100+rng.gen_range(0..=(400/50))*50));
		}
		delays.pop();
		delays.push(Delay::new_from_millis(0));
		
		let packet = SphinxPacket::new(msg.to_vec(), &route, &dest, &delays).expect("Failed to build packet");
		
		packet.to_bytes()
	}
	
    pub async fn create_connection(&self,pub_key: PublicKey) -> (Arc<dyn PeerConnection + Send + Sync>,mpsc::Receiver<()>) {
        let config = RTCConfigurationBuilder::default()
            .with_ice_servers(vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }])
            .build();
			
		let (tx, rx) = mpsc::channel(1);	
		
        let peer_connection = PeerConnectionBuilder::new()
            .with_configuration(config)
            .with_handler(Arc::new(NodeEventHandler{ gather_complete_tx: tx ,private_key: self.private_key.clone(),routing_table: self.routing_table.clone(),replay_cache: self.replay_cache.clone()}))
            .with_udp_addrs(vec!["0.0.0.0:0".to_owned()])
            .with_tcp_addrs(vec!["0.0.0.0:0".to_owned()])
            .build()
            .await
            .expect("Failed to build peer connection");
		
		let pc_arc: Arc<dyn PeerConnection + Send + Sync> = Arc::new(peer_connection);
		
		
        (pc_arc,rx)
    }
	
	pub async fn generate_offer(&self, pc: &Arc<dyn PeerConnection + Send + Sync>,mut gather_rx: mpsc::Receiver<()>, on_open_notify: Arc<Notify>, expected_public_key: PublicKey) -> (String, Arc<dyn DataChannel>){
		let _data_channel = pc.create_data_channel("mixnet-channel", None).await.expect("Failed to create data channel");
		
		let dc_clone = _data_channel.clone();
		let private_key_clone = self.private_key.clone();
		let routing_table_clone = self.routing_table.clone();
		let replay_cache_clone = self.replay_cache.clone();
		
        tokio::spawn(async move {
            Self::listen_to_data_channel(dc_clone, on_open_notify,private_key_clone,routing_table_clone,true,Some(*expected_public_key.as_bytes()),replay_cache_clone).await;
        });
		
		
		let offer = pc.create_offer(None).await.expect("Failed to create offer");
		pc.set_local_description(offer.clone()).await.expect("Failed to set local description");
		
		let _ = gather_rx.recv().await;
		let full = pc.local_description().await.expect("no local description");
		(serde_json::to_string(&full).unwrap(),_data_channel)
	}
	
	pub async fn accept_offer_and_answer(&self, pc: &Arc<dyn PeerConnection + Send + Sync>,mut gather_rx: mpsc::Receiver<()>, offer_json: &str) -> String {
        let offer: RTCSessionDescription = serde_json::from_str(offer_json).expect("Invalid JSON offer");
        pc.set_remote_description(offer).await.expect("Failed to set remote description");
        let answer = pc.create_answer(None).await.expect("Failed to create answer");
		
        pc.set_local_description(answer.clone()).await.expect("Failed to set local description");
		let _ = gather_rx.recv().await;
		let full = pc.local_description().await.expect("no local description");
        serde_json::to_string(&full).unwrap()
    }
	
	pub async fn finalize_connection(&self, pc: &Arc<dyn PeerConnection + Send + Sync>, answer_json: &str) {
        let answer: RTCSessionDescription = serde_json::from_str(answer_json).expect("Invalid JSON answer");
        pc.set_remote_description(answer).await.expect("Failed to set remote description");
        
        println!("Handshake finalized! WebRTC is now negotiating the connection in the background.");
    }
	
	async fn noise_handshake(dc: &Arc<dyn DataChannel>, private_key: &PrivateKey, initiator: bool) -> Option<(TransportState, [u8; 32])> {
		let key = private_key.to_bytes();
		let params = PATTERN.parse().ok()?;
		let builder = Builder::new(params).local_private_key(&key).ok()?;
		let built = if initiator { builder.build_initiator() } else { builder.build_responder() };
		let mut handshake_state = built.ok()?;
		let mut buf = vec![0u8; 65535];
		let mut scratch = vec![0u8; 65535];
		
		if initiator{
			loop {
				match dc.poll().await {
					Some(DataChannelEvent::OnOpen) => break,
					Some(DataChannelEvent::OnClose) | None => return None,
					_ => {}
				}
			}
		}
		
		while !handshake_state.is_handshake_finished(){
			if handshake_state.is_my_turn(){
				let n = handshake_state.write_message(&[], &mut buf).ok()?;
				dc.send(BytesMut::from(&buf[..n])).await.ok()?;
			}
			else{
				match dc.poll().await {
					Some(DataChannelEvent::OnMessage(msg)) => { handshake_state.read_message(&msg.data, &mut scratch).ok()?; }
					Some(DataChannelEvent::OnClose) | None => return None,
					_ => {}
				}
			}
		}
		let remote: [u8; 32] = handshake_state.get_remote_static()?.try_into().ok()?;
		Some((handshake_state.into_transport_mode().ok()?, remote))
	}
	
	
	
	
	pub async fn listen_to_data_channel(data_channel: Arc<dyn DataChannel>, on_open_notify: Arc<Notify>, private_key: PrivateKey, routing_table: RoutingTable, initiator: bool, expected: Option<[u8; 32]>,replay_cache: ReplayCache) {
        
		let result = tokio::time::timeout(HANDSHAKE_TIMEOUT, Self::noise_handshake(&data_channel,&private_key,initiator)).await;
		let (transport, remote_key) = match result {
            Ok(Some((t, r))) => (t, r),
            _ => {
                println!("Noise handshake failed or timed out: dropping link");
                let _ = data_channel.close().await;
                return;
            }
        };
		
		if let Some(expected) = expected {
			if remote_key != expected{
				println!("peer proved key {:02x?} but we expected {:02x?}: REJECTED", &remote_key[..4], &expected[..4]);
				let _ = data_channel.close().await;
				return;
			}
		}
		
		let (out_tx, out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
		let link = Arc::new(SecureLink{dc: data_channel.clone(), transport: Mutex::new(transport), out_tx, stats: LinkStats::default(),});
		tokio::spawn(SecureLink::run_sender(link.clone(), out_rx,routing_table.clone()));
		routing_table.lock().await.insert(remote_key, link.clone());
		println!("secure link established with peer {:02x?}", &remote_key[..4]);
		on_open_notify.notify_one();
		
		while let Some(event) = data_channel.poll().await {
			match event {
				DataChannelEvent::OnMessage(msg) => {
					let Some(plain) = link.decrypt(&msg.data).await else {
						println!("link message failed to decrypt: dropped");
						continue;
					};
					link.stats.received.fetch_add(1, Ordering::Relaxed);
					Self::handle_cell(plain, &private_key, &routing_table, &link.stats,&replay_cache).await;
				}
				DataChannelEvent::OnClose => {
					println!("Data channel closed");
					routing_table.lock().await.remove(&remote_key);
					break;
				}
				_ => {}
			}
		}
    }
	
	async fn handle_cell(raw_bytes: Vec<u8>, private_key: &PrivateKey, routing_table: &RoutingTable, stats: &LinkStats,replay_cache: &ReplayCache)
	{
		let Ok(packet) = SphinxPacket::from_bytes(&raw_bytes) else{
			stats.not_for_us.fetch_add(1, Ordering::Relaxed);
			return;
		};
		let replay_tag = *packet.header.shared_secret.as_bytes();
		if !replay_cache.lock().await.insert(replay_tag){
			return;
		}
		let Ok(processed) = packet.process(private_key) else{
			stats.not_for_us.fetch_add(1, Ordering::Relaxed);
			return;
		};
		
		match processed.data{
			ProcessedPacketData::FinalHop{payload, ..} => {
				match payload.recover_plaintext(){
					Ok(plain_text) => println!(">>> SPHINX FINAL DESTINATION. Decrypted payload: {}", String::from_utf8_lossy(&plain_text)),
					Err(_) => println!(">>> final hop with a corrupt payload: dropped"),
				}
				
			}
			
			ProcessedPacketData::ForwardHop{next_hop_address, delay, next_hop_packet} =>{
				let target = *next_hop_address.as_bytes();
				println!(">>> SPHINX MIDDLE NODE. Forwarding to {:02x?} after {:?}", &target[..4], delay.to_duration());
				let bytes = next_hop_packet.to_bytes();
				let routing_table = routing_table.clone();
				
				tokio::spawn(async move{
					tokio::time::sleep(delay.to_duration()).await;
					
					let link = routing_table.lock().await.get(&target).cloned();
					
					match link {
                        Some(link) => {
                            if link.send(&bytes).await {
                                println!(">>> Forwarding successful!");
                            } else {
                                println!(">>> Forwarding failed! Next hop not found or send failed.");
                            }
                        }
                        None => println!(">>> Forwarding failed! Next hop not found."),
                    }
				});
				
			}
		}
	}
	
	pub async fn send_to(&self,peer: &PublicKey,plaintext: &[u8]) -> bool{
		let link = self.routing_table.lock().await.get(peer.as_bytes()).cloned();
			
		match link {
			Some(link) => link.send(plaintext).await,
			None => false,
		}
		
	}
	
	
	pub async fn wait_for_link(&self, peer: &PublicKey, max: Duration) -> bool {
		let deadline = tokio::time::Instant::now() + max;
		while tokio::time::Instant::now() < deadline {
			if self.routing_table.lock().await.contains_key(peer.as_bytes()) { return true; }
			tokio::time::sleep(Duration::from_millis(100)).await;
		}
		false
	}
	
	pub async fn print_stats(&self, name: &str) {
	let table = self.routing_table.lock().await;
	for (peer, link) in table.iter() {
		let s = &link.stats;
		println!("[{name}] link {:02x?}: real sent {}, noise sent {}, received {}, not for us {}",
			&peer[..4],
			s.real_sent.load(Ordering::Relaxed), s.noise_sent.load(Ordering::Relaxed),
			s.received.load(Ordering::Relaxed), s.not_for_us.load(Ordering::Relaxed));
	}
}
}

