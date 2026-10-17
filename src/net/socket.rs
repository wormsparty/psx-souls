//! The peer-to-peer connection (WebRTC data channels, matchbox): a reliable channel for the
//! lobby messages, an unreliable one for the rollback traffic (GGRS has its own reliability).
//! The signaling server (`matchbox_server`, `tools/lobby.sh`) only puts the peers in touch.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use ggrs::{Message, NonBlockingSocket};
use matchbox_socket::{PeerId, PeerState, WebRtcChannel, WebRtcSocket};
use serde::Serialize;
use serde::de::DeserializeOwned;

const RELIABLE: usize = 0;
const UNRELIABLE: usize = 1;

pub struct Socket {
    rtc: WebRtcSocket,
    /// Peers whose data channels exist: matchbox panics when sending to any other.
    known: Arc<Mutex<HashSet<PeerId>>>,
    /// Why the connection ended (signaling server unreachable…), set by the message loop.
    ended: Arc<Mutex<Option<String>>>,
}

impl Socket {
    /// Connects to the room `url` (signaling server address + room name).
    pub fn open(url: &str) -> Self {
        let (rtc, message_loop) = WebRtcSocket::builder(url).add_reliable_channel().add_unreliable_channel().build();
        let ended = Arc::new(Mutex::new(None));
        let flag = ended.clone();
        let task = async move {
            let why = match message_loop.await {
                Ok(()) => "connection closed".to_string(),
                Err(e) => e.to_string(),
            };
            if let Ok(mut f) = flag.lock() {
                *f = Some(why);
            }
        };
        // webrtc-rs runs on tokio: `Compat` provides it with a runtime.
        #[cfg(not(target_arch = "wasm32"))]
        bevy::tasks::IoTaskPool::get().spawn(async_compat::Compat::new(task)).detach();
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(task);
        Self { rtc, known: default(), ended }
    }

    /// Our id, once the signaling server has given it.
    pub fn id(&mut self) -> Option<PeerId> {
        self.rtc.id()
    }

    /// Peers that just connected (`true`) or left (`false`).
    pub fn update(&mut self) -> Vec<(PeerId, bool)> {
        let Ok(changes) = self.rtc.try_update_peers() else { return Vec::new() };
        let mut known = self.known.lock().expect("peer set");
        changes
            .into_iter()
            .map(|(peer, state)| {
                let up = state == PeerState::Connected;
                if up {
                    known.insert(peer);
                }
                (peer, up)
            })
            .collect()
    }

    pub fn connected(&self) -> Vec<PeerId> {
        self.rtc.connected_peers().collect()
    }

    pub fn failure(&self) -> Option<String> {
        self.ended.lock().ok().and_then(|f| f.clone())
    }

    /// Sends a lobby message (reliable, in order).
    pub fn send<T: Serialize>(&mut self, peer: PeerId, msg: &T) {
        if !self.known.lock().is_ok_and(|k| k.contains(&peer)) {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(msg) else { return };
        if let Ok(ch) = self.rtc.get_channel_mut(RELIABLE) {
            let _ = ch.try_send(bytes.into_boxed_slice(), peer);
        }
    }

    pub fn receive<T: DeserializeOwned>(&mut self) -> Vec<(PeerId, T)> {
        let Ok(ch) = self.rtc.get_channel_mut(RELIABLE) else { return Vec::new() };
        ch.receive()
            .into_iter()
            .filter_map(|(peer, bytes)| match serde_json::from_slice(&bytes) {
                Ok(m) => Some((peer, m)),
                Err(e) => {
                    warn!("co-op: unreadable message from {peer}: {e}");
                    None
                }
            })
            .collect()
    }

    /// The unreliable channel, handed over to the rollback session.
    pub fn take_ggrs(&mut self) -> Option<GgrsChannel> {
        let channel = self.rtc.take_channel(UNRELIABLE).ok()?;
        Some(GgrsChannel { channel, known: self.known.clone() })
    }
}

/// GGRS over the unreliable data channel (bincode, the encoding GGRS itself uses).
pub struct GgrsChannel {
    channel: WebRtcChannel,
    known: Arc<Mutex<HashSet<PeerId>>>,
}

impl NonBlockingSocket<PeerId> for GgrsChannel {
    fn send_to(&mut self, msg: &Message, addr: &PeerId) {
        // A peer not connected yet (the other guests, at the start): the synchronisation retries.
        if !self.known.lock().is_ok_and(|k| k.contains(addr)) {
            return;
        }
        if let Ok(bytes) = bincode::serialize(msg) {
            let _ = self.channel.try_send(bytes.into_boxed_slice(), *addr);
        }
    }

    fn receive_all_messages(&mut self) -> Vec<(PeerId, Message)> {
        self.channel
            .receive()
            .into_iter()
            .filter_map(|(peer, bytes)| bincode::deserialize(&bytes).ok().map(|m| (peer, m)))
            .collect()
    }
}
