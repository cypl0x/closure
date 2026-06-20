//! Sync transports for closure vaults.
//!
//! Transports implement [`Transport`] and ship edits that a CRDT merger
//! consumes. Phase 1 transports: plain file copy and git. Later phases:
//! IPFS / iroh for P2P collaboration.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::Command;

use closure_core::{BlockId, Document};
use closure_crdt::{CrdtError, Replica, VectorClock};
use thiserror::Error;

pub use ed25519_dalek::{SigningKey, VerifyingKey};

/// A peer's CRDT sync state: an accumulated [`Replica`] + [`VectorClock`].
///
/// Local edits are folded in via [`Self::record_local`]; a peer's
/// replica is merged in via [`Self::receive`]. Merges are commutative +
/// idempotent (LWW per block field), so any exchange order converges.
/// Transport-agnostic — the loopback and iroh transports both just move
/// the [`Replica`].
#[derive(Debug, Clone, Default)]
pub struct SyncSession {
    replica: Replica,
    clock: VectorClock,
    name: String,
}

impl SyncSession {
    /// New session for replica `name` (the clock's replica id).
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            replica: Replica::default(),
            clock: VectorClock::new(name),
            name: name.to_owned(),
        }
    }

    /// Fold a local snapshot of `doc` into this session. Bumps the
    /// clock and snapshots `doc` *against* the accumulated replica at
    /// the new logical time: only fields that actually changed advance
    /// their timestamp, so an untouched field never outranks a
    /// concurrent edit on another peer (the key to convergence).
    pub fn record_local(&mut self, doc: &Document) {
        self.clock.bump(&self.name);
        let ts = self.clock.logical_time();
        let snap = Replica::snapshot_against(&self.replica, doc, ts, &self.name);
        self.replica.merge(&snap);
    }

    /// The replica to ship to a peer.
    #[must_use]
    pub const fn outgoing(&self) -> &Replica {
        &self.replica
    }

    /// Merge a peer's replica into this session (LWW per block field).
    pub fn receive(&mut self, other: &Replica) {
        self.replica.merge(other);
    }

    /// Merge a received [`SyncMessage`] into this session.
    pub fn apply_message(&mut self, msg: &SyncMessage) {
        self.replica.merge(msg.replica());
    }

    /// Winning title for the block whose id is `id` (string form), for
    /// callers that hold the id as text rather than a [`BlockId`].
    #[must_use]
    pub fn title_of_str(&self, id: &str) -> Option<String> {
        self.replica
            .title_of(&BlockId::from_existing(id))
            .map(ToOwned::to_owned)
    }

    /// Block ids known to this session.
    pub fn block_ids(&self) -> impl Iterator<Item = &BlockId> {
        self.replica.block_ids()
    }

    /// Winning title for `id`.
    #[must_use]
    pub fn title_of(&self, id: &BlockId) -> Option<&str> {
        self.replica.title_of(id)
    }

    /// Converged body text for `id` (materialised from the body RGA).
    #[must_use]
    pub fn body_of(&self, id: &BlockId) -> Option<String> {
        self.replica.body_of(id)
    }

    /// Reconcile `doc` to this session's winning registers through
    /// kernel commands (I8). Returns the number of edits applied.
    ///
    /// # Errors
    ///
    /// [`CrdtError`] when a kernel command refuses an edit.
    pub fn apply_to(&self, doc: &mut Document) -> Result<usize, CrdtError> {
        self.replica.apply_to(doc)
    }
}

/// Wire magic for a [`SyncMessage`] frame.
const SYNC_MAGIC: &[u8; 4] = b"CLSY";
/// Current (unsigned) sync wire-protocol version.
const SYNC_VERSION: u8 = 1;
/// Authenticated frame version: `magic | 2 | pubkey(32) | sig(64) | replica`.
const SYNC_VERSION_SIGNED: u8 = 2;
/// ed25519 public-key length.
const PUBKEY_LEN: usize = 32;
/// ed25519 signature length.
const SIG_LEN: usize = 64;

/// A framed sync message a transport ships.
///
/// A 4-byte magic + 1-byte version header wrapping an encoded
/// [`Replica`] (see [`Replica::encode`]). Transport-agnostic — the
/// loopback and iroh transports both move these bytes.
#[derive(Debug, Clone)]
pub struct SyncMessage {
    replica: Replica,
}

impl SyncMessage {
    /// Build a message carrying `session`'s current replica.
    #[must_use]
    pub fn from_session(session: &SyncSession) -> Self {
        Self {
            replica: session.outgoing().clone(),
        }
    }

    /// Serialise to the framed wire bytes (`magic | version | replica`).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5);
        out.extend_from_slice(SYNC_MAGIC);
        out.push(SYNC_VERSION);
        out.extend_from_slice(&self.replica.encode());
        out
    }

    /// Parse framed wire bytes back into a message.
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] on a short buffer, a bad magic, an
    /// unsupported version, or a malformed replica payload.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SyncError> {
        let header = bytes
            .get(..5)
            .ok_or_else(|| SyncError::Transport("sync message too short".into()))?;
        if &header[..4] != SYNC_MAGIC {
            return Err(SyncError::Transport("bad sync magic".into()));
        }
        if header[4] != SYNC_VERSION {
            return Err(SyncError::Transport(format!(
                "unsupported sync version {}",
                header[4]
            )));
        }
        let replica =
            Replica::decode(&bytes[5..]).map_err(|e| SyncError::Transport(e.to_string()))?;
        Ok(Self { replica })
    }

    /// The carried replica.
    #[must_use]
    pub const fn replica(&self) -> &Replica {
        &self.replica
    }

    /// Serialise to an authenticated frame signed by `key` (C3a):
    /// `magic | 2 | pubkey | signature | replica`. The signature covers
    /// the version byte and the encoded replica, so neither the payload
    /// nor a version downgrade can be tampered with undetected.
    #[must_use]
    pub fn to_signed_bytes(&self, key: &SigningKey) -> Vec<u8> {
        use ed25519_dalek::Signer as _;
        let payload = self.replica.encode();
        let mut signed = Vec::with_capacity(1 + payload.len());
        signed.push(SYNC_VERSION_SIGNED);
        signed.extend_from_slice(&payload);
        let sig = key.sign(&signed);

        let mut out = Vec::with_capacity(4 + 1 + PUBKEY_LEN + SIG_LEN + payload.len());
        out.extend_from_slice(SYNC_MAGIC);
        out.push(SYNC_VERSION_SIGNED);
        out.extend_from_slice(key.verifying_key().as_bytes());
        out.extend_from_slice(&sig.to_bytes());
        out.extend_from_slice(&payload);
        out
    }

    /// Parse + verify an authenticated frame (C3a). The embedded
    /// signature is checked against the embedded public key (rejecting
    /// any tampering); when `trusted` is non-empty the signer must be
    /// one of those keys (rejecting an unknown/forged peer). An empty
    /// `trusted` is integrity-only mode (any self-consistent signature).
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] on a short/garbled frame, a wrong magic
    /// or version, a bad signature, an untrusted signer, or a malformed
    /// replica payload.
    pub fn from_signed_bytes(bytes: &[u8], trusted: &[VerifyingKey]) -> Result<Self, SyncError> {
        use ed25519_dalek::Verifier as _;
        const HEAD: usize = 4 + 1 + PUBKEY_LEN + SIG_LEN;
        let head = bytes
            .get(..HEAD)
            .ok_or_else(|| SyncError::Transport("signed frame too short".into()))?;
        if &head[..4] != SYNC_MAGIC {
            return Err(SyncError::Transport("bad sync magic".into()));
        }
        if head[4] != SYNC_VERSION_SIGNED {
            return Err(SyncError::Transport(format!(
                "not a signed frame (version {})",
                head[4]
            )));
        }
        // Fixed-length copies from the validated HEAD slice (no panic
        // path: lengths are guaranteed by the `get(..HEAD)` check above).
        let mut pk = [0u8; PUBKEY_LEN];
        pk.copy_from_slice(&head[5..5 + PUBKEY_LEN]);
        let mut sig = [0u8; SIG_LEN];
        sig.copy_from_slice(&head[5 + PUBKEY_LEN..HEAD]);
        let vk = VerifyingKey::from_bytes(&pk)
            .map_err(|e| SyncError::Transport(format!("bad public key: {e}")))?;
        let signature = ed25519_dalek::Signature::from_bytes(&sig);
        let payload = &bytes[HEAD..];

        // Signature covers `version | payload` (see `to_signed_bytes`).
        let mut signed = Vec::with_capacity(1 + payload.len());
        signed.push(SYNC_VERSION_SIGNED);
        signed.extend_from_slice(payload);
        vk.verify(&signed, &signature)
            .map_err(|_| SyncError::Transport("signature verification failed".into()))?;

        if !trusted.is_empty() && !trusted.contains(&vk) {
            return Err(SyncError::Transport("untrusted signing key".into()));
        }
        let replica = Replica::decode(payload).map_err(|e| SyncError::Transport(e.to_string()))?;
        Ok(Self { replica })
    }
}

/// Noise protocol name for the transport channel (C3b): ephemeral-only
/// (NN) key agreement — confidentiality on the wire; peer *authenticity*
/// is provided independently by the C3a frame signatures carried inside.
const NOISE_PARAMS: &str = "Noise_NN_25519_ChaChaPoly_BLAKE2s";
/// Max plaintext per Noise transport message (64 KiB frame − 16-byte tag).
const NOISE_MAX_PLAINTEXT: usize = 65535 - 16;

/// An established encrypted transport channel (C3b).
///
/// After a Noise NN handshake the two endpoints hold paired
/// [`NoiseChannel`]s; [`Self::encrypt`] / [`Self::decrypt`] move
/// AEAD-protected frames so a [`SyncMessage`]'s replica never crosses
/// the wire in plaintext. Transport-agnostic — wraps any byte stream.
pub struct NoiseChannel {
    transport: snow::TransportState,
}

impl NoiseChannel {
    /// Perform an in-process NN handshake and return the
    /// `(initiator, responder)` channel pair. The hermetic stand-in for
    /// a handshake driven over a real socket (which exchanges the same
    /// two messages); also the building block a TCP transport reuses.
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] on any handshake failure.
    pub fn pair() -> Result<(Self, Self), SyncError> {
        let params: snow::params::NoiseParams = NOISE_PARAMS.parse().map_err(noise_err)?;
        let mut ini = snow::Builder::new(params.clone())
            .build_initiator()
            .map_err(noise_err)?;
        let mut resp = snow::Builder::new(params)
            .build_responder()
            .map_err(noise_err)?;
        let mut buf = vec![0u8; 65535];
        let mut scratch = vec![0u8; 65535];
        // -> e
        let n = ini.write_message(&[], &mut buf).map_err(noise_err)?;
        resp.read_message(&buf[..n], &mut scratch)
            .map_err(noise_err)?;
        // <- e, ee
        let n = resp.write_message(&[], &mut buf).map_err(noise_err)?;
        ini.read_message(&buf[..n], &mut scratch)
            .map_err(noise_err)?;
        let ini_t = ini.into_transport_mode().map_err(noise_err)?;
        let resp_t = resp.into_transport_mode().map_err(noise_err)?;
        Ok((Self { transport: ini_t }, Self { transport: resp_t }))
    }

    /// Encrypt `plaintext` into one AEAD frame.
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] if the plaintext exceeds a single Noise
    /// frame or the cipher fails.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, SyncError> {
        if plaintext.len() > NOISE_MAX_PLAINTEXT {
            return Err(SyncError::Transport(
                "payload too large for a single noise frame".into(),
            ));
        }
        let mut buf = vec![0u8; plaintext.len() + 16];
        let n = self
            .transport
            .write_message(plaintext, &mut buf)
            .map_err(noise_err)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Decrypt one AEAD frame, rejecting any tampering (bad tag).
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] if authentication or decryption fails.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, SyncError> {
        let mut buf = vec![0u8; ciphertext.len()];
        let n = self
            .transport
            .read_message(ciphertext, &mut buf)
            .map_err(noise_err)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Drive the NN handshake as the initiator over a byte stream
    /// (length-framed handshake messages), returning the established
    /// channel. The socket-driven counterpart to [`Self::pair`].
    ///
    /// # Errors
    ///
    /// [`SyncError`] on IO or handshake failure.
    pub fn handshake_initiator<S: std::io::Read + std::io::Write>(
        stream: &mut S,
    ) -> Result<Self, SyncError> {
        let params: snow::params::NoiseParams = NOISE_PARAMS.parse().map_err(noise_err)?;
        let mut hs = snow::Builder::new(params)
            .build_initiator()
            .map_err(noise_err)?;
        let mut buf = vec![0u8; 65535];
        let mut scratch = vec![0u8; 65535];
        let n = hs.write_message(&[], &mut buf).map_err(noise_err)?;
        write_framed(stream, &buf[..n])?;
        let reply = read_framed(stream)?;
        hs.read_message(&reply, &mut scratch).map_err(noise_err)?;
        Ok(Self {
            transport: hs.into_transport_mode().map_err(noise_err)?,
        })
    }

    /// Drive the NN handshake as the responder over a byte stream.
    ///
    /// # Errors
    ///
    /// [`SyncError`] on IO or handshake failure.
    pub fn handshake_responder<S: std::io::Read + std::io::Write>(
        stream: &mut S,
    ) -> Result<Self, SyncError> {
        let params: snow::params::NoiseParams = NOISE_PARAMS.parse().map_err(noise_err)?;
        let mut hs = snow::Builder::new(params)
            .build_responder()
            .map_err(noise_err)?;
        let mut buf = vec![0u8; 65535];
        let mut scratch = vec![0u8; 65535];
        let msg1 = read_framed(stream)?;
        hs.read_message(&msg1, &mut scratch).map_err(noise_err)?;
        let n = hs.write_message(&[], &mut buf).map_err(noise_err)?;
        write_framed(stream, &buf[..n])?;
        Ok(Self {
            transport: hs.into_transport_mode().map_err(noise_err)?,
        })
    }
}

fn noise_err<E: std::fmt::Display>(e: E) -> SyncError {
    SyncError::Transport(format!("noise: {e}"))
}

/// Write a `u32`-length-prefixed frame to any writer.
fn write_framed<W: std::io::Write>(w: &mut W, bytes: &[u8]) -> Result<(), SyncError> {
    let len =
        u32::try_from(bytes.len()).map_err(|_| SyncError::Transport("frame too large".into()))?;
    w.write_all(&len.to_le_bytes())
        .and_then(|()| w.write_all(bytes))
        .map_err(|e| SyncError::Io(e.to_string()))
}

/// Read a `u32`-length-prefixed frame from any reader.
fn read_framed<R: std::io::Read>(r: &mut R) -> Result<Vec<u8>, SyncError> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)
        .map_err(|e| SyncError::Io(e.to_string()))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)
        .map_err(|e| SyncError::Io(e.to_string()))?;
    Ok(buf)
}

/// In-memory loopback link between two [`SyncSession`] peers.
///
/// A shared pair of one-shot mailboxes carrying a [`Replica`] each way
/// — the hermetic stand-in for a network link, which the iroh transport
/// mirrors over the wire instead of in process.
#[derive(Debug, Default)]
pub struct LoopbackPair {
    a_to_b: Option<Replica>,
    b_to_a: Option<Replica>,
}

impl LoopbackPair {
    /// New empty link.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Peer A publishes its replica toward B.
    pub fn push_a(&mut self, a: &SyncSession) {
        self.a_to_b = Some(a.outgoing().clone());
    }
    /// Peer B publishes its replica toward A.
    pub fn push_b(&mut self, b: &SyncSession) {
        self.b_to_a = Some(b.outgoing().clone());
    }
    /// Peer A consumes + merges whatever B published (no-op if empty).
    pub fn pull_a(&mut self, a: &mut SyncSession) {
        if let Some(r) = self.b_to_a.take() {
            a.receive(&r);
        }
    }
    /// Peer B consumes + merges whatever A published (no-op if empty).
    pub fn pull_b(&mut self, b: &mut SyncSession) {
        if let Some(r) = self.a_to_b.take() {
            b.receive(&r);
        }
    }

    /// One full sync round: both peers publish, then both consume +
    /// merge. After this, the two sessions have converged.
    pub fn sync_round(&mut self, a: &mut SyncSession, b: &mut SyncSession) {
        self.push_a(a);
        self.push_b(b);
        self.pull_a(a);
        self.pull_b(b);
    }
}

/// A real network sync transport over std TCP.
///
/// Ships length-framed [`SyncMessage`] bytes between two peers and
/// merges them into their [`SyncSession`]s, converging them. Uses only
/// `std::net` (no async runtime, no heavy deps) so it is tested
/// hermetically over `127.0.0.1` loopback. A QUIC/iroh transport is a
/// future drop-in behind the same `SyncMessage` protocol (see ROADMAP
/// Decisions).
#[derive(Debug, Clone, Copy)]
pub struct TcpSyncTransport;

impl TcpSyncTransport {
    fn write_frame(stream: &mut std::net::TcpStream, msg: &SyncMessage) -> Result<(), SyncError> {
        use std::io::Write as _;
        let bytes = msg.to_bytes();
        let len = u32::try_from(bytes.len())
            .map_err(|_| SyncError::Transport("message too large".into()))?;
        stream
            .write_all(&len.to_le_bytes())
            .and_then(|()| stream.write_all(&bytes))
            .map_err(|e| SyncError::Io(e.to_string()))
    }

    fn read_frame(stream: &mut std::net::TcpStream) -> Result<SyncMessage, SyncError> {
        use std::io::Read as _;
        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .map_err(|e| SyncError::Io(e.to_string()))?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        stream
            .read_exact(&mut buf)
            .map_err(|e| SyncError::Io(e.to_string()))?;
        SyncMessage::from_bytes(&buf)
    }

    /// Connect to a serving peer at `addr`, send our session's message,
    /// receive theirs, and merge it (a one-shot sync round, client side).
    ///
    /// # Errors
    ///
    /// [`SyncError`] on connect / IO / decode failure.
    pub fn connect_and_sync(
        addr: std::net::SocketAddr,
        session: &mut SyncSession,
    ) -> Result<(), SyncError> {
        let mut stream =
            std::net::TcpStream::connect(addr).map_err(|e| SyncError::Io(e.to_string()))?;
        Self::write_frame(&mut stream, &SyncMessage::from_session(session))?;
        let theirs = Self::read_frame(&mut stream)?;
        session.apply_message(&theirs);
        Ok(())
    }

    /// Accept one peer on `listener`, receive their message, merge it,
    /// and send ours back (a one-shot sync round, server side).
    ///
    /// # Errors
    ///
    /// [`SyncError`] on accept / IO / decode failure.
    pub fn serve_once(
        listener: &std::net::TcpListener,
        session: &mut SyncSession,
    ) -> Result<(), SyncError> {
        let (mut stream, _) = listener
            .accept()
            .map_err(|e| SyncError::Io(e.to_string()))?;
        let theirs = Self::read_frame(&mut stream)?;
        session.apply_message(&theirs);
        Self::write_frame(&mut stream, &SyncMessage::from_session(session))?;
        Ok(())
    }

    /// Secure client round (C3a+C3b): Noise-handshake the socket, then
    /// exchange ed25519-**signed** frames over the **encrypted** channel.
    /// The peer's frame is verified against `trusted` before it is
    /// merged. This is the hardened replacement for [`Self::connect_and_sync`].
    ///
    /// # Errors
    ///
    /// [`SyncError`] on connect / handshake / verify / IO failure.
    pub fn connect_and_sync_secure(
        addr: std::net::SocketAddr,
        session: &mut SyncSession,
        signing_key: &SigningKey,
        trusted: &[VerifyingKey],
    ) -> Result<(), SyncError> {
        let mut stream =
            std::net::TcpStream::connect(addr).map_err(|e| SyncError::Io(e.to_string()))?;
        let mut chan = NoiseChannel::handshake_initiator(&mut stream)?;
        let frame = SyncMessage::from_session(session).to_signed_bytes(signing_key);
        write_framed(&mut stream, &chan.encrypt(&frame)?)?;
        let ct = read_framed(&mut stream)?;
        let theirs = SyncMessage::from_signed_bytes(&chan.decrypt(&ct)?, trusted)?;
        session.apply_message(&theirs);
        Ok(())
    }

    /// Secure server round (C3a+C3b): the responder counterpart to
    /// [`Self::connect_and_sync_secure`].
    ///
    /// # Errors
    ///
    /// [`SyncError`] on accept / handshake / verify / IO failure.
    pub fn serve_once_secure(
        listener: &std::net::TcpListener,
        session: &mut SyncSession,
        signing_key: &SigningKey,
        trusted: &[VerifyingKey],
    ) -> Result<(), SyncError> {
        let (mut stream, _) = listener
            .accept()
            .map_err(|e| SyncError::Io(e.to_string()))?;
        let mut chan = NoiseChannel::handshake_responder(&mut stream)?;
        let ct = read_framed(&mut stream)?;
        let theirs = SyncMessage::from_signed_bytes(&chan.decrypt(&ct)?, trusted)?;
        session.apply_message(&theirs);
        let frame = SyncMessage::from_session(session).to_signed_bytes(signing_key);
        write_framed(&mut stream, &chan.encrypt(&frame)?)?;
        Ok(())
    }
}

/// Sync transport trait.
pub trait Transport {
    /// Send the vault state to the remote.
    fn push(&mut self) -> Result<(), SyncError>;
    /// Receive the vault state from the remote.
    fn pull(&mut self) -> Result<(), SyncError>;
}

/// Sync error.
#[derive(Debug, Error)]
pub enum SyncError {
    /// Transport-specific error.
    #[error("transport: {0}")]
    Transport(String),
    /// IO error.
    #[error("io: {0}")]
    Io(String),
}

/// No-op transport. Useful for tests and single-host setups.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopTransport;

impl Transport for NoopTransport {
    fn push(&mut self) -> Result<(), SyncError> {
        Ok(())
    }
    fn pull(&mut self) -> Result<(), SyncError> {
        Ok(())
    }
}

/// Git-based transport. The vault directory must already be a git
/// working tree with a configured remote. `push` runs
/// `git add -A && git commit -m <msg> && git push`. `pull` runs
/// `git pull --rebase`.
#[derive(Debug, Clone)]
pub struct GitTransport {
    /// Vault directory.
    pub dir: PathBuf,
    /// Branch to push (defaults to current `HEAD`).
    pub branch: Option<String>,
    /// Commit message used by [`Transport::push`].
    pub commit_message: String,
}

impl GitTransport {
    /// Build a git transport rooted at `dir`.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            branch: None,
            commit_message: "closure: sync".into(),
        }
    }

    fn run(&self, args: &[&str]) -> Result<(), SyncError> {
        let status = Command::new("git")
            .current_dir(&self.dir)
            .args(args)
            .status()
            .map_err(|e| SyncError::Io(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(SyncError::Transport(format!(
                "git {:?} exited {}",
                args,
                status.code().unwrap_or(-1)
            )))
        }
    }
}

impl Transport for GitTransport {
    fn push(&mut self) -> Result<(), SyncError> {
        self.run(&["add", "-A"])?;
        // Allow empty commits to fail silently — nothing to sync is fine.
        let _ = self.run(&["commit", "-m", &self.commit_message]);
        let branch = self.branch.clone().unwrap_or_else(|| "HEAD".into());
        self.run(&["push", "origin", &branch])
    }

    fn pull(&mut self) -> Result<(), SyncError> {
        self.run(&["pull", "--rebase"])
    }
}

/// P2P transport via external binary (iroh or IPFS CLI).
///
/// Decision (recorded per ROADMAP): use external binary (iroh/ipfs) so the core stays hermetic,
/// no heavy deps, and the presence is gated (test skips or errors if binary not in PATH, like git tests).
/// The binary is called for push/pull; for the stub, we check presence and error "not implemented / binary not found".
#[derive(Debug, Clone)]
pub struct IrohTransport {
    /// Vault directory.
    pub dir: PathBuf,
}

impl IrohTransport {
    /// Build an Iroh (or IPFS) P2P transport rooted at `dir` (external binary gated).
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    #[allow(clippy::unused_self)]
    fn binary_present(&self, name: &str) -> bool {
        Command::new(name).arg("--version").output().is_ok()
            || Command::new("which")
                .arg(name)
                .output()
                .is_ok_and(|o| o.status.success())
    }
}

impl Transport for IrohTransport {
    fn push(&mut self) -> Result<(), SyncError> {
        if !self.binary_present("iroh") && !self.binary_present("ipfs") {
            return Err(SyncError::Transport("binary not found: iroh or ipfs (decision: external for P2P, gated on presence to keep core hermetic)".to_owned()));
        }
        // Stub: the real would shell `iroh send ...` or `ipfs add ...` etc with the vault state.
        Err(SyncError::Transport("iroh/ipfs push not implemented in stub (binary present; real impl would do the P2P transfer)".to_owned()))
    }

    fn pull(&mut self) -> Result<(), SyncError> {
        if !self.binary_present("iroh") && !self.binary_present("ipfs") {
            return Err(SyncError::Transport("binary not found: iroh or ipfs (decision: external for P2P, gated on presence to keep core hermetic)".to_owned()));
        }
        Err(SyncError::Transport("iroh/ipfs pull not implemented in stub (binary present; real impl would fetch and apply deltas)".to_owned()))
    }
}
