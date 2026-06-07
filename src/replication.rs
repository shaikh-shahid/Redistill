#![allow(dead_code)] // wired into the server in later replication tasks; tighten then

//! Asynchronous primary→replica replication.
//!
//! A primary feeds every successful write into a broadcast channel and tracks
//! a monotonic byte offset. Replicas receive a consistent snapshot followed by
//! the live command stream and apply it through the normal command path.

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Primary,
    Replica,
}

/// State describing the primary this node replicates from (replica role only).
#[derive(Debug, Clone)]
pub struct MasterInfo {
    pub host: String,
    pub port: u16,
    /// true once the initial sync completed and the stream is live.
    pub link_up: bool,
}

/// A single frame on the replication stream: (end_offset, resp_bytes).
pub type Frame = (u64, Bytes);

pub struct Replication {
    role: RwLock<Role>,
    master: RwLock<Option<MasterInfo>>,
    replid: RwLock<String>,
    offset: AtomicU64,
    /// Guards (offset increment + broadcast send) so frame offsets are
    /// monotonic and consistent with broadcast order.
    feed_lock: Mutex<()>,
    sender: broadcast::Sender<Frame>,
    /// Held shared by every command; held exclusively during a full-sync cut so
    /// the snapshot is quiescent and aligns with a single offset.
    pub barrier: RwLock<()>,
    /// Count of currently connected replicas (for INFO / metrics).
    replica_count: AtomicU64,
}

impl Replication {
    pub fn new(backlog: usize) -> Self {
        let (sender, _rx) = broadcast::channel(backlog.max(16));
        Self {
            role: RwLock::new(Role::Primary),
            master: RwLock::new(None),
            replid: RwLock::new(gen_replid()),
            offset: AtomicU64::new(0),
            feed_lock: Mutex::new(()),
            sender,
            barrier: RwLock::new(()),
            replica_count: AtomicU64::new(0),
        }
    }

    pub fn role(&self) -> Role {
        *self.role.read()
    }
    pub fn offset(&self) -> u64 {
        self.offset.load(Ordering::Relaxed)
    }
    pub fn replid(&self) -> String {
        self.replid.read().clone()
    }
    pub fn master(&self) -> Option<MasterInfo> {
        self.master.read().clone()
    }
    pub fn replica_count(&self) -> u64 {
        self.replica_count.load(Ordering::Relaxed)
    }
    pub fn incr_replicas(&self) {
        self.replica_count.fetch_add(1, Ordering::Relaxed);
    }
    pub fn decr_replicas(&self) {
        self.replica_count.fetch_sub(1, Ordering::Relaxed);
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.sender.subscribe()
    }

    /// Feed a write command to all replicas. Cheap when there are no subscribers.
    /// Called from the synchronous command path.
    pub fn feed(&self, command: &[Bytes]) {
        let frame = encode_command(command);
        let _g = self.feed_lock.lock();
        let new_off =
            self.offset.fetch_add(frame.len() as u64, Ordering::Relaxed) + frame.len() as u64;
        // Ignore send errors: zero subscribers is the common case.
        let _ = self.sender.send((new_off, frame));
    }

    /// Become a replica of host:port. Sets role + master info; the actual
    /// connection is driven by the replica client task.
    pub fn set_master(&self, host: String, port: u16) {
        *self.role.write() = Role::Replica;
        *self.master.write() = Some(MasterInfo {
            host,
            port,
            link_up: false,
        });
    }

    pub fn set_link_up(&self, up: bool) {
        if let Some(m) = self.master.write().as_mut() {
            m.link_up = up;
        }
    }

    /// Promote to primary (`REPLICAOF NO ONE`). Fresh replid, keep offset.
    pub fn promote(&self) {
        *self.role.write() = Role::Primary;
        *self.master.write() = None;
        *self.replid.write() = gen_replid();
    }
}

/// Generate a 40-char lowercase hex run id.
pub fn gen_replid() -> String {
    let mut s = String::with_capacity(40);
    for _ in 0..40 {
        let nibble = fastrand::u8(0..16);
        s.push(char::from_digit(nibble as u32, 16).unwrap());
    }
    s
}

/// Serialize a command as a RESP array frame (same wire shape as the AOF).
pub fn encode_command(command: &[Bytes]) -> Bytes {
    let mut buf = Vec::with_capacity(16 + command.iter().map(|p| p.len() + 16).sum::<usize>());
    buf.extend_from_slice(format!("*{}\r\n", command.len()).as_bytes());
    for part in command {
        buf.extend_from_slice(format!("${}\r\n", part.len()).as_bytes());
        buf.extend_from_slice(part);
        buf.extend_from_slice(b"\r\n");
    }
    Bytes::from(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replid_is_40_hex_chars() {
        let id = gen_replid();
        assert_eq!(id.len(), 40);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn encode_command_produces_resp_array() {
        let cmd = vec![
            Bytes::from_static(b"SET"),
            Bytes::from_static(b"k"),
            Bytes::from_static(b"v"),
        ];
        let out = encode_command(&cmd);
        assert_eq!(&out[..], b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$1\r\nv\r\n");
    }

    #[test]
    fn new_replication_is_primary_with_zero_offset() {
        let r = Replication::new(16);
        assert!(matches!(r.role(), Role::Primary));
        assert_eq!(r.offset(), 0);
        assert_eq!(r.replid().len(), 40);
    }

    #[test]
    fn feed_advances_offset() {
        let r = Replication::new(16);
        // Subscribe so the frame is actually buffered (and to mimic a replica).
        let _rx = r.subscribe();
        r.feed(&[
            Bytes::from_static(b"SET"),
            Bytes::from_static(b"k"),
            Bytes::from_static(b"v"),
        ]);
        assert!(r.offset() > 0);
    }
}
