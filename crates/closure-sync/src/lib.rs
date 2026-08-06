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

/// A fresh signing identity from OS entropy.
///
/// Key generation lives here rather than in a shell because this crate
/// already owns the crypto: one place decides what a closure identity
/// is made of, and shells never touch an RNG.
#[must_use]
pub fn generate_key() -> SigningKey {
    SigningKey::generate(&mut rand_core::OsRng)
}

/// A content identifier (V5a): a stable, dep-free address for a blob,
/// derived from its bytes. Identical content always yields an equal
/// `Cid` (dedup + verify-on-read); textual form is deterministic (I6).
///
/// Cryptographic (D2): a 256-bit BLAKE3 digest, prefixed `b3`. Collision
/// resistance lets the `Cid` double as an integrity check — a tampered or
/// truncated blob hashes to a different `Cid` and is rejected on read. The
/// value is an opaque string, so swapping the algorithm again later needs
/// no API change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cid(String);

impl Cid {
    /// The content id of `bytes`: `b3` + the 64-hex BLAKE3 digest.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(format!("b3{}", blake3::hash(bytes).to_hex()))
    }

    /// The textual content id (stable; usable as a key / on the wire).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reconstruct a [`Cid`] from its textual form (e.g. a filename or a
    /// value received from a peer). The content is verified separately.
    #[must_use]
    pub const fn from_raw(s: String) -> Self {
        Self(s)
    }
}

/// A pluggable content-addressed store (V5b): blobs addressed by [`Cid`].
///
/// In-memory ([`BlockStore`]) and filesystem ([`FsBlockStore`]) impls
/// ship today; an IPFS/iroh network provider is a future impl behind the
/// same trait (kept external/feature-gated so the core stays hermetic).
pub trait BlockProvider {
    /// Whether `cid` is present.
    fn has(&self, cid: &Cid) -> bool;
    /// The bytes for `cid`, if present.
    fn get(&self, cid: &Cid) -> Option<Vec<u8>>;
    /// Store `content`, returning its [`Cid`] (idempotent).
    fn put(&mut self, content: &[u8]) -> Cid;
    /// Every stored [`Cid`].
    fn cids(&self) -> Vec<Cid>;
}

impl BlockProvider for BlockStore {
    fn has(&self, cid: &Cid) -> bool {
        self.has(cid)
    }
    fn get(&self, cid: &Cid) -> Option<Vec<u8>> {
        self.get(cid).map(<[u8]>::to_vec)
    }
    fn put(&mut self, content: &[u8]) -> Cid {
        self.put(content)
    }
    fn cids(&self) -> Vec<Cid> {
        self.blobs.keys().cloned().collect()
    }
}

/// A filesystem-backed content-addressed store (V5b): each blob is a file
/// named by its [`Cid`] under a directory. Persistent across processes.
#[derive(Debug, Clone)]
pub struct FsBlockStore {
    dir: PathBuf,
}

impl FsBlockStore {
    /// Open (or use) the store rooted at `dir`.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
}

impl BlockProvider for FsBlockStore {
    fn has(&self, cid: &Cid) -> bool {
        self.dir.join(cid.as_str()).is_file()
    }
    fn get(&self, cid: &Cid) -> Option<Vec<u8>> {
        std::fs::read(self.dir.join(cid.as_str())).ok()
    }
    fn put(&mut self, content: &[u8]) -> Cid {
        let cid = Cid::of(content);
        let path = self.dir.join(cid.as_str());
        if !path.exists() {
            let _ = std::fs::create_dir_all(&self.dir);
            let _ = std::fs::write(&path, content);
        }
        cid
    }
    fn cids(&self) -> Vec<Cid> {
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .map(Cid::from_raw)
            .collect()
    }
}

/// Exchange blobs between two providers so both converge to the union of
/// their content (V5b): every blob one side has and the other lacks is
/// copied across. Returns the number of blobs transferred.
///
/// Content addressing makes this safe + order-independent — a received
/// blob is re-`put` (re-hashed), so a corrupt transfer lands under a
/// different `Cid` and is detectable.
pub fn sync_providers<A: BlockProvider, B: BlockProvider>(a: &mut A, b: &mut B) -> usize {
    let mut moved = 0;
    for cid in a.cids() {
        if !b.has(&cid)
            && let Some(content) = a.get(&cid)
        {
            b.put(&content);
            moved += 1;
        }
    }
    for cid in b.cids() {
        if !a.has(&cid)
            && let Some(content) = b.get(&cid)
        {
            a.put(&content);
            moved += 1;
        }
    }
    moved
}

/// A content-addressed block store (V5a): blobs keyed by their [`Cid`].
///
/// `put` is idempotent (identical content dedups to one entry); `get`
/// returns the stored bytes; `verify` re-hashes a stored blob to confirm
/// it still matches its key (corruption / tamper detection). In-memory +
/// pure; the foundation for content-addressed sync (V5b).
#[derive(Debug, Clone, Default)]
pub struct BlockStore {
    blobs: std::collections::HashMap<Cid, Vec<u8>>,
}

impl BlockStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Store `content`, returning its [`Cid`]. Idempotent.
    pub fn put(&mut self, content: &[u8]) -> Cid {
        let cid = Cid::of(content);
        self.blobs
            .entry(cid.clone())
            .or_insert_with(|| content.to_vec());
        cid
    }

    /// The bytes stored under `cid`, if any.
    #[must_use]
    pub fn get(&self, cid: &Cid) -> Option<&[u8]> {
        self.blobs.get(cid).map(Vec::as_slice)
    }

    /// Whether `cid` is present.
    #[must_use]
    pub fn has(&self, cid: &Cid) -> bool {
        self.blobs.contains_key(cid)
    }

    /// Number of stored blobs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// Insert `content` under an explicit `cid` without re-hashing — used
    /// to receive a blob from a peer (verified separately) or, in tests,
    /// to inject a mismatched blob.
    pub fn insert_raw(&mut self, cid: Cid, content: Vec<u8>) {
        self.blobs.insert(cid, content);
    }

    /// Re-hash the blob under `cid` and confirm it matches the key.
    /// `false` for a missing or tampered blob.
    #[must_use]
    pub fn verify(&self, cid: &Cid) -> bool {
        self.blobs.get(cid).is_some_and(|b| &Cid::of(b) == cid)
    }
}

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

    /// Merge a peer's replica, surfacing every concurrent title
    /// divergence the automatic LWW resolved (Q3 —
    /// [`Replica::merge_with_conflicts`]); the report feeds a shell's
    /// resolution surface (`ConflictApp`).
    pub fn receive_with_conflicts(&mut self, other: &Replica) -> Vec<closure_crdt::FieldConflict> {
        self.replica.merge_with_conflicts(other)
    }

    /// Merge a received [`SyncMessage`] into this session.
    pub fn apply_message(&mut self, msg: &SyncMessage) {
        self.replica.merge(msg.replica());
    }

    /// Merge a received [`SyncMessage`], reporting every divergence the
    /// automatic LWW would otherwise have resolved silently.
    ///
    /// The message-shaped half of [`Self::receive_with_conflicts`] —
    /// what a transport that carries frames rather than sessions needs,
    /// which is the disk-file courier as well as the socket.
    pub fn receive_message_with_conflicts(
        &mut self,
        msg: &SyncMessage,
    ) -> Vec<closure_crdt::FieldConflict> {
        self.replica.merge_with_conflicts(msg.replica())
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
    /// The Noise handshake hash: a value unique to *this* conversation
    /// and known to both ends of it. What [`Self::authenticate`] signs,
    /// and the reason a signature from another conversation is not a
    /// proof here.
    handshake_hash: Vec<u8>,
}

/// Domain separator for the channel-binding signature.
///
/// A signature is only meaningful against the thing it was meant to
/// sign. Without this, a proof-of-channel could be replayed as a frame
/// signature or the other way about.
const CHANNEL_PROOF_DOMAIN: &[u8] = b"closure-sync channel proof v1\n";

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
        let (ini_h, resp_h) = (
            ini.get_handshake_hash().to_vec(),
            resp.get_handshake_hash().to_vec(),
        );
        let ini_t = ini.into_transport_mode().map_err(noise_err)?;
        let resp_t = resp.into_transport_mode().map_err(noise_err)?;
        Ok((
            Self {
                transport: ini_t,
                handshake_hash: ini_h,
            },
            Self {
                transport: resp_t,
                handshake_hash: resp_h,
            },
        ))
    }

    /// Encrypt `plaintext` into as many AEAD frames as it takes.
    ///
    /// A Noise transport message holds 64 KiB minus its tag, and a
    /// replica is one payload — so a vault past that size failed to
    /// push at all, which is what "the vault seems to be too big"
    /// looked like from the outside. The chunks are sequential messages
    /// on the same channel, so the nonce sequence keeps them ordered
    /// and the receiver rejects a reordered or replayed one.
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] if the cipher fails.
    pub fn encrypt_chunks(&mut self, plaintext: &[u8]) -> Result<Vec<Vec<u8>>, SyncError> {
        plaintext
            .chunks(NOISE_MAX_PLAINTEXT)
            .map(|chunk| self.encrypt(chunk))
            .collect()
    }

    /// Decrypt what [`Self::encrypt_chunks`] produced, back into one
    /// payload.
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] if any frame fails to decrypt — a
    /// missing or reordered chunk included.
    pub fn decrypt_chunks(&mut self, frames: &[Vec<u8>]) -> Result<Vec<u8>, SyncError> {
        let mut out = Vec::new();
        for frame in frames {
            out.extend_from_slice(&self.decrypt(frame)?);
        }
        Ok(out)
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
        let handshake_hash = hs.get_handshake_hash().to_vec();
        Ok(Self {
            transport: hs.into_transport_mode().map_err(noise_err)?,
            handshake_hash,
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
        let handshake_hash = hs.get_handshake_hash().to_vec();
        Ok(Self {
            transport: hs.into_transport_mode().map_err(noise_err)?,
            handshake_hash,
        })
    }
}

impl NoiseChannel {
    /// This conversation's Noise handshake hash.
    #[must_use]
    pub fn handshake_hash(&self) -> &[u8] {
        &self.handshake_hash
    }

    /// Prove to the peer that we hold `ours`, and check that it holds
    /// `theirs` — over *this* channel.
    ///
    /// The gap this closes: `Noise_NN` agrees a key with whoever
    /// answers, so it is confidentiality against a listener and
    /// nothing against somebody in the path. An attacker there runs
    /// one handshake with each side and holds two channels it can read
    /// and rewrite. The frames inside are signed, which stops it
    /// forging a replica — and does not stop it reading every one,
    /// dropping the ones it dislikes, or replaying old ones. Signing
    /// the payload says who wrote the bytes; it does not say who you
    /// are talking to.
    ///
    /// So each side signs the handshake hash and sends the signature
    /// *through* the encrypted channel. Somebody in the middle holds
    /// two channels with two different hashes and cannot produce the
    /// peer's signature over the one you are on.
    ///
    /// Chosen over `Noise_XX` with the ticket's key because that key is
    /// ed25519 and XX wants X25519: reusing one key for both signing
    /// and key agreement is a thing ed25519-dalek warns against in as
    /// many words, and a separate static key would mean a new ticket
    /// format and every paired peer re-pairing. This needs neither.
    ///
    /// `we_speak_first` orders the exchange; the two ends must
    /// disagree about it, exactly as they do for the handshake itself.
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] when the peer's proof is missing,
    /// malformed, or not a signature by `theirs` over this channel —
    /// which is what an attacker in the middle looks like from here.
    pub fn authenticate<S: std::io::Read + std::io::Write>(
        &mut self,
        stream: &mut S,
        ours: &SigningKey,
        theirs: &VerifyingKey,
        we_speak_first: bool,
    ) -> Result<(), SyncError> {
        self.authenticate_any(stream, ours, std::slice::from_ref(theirs), we_speak_first)
            .map(|_| ())
    }

    /// [`Self::authenticate`] against a set, returning which of them
    /// answered.
    ///
    /// The listening side needs this: it does not know who is dialling
    /// in until they have proved it, and "one of the peers I have
    /// paired with" is the honest question there. An empty set accepts
    /// nobody — a listener that trusts no keys has nobody to sync
    /// with, and accepting anyone would be the bug this change is
    /// about.
    ///
    /// # Errors
    ///
    /// As [`Self::authenticate`].
    pub fn authenticate_any<S: std::io::Read + std::io::Write>(
        &mut self,
        stream: &mut S,
        ours: &SigningKey,
        trusted: &[VerifyingKey],
        we_speak_first: bool,
    ) -> Result<VerifyingKey, SyncError> {
        use ed25519_dalek::{Signer as _, Verifier as _};
        let mut proof = CHANNEL_PROOF_DOMAIN.to_vec();
        proof.extend_from_slice(&self.handshake_hash);
        let ours_sig = ours.sign(&proof).to_bytes().to_vec();

        let theirs_sig = if we_speak_first {
            write_chunked(stream, &self.encrypt_chunks(&ours_sig)?)?;
            self.decrypt_chunks(&read_chunked(stream)?)?
        } else {
            let theirs = self.decrypt_chunks(&read_chunked(stream)?)?;
            write_chunked(stream, &self.encrypt_chunks(&ours_sig)?)?;
            theirs
        };

        let bytes: [u8; 64] = theirs_sig
            .as_slice()
            .try_into()
            .map_err(|_| SyncError::Transport("peer sent no usable proof of who it is".into()))?;
        let signature = ed25519_dalek::Signature::from_bytes(&bytes);
        trusted
            .iter()
            .find(|key| key.verify(&proof, &signature).is_ok())
            .copied()
            .ok_or_else(|| {
                SyncError::Transport(
                    "untrusted peer on this connection: not one this vault has paired \
                     with — somebody is in the middle, or the address now answers to \
                     somebody else"
                        .into(),
                )
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

/// Write a chunked, encrypted payload: the chunk count, then each
/// length-prefixed frame.
///
/// A replica bigger than one Noise message used to fail outright; it
/// takes as many frames as it needs now, and the count tells the reader
/// how many to expect.
fn write_chunked<W: std::io::Write>(w: &mut W, frames: &[Vec<u8>]) -> Result<(), SyncError> {
    let count =
        u32::try_from(frames.len()).map_err(|_| SyncError::Transport("too many frames".into()))?;
    w.write_all(&count.to_le_bytes())
        .map_err(|e| SyncError::Io(e.to_string()))?;
    for frame in frames {
        write_framed(w, frame)?;
    }
    Ok(())
}

/// Read what [`write_chunked`] wrote.
fn read_chunked<R: std::io::Read>(r: &mut R) -> Result<Vec<Vec<u8>>, SyncError> {
    /// A sane ceiling: a peer claiming millions of frames is not a peer
    /// to allocate for (I5).
    const MAX_FRAMES: u32 = 100_000;
    let mut count_buf = [0u8; 4];
    r.read_exact(&mut count_buf)
        .map_err(|e| SyncError::Io(e.to_string()))?;
    let count = u32::from_le_bytes(count_buf);
    if count > MAX_FRAMES {
        return Err(SyncError::Transport(format!(
            "peer announced {count} frames, which is not a sync"
        )));
    }
    (0..count).map(|_| read_framed(r)).collect()
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

    /// One live round on an already-open stream, client side (Q11-C1):
    /// send our replica frame, merge the peer's. Called repeatedly on
    /// ONE connection, this is the continuous-session loop — each
    /// `record_local` between rounds flows to the peer on the next
    /// round.
    ///
    /// # Errors
    ///
    /// [`SyncError`] on IO or a malformed frame.
    pub fn stream_round_client(
        stream: &mut std::net::TcpStream,
        session: &mut SyncSession,
    ) -> Result<(), SyncError> {
        write_framed(stream, &SyncMessage::from_session(session).to_bytes())?;
        let theirs = SyncMessage::from_bytes(&read_framed(stream)?)?;
        session.apply_message(&theirs);
        Ok(())
    }

    /// The responder counterpart to [`Self::stream_round_client`].
    ///
    /// # Errors
    ///
    /// [`SyncError`] on IO or a malformed frame.
    pub fn stream_round_server(
        stream: &mut std::net::TcpStream,
        session: &mut SyncSession,
    ) -> Result<(), SyncError> {
        let theirs = SyncMessage::from_bytes(&read_framed(stream)?)?;
        session.apply_message(&theirs);
        write_framed(stream, &SyncMessage::from_session(session).to_bytes())?;
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
        // Before a byte of the vault goes over: prove who we are and
        // check who they are, bound to *this* channel. Without it the
        // handshake agrees a key with whoever answered, and a signed
        // frame proves only who wrote it — not who is reading it.
        chan.authenticate_any(&mut stream, signing_key, trusted, true)?;
        let frame = SyncMessage::from_session(session).to_signed_bytes(signing_key);
        write_chunked(&mut stream, &chan.encrypt_chunks(&frame)?)?;
        let ct = read_chunked(&mut stream)?;
        let theirs = SyncMessage::from_signed_bytes(&chan.decrypt_chunks(&ct)?, trusted)?;
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
        chan.authenticate_any(&mut stream, signing_key, trusted, false)?;
        let ct = read_chunked(&mut stream)?;
        let theirs = SyncMessage::from_signed_bytes(&chan.decrypt_chunks(&ct)?, trusted)?;
        session.apply_message(&theirs);
        let frame = SyncMessage::from_session(session).to_signed_bytes(signing_key);
        write_chunked(&mut stream, &chan.encrypt_chunks(&frame)?)?;
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

/// Ephemeral presence (Q11-C2): which block a peer is on and where.
///
/// Presence is session chatter, not document state — it is NEVER
/// persisted, never enters the undo tree, and carries its own wire
/// magic (`CLPR`) so [`SyncMessage::from_bytes`] rejects it instead of
/// merging cursor movement into a replica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    /// Peer name.
    pub peer: String,
    /// Focused block id (string form).
    pub block: String,
    /// Cursor line inside the block body.
    pub line: u32,
}

impl Presence {
    /// Frame as `CLPR | 1 | len peer | peer | len block | block | line`.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"CLPR");
        out.push(1);
        let put = |out: &mut Vec<u8>, s: &str| {
            out.extend_from_slice(&u32::try_from(s.len()).unwrap_or(u32::MAX).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        };
        put(&mut out, &self.peer);
        put(&mut out, &self.block);
        out.extend_from_slice(&self.line.to_le_bytes());
        out
    }

    /// Parse a frame produced by [`Self::encode`].
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] on a short buffer, wrong magic /
    /// version, or malformed strings (never a panic, I5).
    pub fn decode(bytes: &[u8]) -> Result<Self, SyncError> {
        let bad = || SyncError::Transport("not a presence frame".into());
        if bytes.len() < 5 || &bytes[..4] != b"CLPR" || bytes[4] != 1 {
            return Err(bad());
        }
        let mut pos = 5usize;
        let mut take_str = |bytes: &[u8]| -> Result<String, SyncError> {
            let len_bytes: [u8; 4] = bytes
                .get(pos..pos + 4)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?;
            let len = u32::from_le_bytes(len_bytes) as usize;
            pos += 4;
            let s = bytes.get(pos..pos + len).ok_or_else(bad)?;
            pos += len;
            String::from_utf8(s.to_vec()).map_err(|_| bad())
        };
        let peer = take_str(bytes)?;
        let block = take_str(bytes)?;
        let line_bytes: [u8; 4] = bytes
            .get(pos..pos + 4)
            .ok_or_else(bad)?
            .try_into()
            .map_err(|_| bad())?;
        Ok(Self {
            peer,
            block,
            line: u32::from_le_bytes(line_bytes),
        })
    }
}

/// A plain-text pairing artifact (Q10).
///
/// Where to connect and WHO must sign —
/// `closure-sync:<addr>|<hex ed25519 verifying key>`, one line,
/// storable in a vault org file. `join` derives its C3a trusted set
/// from the ticket, so authenticity is pinned at pairing time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncTicket {
    /// Socket address the peer listens on.
    pub addr: std::net::SocketAddr,
    /// The peer's ed25519 verifying key (its frames must sign with it).
    pub pubkey: VerifyingKey,
}

impl SyncTicket {
    /// Render as the single-line plain-text ticket.
    #[must_use]
    pub fn encode(&self) -> String {
        use std::fmt::Write as _;
        let mut hex = String::with_capacity(64);
        for b in self.pubkey.to_bytes() {
            let _ = write!(hex, "{b:02x}");
        }
        format!("closure-sync:{}|{hex}", self.addr)
    }

    /// Parse a ticket produced by [`Self::encode`].
    ///
    /// # Errors
    ///
    /// [`SyncError::Transport`] on a missing prefix, malformed
    /// address, or an invalid key (never a panic, I5).
    pub fn decode(s: &str) -> Result<Self, SyncError> {
        let rest = s
            .trim()
            .strip_prefix("closure-sync:")
            .ok_or_else(|| SyncError::Transport("not a closure-sync ticket".into()))?;
        let (addr_s, hex) = rest
            .rsplit_once('|')
            .ok_or_else(|| SyncError::Transport("ticket missing key part".into()))?;
        let addr: std::net::SocketAddr = addr_s
            .parse()
            .map_err(|_| SyncError::Transport(format!("bad ticket address: {addr_s}")))?;
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(SyncError::Transport("bad ticket key".into()));
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(chunk)
                .map_err(|_| SyncError::Transport("bad ticket key".into()))?;
            bytes[i] = u8::from_str_radix(s, 16)
                .map_err(|_| SyncError::Transport("bad ticket key".into()))?;
        }
        let pubkey = VerifyingKey::from_bytes(&bytes)
            .map_err(|_| SyncError::Transport("invalid ed25519 key in ticket".into()))?;
        Ok(Self { addr, pubkey })
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
