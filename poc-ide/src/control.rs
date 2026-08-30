//! `ControlClient` Adapter and `UnixControl` Adapter. Never `$/` FilesSince.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use progressive_lsp_control::prost::Message;
use progressive_lsp_control::{
    decode_frame, encode_frame, files_since_request, DecodeOutcome, Envelope, FilesSinceRequest,
    FilesSinceResponse, GetConfigRequest, GetConfigResponse, IndexStatusRequest,
    IndexStatusResponse, InstallPacksRequest, InstallPacksResponse, ReloadConfigRequest,
    ReloadConfigResponse, ReloadScriptsRequest, ReloadScriptsResponse, SetConfigRequest,
    SetConfigResponse, TierReady, TierStatusRequest, TierStatusResponse, WatchBatch,
    WatchSubscribeRequest, WatchSubscribeResponse, MAX_PAYLOAD_BYTES, METHOD_FILES_SINCE,
    METHOD_GET_CONFIG, METHOD_INDEX_STATUS, METHOD_INSTALL_PACKS, METHOD_RELOAD_CONFIG,
    METHOD_RELOAD_SCRIPTS, METHOD_SET_CONFIG, METHOD_TIER_READY, METHOD_TIER_STATUS,
    METHOD_WATCH_BATCH, METHOD_WATCH_SUBSCRIBE,
};

use crate::error::IdeError;
use crate::lsp::ProgressiveLspCap;
use crate::ports::ControlTransport;

/// Unary RPC names from the user API table. Case-sensitive. Never `$/`.
pub const CONTROL_UNARY_METHODS: &[&str] = &[
    METHOD_GET_CONFIG,
    METHOD_SET_CONFIG,
    METHOD_RELOAD_CONFIG,
    METHOD_INSTALL_PACKS,
    METHOD_WATCH_SUBSCRIBE,
    METHOD_FILES_SINCE,
    METHOD_INDEX_STATUS,
    METHOD_TIER_STATUS,
    METHOD_RELOAD_SCRIPTS,
];

/// Typed server push. `request_id` is always 0.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlPush {
    WatchBatch(WatchBatch),
    TierReady(TierReady),
}

impl ControlPush {
    pub fn from_envelope(env: &Envelope) -> Result<Self, IdeError> {
        if env.request_id != 0 {
            return Err(IdeError::control("push must have request_id 0"));
        }
        match env.method.as_str() {
            METHOD_WATCH_BATCH => env
                .decode_body::<WatchBatch>()
                .map(Self::WatchBatch)
                .map_err(|e| IdeError::control(e.to_string())),
            METHOD_TIER_READY => env
                .decode_body::<TierReady>()
                .map(Self::TierReady)
                .map_err(|e| IdeError::control(e.to_string())),
            other => Err(IdeError::control(format!("unknown push {other}"))),
        }
    }

    pub fn method(&self) -> &'static str {
        match self {
            Self::WatchBatch(_) => METHOD_WATCH_BATCH,
            Self::TierReady(_) => METHOD_TIER_READY,
        }
    }

    pub fn request_id(&self) -> u64 {
        0
    }

    pub fn is_watch_batch(&self) -> bool {
        matches!(self, Self::WatchBatch(_))
    }

    pub fn is_tier_ready(&self) -> bool {
        matches!(self, Self::TierReady(_))
    }
}

/// Unix socket + `encode_frame` / `decode_frame`. Payload > 16 MiB fails.
pub struct UnixControl {
    stream: UnixStream,
    buf: Vec<u8>,
    pushes: Vec<Envelope>,
    path: Option<PathBuf>,
}

impl UnixControl {
    pub fn connect(path: impl AsRef<Path>) -> Result<Self, IdeError> {
        let path = path.as_ref();
        let stream = UnixStream::connect(path)
            .map_err(|e| IdeError::control(format!("control socket {}: {e}", path.display())))?;
        Ok(Self {
            stream,
            buf: Vec::new(),
            pushes: Vec::new(),
            path: Some(path.to_path_buf()),
        })
    }

    pub fn from_stream(stream: UnixStream) -> Self {
        Self {
            stream,
            buf: Vec::new(),
            pushes: Vec::new(),
            path: None,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn reject_oversize(request: &Envelope) -> Result<(), IdeError> {
        if request.body.len() > MAX_PAYLOAD_BYTES as usize {
            return Err(IdeError::control(format!(
                "payload exceeds {MAX_PAYLOAD_BYTES} bytes ({})",
                request.body.len()
            )));
        }
        Ok(())
    }

    fn write_envelope(&mut self, env: &Envelope) -> Result<(), IdeError> {
        let frame = encode_frame(&env.to_bytes()).map_err(|e| IdeError::control(e.to_string()))?;
        self.stream
            .write_all(&frame)
            .map_err(|e| IdeError::control(e.to_string()))?;
        self.stream
            .flush()
            .map_err(|e| IdeError::control(e.to_string()))
    }

    fn read_blocking(&mut self) -> Result<(), IdeError> {
        let mut tmp = [0u8; 4096];
        let n = self
            .stream
            .read(&mut tmp)
            .map_err(|e| IdeError::control(e.to_string()))?;
        if n == 0 {
            return Err(IdeError::control("eof"));
        }
        self.buf.extend_from_slice(&tmp[..n]);
        Ok(())
    }

    fn drain_complete(&mut self, want_reply: bool) -> Result<Option<Envelope>, IdeError> {
        loop {
            match decode_frame(&self.buf) {
                Ok(DecodeOutcome::Complete { payload, consumed }) => {
                    self.buf.drain(..consumed);
                    let env = Envelope::from_bytes(&payload)
                        .map_err(|e| IdeError::control(e.to_string()))?;
                    if env.request_id == 0 {
                        self.pushes.push(env);
                        continue;
                    }
                    if want_reply {
                        return Ok(Some(env));
                    }
                }
                Ok(DecodeOutcome::Incomplete { .. }) => return Ok(None),
                Err(e) => return Err(IdeError::control(e.to_string())),
            }
        }
    }
}

impl std::fmt::Debug for UnixControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnixControl")
            .field("path", &self.path)
            .field("buf_len", &self.buf.len())
            .field("pushes", &self.pushes.len())
            .finish()
    }
}

impl ControlTransport for UnixControl {
    fn send(&mut self, request: Envelope) -> Result<Envelope, IdeError> {
        Self::reject_oversize(&request)?;
        let want = request.request_id;
        self.write_envelope(&request)?;
        loop {
            if let Some(env) = self.drain_complete(true)? {
                if env.request_id == want {
                    return Ok(env);
                }
                continue;
            }
            self.read_blocking()?;
        }
    }

    fn take_pushes(&mut self) -> Vec<Envelope> {
        std::mem::take(&mut self.pushes)
    }

    fn poll(&mut self) -> Result<(), IdeError> {
        self.stream
            .set_nonblocking(true)
            .map_err(|e| IdeError::control(e.to_string()))?;
        let mut tmp = [0u8; 4096];
        let mut eof = false;
        let read_result = loop {
            match self.stream.read(&mut tmp) {
                Ok(0) => {
                    eof = true;
                    break Ok(());
                }
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::Interrupted =>
                {
                    break Ok(());
                }
                Err(e) => break Err(IdeError::control(e.to_string())),
            }
        };
        let _ = self.stream.set_nonblocking(false);
        read_result?;
        let _ = self.drain_complete(false)?;
        if eof && !self.buf.is_empty() {
            return Err(IdeError::control("eof"));
        }
        Ok(())
    }
}

/// Advertised socket after `initialize`. `--mux` stays `pending_mux`.
pub fn advertised_control_socket(cap: &ProgressiveLspCap) -> Result<&str, IdeError> {
    if cap.mux() {
        return Err(IdeError::pending_mux());
    }
    cap.socket().ok_or_else(IdeError::control_socket_missing)
}

fn reject_lsp_files_since(method: &str) -> Result<(), IdeError> {
    if method == METHOD_FILES_SINCE {
        return Ok(());
    }
    if method.contains("filesSince") || method.starts_with("$/") {
        return Err(IdeError::control(format!(
            "FilesSince is Envelope-only; refused {method}"
        )));
    }
    Ok(())
}

/// Unary RPCs + push dispatch. Never `$/` FilesSince.
pub struct ControlClient<T: ControlTransport> {
    transport: T,
    next_id: u64,
    pushes: Vec<ControlPush>,
}

impl<T: ControlTransport> ControlClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            next_id: 1,
            pushes: Vec::new(),
        }
    }

    pub fn next_request_id(&self) -> u64 {
        self.next_id
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn into_inner(self) -> T {
        self.transport
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        if id == 0 {
            self.next_id = 2;
            1
        } else {
            id
        }
    }

    fn ingest_pushes(&mut self) {
        for env in self.transport.take_pushes() {
            if let Ok(push) = ControlPush::from_envelope(&env) {
                self.pushes.push(push);
            }
        }
    }

    fn call<Req, Resp>(&mut self, method: &str, req: Req) -> Result<Resp, IdeError>
    where
        Req: Message,
        Resp: Message + Default,
    {
        reject_lsp_files_since(method)?;
        let id = self.take_id();
        let env = Envelope::request(method, id, req);
        let reply = self.transport.send(env)?;
        self.ingest_pushes();
        if reply.request_id != id {
            return Err(IdeError::control("request_id mismatch"));
        }
        reply
            .decode_body()
            .map_err(|e| IdeError::control(e.to_string()))
    }

    pub fn invoke(&mut self, method: &str, body: Vec<u8>) -> Result<Envelope, IdeError> {
        reject_lsp_files_since(method)?;
        let id = self.take_id();
        let env = Envelope {
            method: method.to_string(),
            request_id: id,
            body,
        };
        let reply = self.transport.send(env)?;
        self.ingest_pushes();
        Ok(reply)
    }

    pub fn get_config(&mut self) -> Result<GetConfigResponse, IdeError> {
        self.call(METHOD_GET_CONFIG, GetConfigRequest {})
    }

    pub fn set_config(
        &mut self,
        patch_toml: impl Into<String>,
    ) -> Result<SetConfigResponse, IdeError> {
        self.call(
            METHOD_SET_CONFIG,
            SetConfigRequest {
                patch_toml: patch_toml.into(),
            },
        )
    }

    pub fn reload_config(&mut self) -> Result<ReloadConfigResponse, IdeError> {
        self.call(METHOD_RELOAD_CONFIG, ReloadConfigRequest {})
    }

    pub fn install_packs(
        &mut self,
        packs: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<InstallPacksResponse, IdeError> {
        self.call(
            METHOD_INSTALL_PACKS,
            InstallPacksRequest {
                packs: packs.into_iter().map(Into::into).collect(),
            },
        )
    }

    pub fn watch_subscribe(&mut self) -> Result<WatchSubscribeResponse, IdeError> {
        self.call(METHOD_WATCH_SUBSCRIBE, WatchSubscribeRequest {})
    }

    pub fn files_since(
        &mut self,
        since: files_since_request::Since,
    ) -> Result<FilesSinceResponse, IdeError> {
        self.call(METHOD_FILES_SINCE, FilesSinceRequest { since: Some(since) })
    }

    pub fn files_since_generation(
        &mut self,
        generation: u64,
    ) -> Result<FilesSinceResponse, IdeError> {
        self.files_since(files_since_request::Since::SinceGeneration(generation))
    }

    pub fn files_since_unix_ms(&mut self, unix_ms: u64) -> Result<FilesSinceResponse, IdeError> {
        self.files_since(files_since_request::Since::SinceUnixMs(unix_ms))
    }

    pub fn index_status(&mut self) -> Result<IndexStatusResponse, IdeError> {
        self.call(METHOD_INDEX_STATUS, IndexStatusRequest {})
    }

    pub fn tier_status(&mut self) -> Result<TierStatusResponse, IdeError> {
        self.call(METHOD_TIER_STATUS, TierStatusRequest {})
    }

    pub fn reload_scripts(&mut self) -> Result<ReloadScriptsResponse, IdeError> {
        self.call(METHOD_RELOAD_SCRIPTS, ReloadScriptsRequest {})
    }

    pub fn poll_pushes(&mut self) -> Result<Vec<ControlPush>, IdeError> {
        self.transport.poll()?;
        self.ingest_pushes();
        Ok(std::mem::take(&mut self.pushes))
    }

    pub fn take_pushes(&mut self) -> Vec<ControlPush> {
        self.ingest_pushes();
        std::mem::take(&mut self.pushes)
    }
}

impl ControlClient<UnixControl> {
    pub fn connect(path: impl AsRef<Path>) -> Result<Self, IdeError> {
        Ok(Self::new(UnixControl::connect(path)?))
    }

    pub fn from_cap(cap: &ProgressiveLspCap) -> Result<Self, IdeError> {
        let path = advertised_control_socket(cap)?;
        Self::connect(path)
    }

    pub fn connect_matching(cap: &ProgressiveLspCap, expected: &Path) -> Result<Self, IdeError> {
        let path = advertised_control_socket(cap)?;
        if Path::new(path) != expected {
            return Err(IdeError::control(format!(
                "advertised socket {path} does not match {}",
                expected.display()
            )));
        }
        Self::connect(path)
    }
}

impl<T: ControlTransport> std::fmt::Debug for ControlClient<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlClient")
            .field("next_id", &self.next_id)
            .field("pushes", &self.pushes.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::FakeControl;
    use progressive_lsp_control::{Status, WatchEvent};
    use serde_json::json;

    fn cap(socket: Option<&str>, mux: bool) -> ProgressiveLspCap {
        ProgressiveLspCap::from_initialize_result(&json!({
            "capabilities": {
                "experimental": {
                    "progressiveLsp": {
                        "version": "v1",
                        "socket": socket,
                        "mux": mux
                    }
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn advertised_control_socket_mux_is_pending_mux() {
        assert!(advertised_control_socket(&cap(None, false))
            .unwrap_err()
            .is_control_socket_missing());
        assert!(advertised_control_socket(&cap(Some("/tmp/ok.sock"), true))
            .unwrap_err()
            .is_pending_mux());
        assert_eq!(
            advertised_control_socket(&cap(Some("/tmp/ok.sock"), false)).unwrap(),
            "/tmp/ok.sock"
        );
        assert!(ControlClient::<UnixControl>::from_cap(&cap(None, false))
            .unwrap_err()
            .is_control_socket_missing());
        assert!(
            ControlClient::<UnixControl>::from_cap(&cap(Some("/tmp/x.sock"), true))
                .unwrap_err()
                .is_pending_mux()
        );
        assert!(ControlClient::<UnixControl>::connect_matching(
            &cap(Some("/tmp/a.sock"), false),
            Path::new("/tmp/b.sock"),
        )
        .unwrap_err()
        .is_control());
        assert!(ControlClient::<UnixControl>::connect_matching(
            &cap(Some("/tmp/a.sock"), true),
            Path::new("/tmp/a.sock"),
        )
        .unwrap_err()
        .is_pending_mux());
        assert!(CONTROL_UNARY_METHODS.contains(&METHOD_FILES_SINCE));
        assert!(!CONTROL_UNARY_METHODS
            .iter()
            .any(|m| m.contains("$/") || m.contains("filesSince") && *m != METHOD_FILES_SINCE));
        assert_eq!(CONTROL_UNARY_METHODS.len(), 9);
    }

    #[test]
    fn control_client_adapter_unary_rpcs_and_push_dispatch() {
        let mut fake = FakeControl::new();
        fake.queue_push(Envelope::push(
            METHOD_WATCH_BATCH,
            WatchBatch {
                events: vec![WatchEvent {
                    path: "/ws/a.rs".into(),
                    kind: "modify".into(),
                }],
                overflow: false,
                need_rescan: false,
                generation: 4,
            },
        ));
        fake.queue_push(Envelope::push(
            METHOD_TIER_READY,
            TierReady {
                package_id: "pkg".into(),
                tier: "types".into(),
            },
        ));
        let mut client = ControlClient::new(fake);
        assert_eq!(client.next_request_id(), 1);
        let debug = format!("{client:?}");
        assert!(debug.contains("ControlClient"));
        assert!(debug.contains("next_id: 1"));

        let cfg = client.get_config().unwrap();
        assert!(cfg.status.unwrap().is_ok());
        assert_eq!(cfg.toml, "packs = []\n");
        assert!(client
            .set_config("packs = [\"rust\"]")
            .unwrap()
            .status
            .unwrap()
            .is_ok());
        assert!(client.reload_config().unwrap().status.unwrap().is_ok());
        assert!(client
            .install_packs(["python", "rust"])
            .unwrap()
            .status
            .unwrap()
            .is_ok());
        assert!(client.watch_subscribe().unwrap().status.unwrap().is_ok());
        let since = client.files_since_generation(3).unwrap();
        assert!(since.status.unwrap().is_ok());
        assert_eq!(since.paths, vec!["/ws/a.rs"]);
        assert!(!since.truncated);
        assert_eq!(since.generation, 1);
        let since_ms = client.files_since_unix_ms(99).unwrap();
        assert!(since_ms.status.unwrap().is_ok());
        let index = client.index_status().unwrap();
        assert_eq!(index.packages[0].package_id, "pkg");
        let tiers = client.tier_status().unwrap();
        assert_eq!(tiers.rows[0].tier, "syntax");
        assert!(client.reload_scripts().unwrap().status.unwrap().is_ok());

        let pushes = client.poll_pushes().unwrap();
        assert_eq!(pushes.len(), 2);
        assert!(pushes.iter().all(|p| p.request_id() == 0));
        assert!(pushes[0].is_watch_batch());
        assert_eq!(pushes[0].method(), METHOD_WATCH_BATCH);
        assert!(pushes[1].is_tier_ready());
        assert_eq!(pushes[1].method(), METHOD_TIER_READY);
        match &pushes[0] {
            ControlPush::WatchBatch(b) => assert_eq!(b.generation, 4),
            ControlPush::TierReady(_) => panic!("expected WatchBatch"),
        }
        match &pushes[1] {
            ControlPush::TierReady(t) => {
                assert_eq!(t.package_id, "pkg");
                assert_eq!(t.tier, "types");
            }
            ControlPush::WatchBatch(_) => panic!("expected TierReady"),
        }
        assert!(client.take_pushes().is_empty());

        let inner = client.into_inner();
        assert!(inner
            .sent()
            .iter()
            .all(|e| e.method != "$/progressive/filesSince"
                && e.method != "workspace/filesSince"
                && !e.method.starts_with("$/")));
        assert!(inner.sent().iter().any(|e| e.method == METHOD_FILES_SINCE));
        assert_eq!(inner.sent().len(), 10);
    }

    #[test]
    fn control_client_adapter_never_dollar_files_since() {
        let mut client = ControlClient::new(FakeControl::new());
        assert!(client
            .invoke("$/progressive/filesSince", vec![])
            .unwrap_err()
            .is_control());
        assert!(client
            .invoke("workspace/filesSince", vec![])
            .unwrap_err()
            .is_control());
        assert!(client
            .invoke("$/progress", vec![])
            .unwrap_err()
            .is_control());
        let ok = client.invoke(METHOD_FILES_SINCE, vec![]).unwrap();
        assert_eq!(ok.method, METHOD_FILES_SINCE);
        assert_eq!(ok.request_id, 1);
        assert_eq!(client.next_request_id(), 2);
        assert!(client
            .transport()
            .sent()
            .iter()
            .all(|e| e.method != "$/progressive/filesSince"));
        assert_eq!(client.transport().sent_methods(), vec![METHOD_FILES_SINCE]);
    }

    #[test]
    fn control_client_adapter_missing_socket_and_status_error() {
        let mut missing = ControlClient::new(FakeControl::missing_socket());
        assert!(missing
            .get_config()
            .unwrap_err()
            .is_control_socket_missing());
        assert!(missing
            .poll_pushes()
            .unwrap_err()
            .is_control_socket_missing());

        let mut fake = FakeControl::new();
        fake.script(Envelope::reply(
            METHOD_INSTALL_PACKS,
            0,
            InstallPacksResponse {
                status: Some(Status::error(2, "hash")),
            },
        ));
        let mut client = ControlClient::new(fake);
        let resp = client.install_packs(["python"]).unwrap();
        assert!(!resp.status.as_ref().unwrap().is_ok());
        assert_eq!(resp.status.unwrap().message, "hash");
        let over = client
            .invoke(
                METHOD_GET_CONFIG,
                vec![0u8; (MAX_PAYLOAD_BYTES as usize) + 1],
            )
            .unwrap_err();
        assert!(over.is_control());
        assert!(over.to_string().contains("payload exceeds"));
    }

    #[test]
    fn control_push_value_object_request_id_zero() {
        let batch = Envelope::push(METHOD_WATCH_BATCH, WatchBatch::default());
        let ready = Envelope::push(METHOD_TIER_READY, TierReady::default());
        let batch_push = ControlPush::from_envelope(&batch).unwrap();
        let ready_push = ControlPush::from_envelope(&ready).unwrap();
        assert_eq!(batch_push.request_id(), 0);
        assert_eq!(ready_push.request_id(), 0);
        assert!(batch_push.is_watch_batch());
        assert!(!batch_push.is_tier_ready());
        assert!(ready_push.is_tier_ready());
        assert!(!ready_push.is_watch_batch());
        let mut nonzero = batch.clone();
        nonzero.request_id = 1;
        assert!(ControlPush::from_envelope(&nonzero)
            .unwrap_err()
            .is_control());
        let unknown = Envelope::push("Nope", GetConfigRequest {});
        assert!(ControlPush::from_envelope(&unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown push"));
    }

    #[test]
    fn unix_control_adapter_connect_missing_is_domain_error() {
        let err = UnixControl::connect("/no/such/poc-ide5-control.sock").unwrap_err();
        assert!(err.is_control());
        assert!(err.to_string().contains("control socket"));
        assert!(
            ControlClient::<UnixControl>::connect("/no/such/poc-ide5-control.sock")
                .unwrap_err()
                .is_control()
        );
        assert!(ControlClient::<UnixControl>::from_cap(&cap(
            Some("/no/such/poc-ide5-control.sock"),
            false
        ))
        .unwrap_err()
        .is_control());
    }

    #[cfg(unix)]
    #[test]
    fn unix_control_adapter_envelope_round_trip() {
        let (client_s, mut server_s) = UnixStream::pair().unwrap();
        let mut client = UnixControl::from_stream(client_s);
        let debug = format!("{client:?}");
        assert!(debug.contains("UnixControl"));
        assert!(client.path().is_none());

        let handle = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let env = loop {
                let n = server_s.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..n]);
                match decode_frame(&buf).unwrap() {
                    DecodeOutcome::Complete { payload, consumed } => {
                        buf.drain(..consumed);
                        break Envelope::from_bytes(&payload).unwrap();
                    }
                    DecodeOutcome::Incomplete { .. } => continue,
                }
            };
            assert_eq!(env.method, METHOD_GET_CONFIG);
            assert_eq!(env.request_id, 3);
            let reply = Envelope::reply(
                METHOD_GET_CONFIG,
                3,
                GetConfigResponse {
                    status: Some(Status::ok()),
                    toml: "x = 1\n".into(),
                },
            );
            server_s
                .write_all(&encode_frame(&reply.to_bytes()).unwrap())
                .unwrap();
            let push = Envelope::push(
                METHOD_TIER_READY,
                TierReady {
                    package_id: "p".into(),
                    tier: "graph".into(),
                },
            );
            server_s
                .write_all(&encode_frame(&push.to_bytes()).unwrap())
                .unwrap();
            server_s.flush().unwrap();
        });

        let reply = client
            .send(Envelope::request(METHOD_GET_CONFIG, 3, GetConfigRequest {}))
            .unwrap();
        assert_eq!(reply.request_id, 3);
        assert_eq!(
            reply.decode_body::<GetConfigResponse>().unwrap().toml,
            "x = 1\n"
        );
        handle.join().unwrap();
        client.poll().unwrap();
        let pushes = client.take_pushes();
        assert_eq!(pushes.len(), 1);
        assert_eq!(pushes[0].request_id, 0);
        assert_eq!(pushes[0].method, METHOD_TIER_READY);
    }

    struct MismatchControl;

    impl ControlTransport for MismatchControl {
        fn send(&mut self, request: Envelope) -> Result<Envelope, IdeError> {
            Ok(Envelope {
                method: request.method,
                request_id: request.request_id.saturating_add(1),
                body: Vec::new(),
            })
        }

        fn take_pushes(&mut self) -> Vec<Envelope> {
            Vec::new()
        }

        fn poll(&mut self) -> Result<(), IdeError> {
            Ok(())
        }
    }

    #[test]
    fn control_client_adapter_request_id_mismatch_and_unknown_push() {
        let mut client = ControlClient::new(MismatchControl);
        assert!(client
            .get_config()
            .unwrap_err()
            .to_string()
            .contains("request_id mismatch"));
        assert_eq!(
            client
                .transport_mut()
                .send(Envelope::request(METHOD_GET_CONFIG, 1, GetConfigRequest {}))
                .unwrap()
                .request_id,
            2
        );

        let mut fake = FakeControl::new();
        fake.queue_push(Envelope::push("Nope", GetConfigRequest {}));
        let mut client = ControlClient::new(fake);
        assert!(client.poll_pushes().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn unix_control_adapter_connect_success_poll_and_mismatch_id() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("poc-ide5.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = s.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if matches!(decode_frame(&buf), Ok(DecodeOutcome::Complete { .. })) {
                    break;
                }
            }
            let wrong = Envelope::reply(METHOD_GET_CONFIG, 99, GetConfigResponse::default());
            let right = Envelope::reply(
                METHOD_GET_CONFIG,
                1,
                GetConfigResponse {
                    status: Some(Status::ok()),
                    toml: "ok".into(),
                },
            );
            s.write_all(&encode_frame(&wrong.to_bytes()).unwrap())
                .unwrap();
            s.write_all(&encode_frame(&right.to_bytes()).unwrap())
                .unwrap();
            s.flush().unwrap();
        });
        let mut unix = UnixControl::connect(&sock).unwrap();
        assert_eq!(unix.path(), Some(sock.as_path()));
        let debug = format!("{unix:?}");
        assert!(debug.contains("poc-ide5.sock"));
        unix.poll().unwrap();
        assert!(unix.take_pushes().is_empty());
        let reply = unix
            .send(Envelope::request(METHOD_GET_CONFIG, 1, GetConfigRequest {}))
            .unwrap();
        assert_eq!(reply.request_id, 1);
        handle.join().unwrap();

        let cap = cap(Some(sock.to_str().unwrap()), false);
        assert!(ControlClient::<UnixControl>::connect_matching(&cap, &sock)
            .unwrap_err()
            .is_control());
    }

    #[cfg(unix)]
    #[test]
    fn unix_control_adapter_poll_eof_leftover_and_wouldblock() {
        let (a, _b) = UnixStream::pair().unwrap();
        let mut idle = UnixControl::from_stream(a);
        idle.poll().unwrap();
        assert!(idle.take_pushes().is_empty());

        let (c, mut d) = UnixStream::pair().unwrap();
        let mut reader = UnixControl::from_stream(c);
        d.write_all(&[0, 0]).unwrap();
        d.flush().unwrap();
        drop(d);
        let err = reader.poll().unwrap_err();
        assert!(err.is_control());
        assert!(err.to_string().contains("eof"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_control_adapter_send_eof_is_domain_error() {
        let (a, b) = UnixStream::pair().unwrap();
        drop(b);
        let mut client = UnixControl::from_stream(a);
        let err = client
            .send(Envelope::request(METHOD_GET_CONFIG, 1, GetConfigRequest {}))
            .unwrap_err();
        assert!(err.is_control());
    }

    #[cfg(unix)]
    #[test]
    fn unix_control_adapter_payload_over_16mib_fails() {
        let (a, _b) = UnixStream::pair().unwrap();
        let mut client = UnixControl::from_stream(a);
        let over = Envelope {
            method: METHOD_GET_CONFIG.into(),
            request_id: 1,
            body: vec![0u8; (MAX_PAYLOAD_BYTES as usize) + 1],
        };
        let err = client.send(over).unwrap_err();
        assert!(err.is_control());
        assert!(err.to_string().contains("payload exceeds"));

        let (c, mut d) = UnixStream::pair().unwrap();
        let mut reader = UnixControl::from_stream(c);
        let too_big = (MAX_PAYLOAD_BYTES + 1).to_be_bytes();
        d.write_all(&too_big).unwrap();
        d.flush().unwrap();
        let err = reader.poll().unwrap_err();
        assert!(err.is_control());
        assert!(err.to_string().contains("payload exceeds"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_control_adapter_push_before_reply_is_queued() {
        let (client_s, mut server_s) = UnixStream::pair().unwrap();
        let mut client = UnixControl::from_stream(client_s);
        let handle = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = server_s.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if matches!(decode_frame(&buf), Ok(DecodeOutcome::Complete { .. })) {
                    break;
                }
            }
            let push = Envelope::push(METHOD_WATCH_BATCH, WatchBatch::default());
            let reply = Envelope::reply(METHOD_GET_CONFIG, 1, GetConfigResponse::default());
            server_s
                .write_all(&encode_frame(&push.to_bytes()).unwrap())
                .unwrap();
            server_s
                .write_all(&encode_frame(&reply.to_bytes()).unwrap())
                .unwrap();
            server_s.flush().unwrap();
        });
        let reply = client
            .send(Envelope::request(METHOD_GET_CONFIG, 1, GetConfigRequest {}))
            .unwrap();
        assert_eq!(reply.request_id, 1);
        handle.join().unwrap();
        let pushes = client.take_pushes();
        assert_eq!(pushes.len(), 1);
        assert_eq!(pushes[0].request_id, 0);
        assert_eq!(pushes[0].method, METHOD_WATCH_BATCH);
    }
}
