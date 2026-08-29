//! Control facade. FilesSince is protobuf only — never `$/` JSON-RPC.
//! Public dispatch is [`Envelope`]: method + request_id + body.

use std::sync::Arc;

use prost::Message;

use crate::codec::{decode_exact, encode_frame, CodecError};
use crate::messages::*;

/// Port so the control crate does not own watch internals.
pub trait FilesSincePort: Send + Sync {
    fn files_since(&self, req: &FilesSinceRequest) -> FilesSinceResponse;
    fn last_watch_batch(&self) -> WatchBatch;
}

/// Port for the public Envelope RPCs. Composition root implements this.
pub trait ControlPlane: Send + Sync {
    fn get_config(&self, req: &GetConfigRequest) -> GetConfigResponse;
    fn set_config(&self, req: &SetConfigRequest) -> SetConfigResponse;
    fn reload_config(&self, req: &ReloadConfigRequest) -> ReloadConfigResponse;
    fn install_packs(&self, req: &InstallPacksRequest) -> InstallPacksResponse;
    fn watch_subscribe(&self, req: &WatchSubscribeRequest) -> WatchSubscribeResponse;
    fn files_since(&self, req: &FilesSinceRequest) -> FilesSinceResponse;
    fn last_watch_batch(&self) -> WatchBatch;
    fn take_watch_batches(&self) -> Vec<WatchBatch> {
        Vec::new()
    }
    fn index_status(&self, req: &IndexStatusRequest) -> IndexStatusResponse;
    fn tier_status(&self, req: &TierStatusRequest) -> TierStatusResponse;
    fn take_tier_ready(&self) -> Vec<TierReady>;
    fn reload_scripts(&self, req: &ReloadScriptsRequest) -> ReloadScriptsResponse;
}

/// Same domain services as LSP, different encoding.
#[derive(Clone, Default)]
pub struct ControlServer {
    pub config_toml: String,
    files_since: Option<Arc<dyn FilesSincePort>>,
    plane: Option<Arc<dyn ControlPlane>>,
    pending_tier_ready: Arc<std::sync::Mutex<Vec<TierReady>>>,
    progressive_connected: bool,
}

impl std::fmt::Debug for ControlServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlServer")
            .field("config_toml", &self.config_toml)
            .field("has_files_since", &self.files_since.is_some())
            .field("has_plane", &self.plane.is_some())
            .field("progressive_connected", &self.progressive_connected)
            .finish()
    }
}

impl PartialEq for ControlServer {
    fn eq(&self, other: &Self) -> bool {
        self.config_toml == other.config_toml
            && self.files_since.is_some() == other.files_since.is_some()
            && self.plane.is_some() == other.plane.is_some()
    }
}

impl Eq for ControlServer {}

impl ControlServer {
    pub fn new(config_toml: impl Into<String>) -> Self {
        Self {
            config_toml: config_toml.into(),
            files_since: None,
            plane: None,
            pending_tier_ready: Arc::new(std::sync::Mutex::new(Vec::new())),
            progressive_connected: false,
        }
    }

    pub fn with_files_since(mut self, port: Arc<dyn FilesSincePort>) -> Self {
        self.files_since = Some(port);
        self
    }

    pub fn with_plane(mut self, plane: Arc<dyn ControlPlane>) -> Self {
        self.plane = Some(plane);
        self
    }

    pub fn with_progressive(mut self, connected: bool) -> Self {
        self.progressive_connected = connected;
        self
    }

    pub fn is_progressive_connected(&self) -> bool {
        self.progressive_connected
    }

    /// Push only when a progressive client is connected. Proto only — not `$/`.
    pub fn push_tier_ready(&self, package_id: impl Into<String>, tier: impl Into<String>) -> bool {
        if !self.progressive_connected {
            return false;
        }
        self.pending_tier_ready
            .lock()
            .expect("tier ready lock")
            .push(TierReady {
                package_id: package_id.into(),
                tier: tier.into(),
            });
        true
    }

    pub fn take_tier_ready(&self) -> Vec<TierReady> {
        let mut local = std::mem::take(&mut *self.pending_tier_ready.lock().expect("tier ready lock"));
        if let Some(plane) = &self.plane {
            local.extend(plane.take_tier_ready());
        }
        local
    }

    pub fn get_config(&self, req: &GetConfigRequest) -> GetConfigResponse {
        if let Some(plane) = &self.plane {
            return plane.get_config(req);
        }
        GetConfigResponse {
            status: Some(Status::ok()),
            toml: self.config_toml.clone(),
        }
    }

    pub fn set_config(&self, req: &SetConfigRequest) -> SetConfigResponse {
        if let Some(plane) = &self.plane {
            return plane.set_config(req);
        }
        SetConfigResponse {
            status: Some(Status::ok()),
        }
    }

    pub fn reload_config(&self, req: &ReloadConfigRequest) -> ReloadConfigResponse {
        if let Some(plane) = &self.plane {
            return plane.reload_config(req);
        }
        ReloadConfigResponse {
            status: Some(Status::ok()),
        }
    }

    pub fn install_packs(&self, req: &InstallPacksRequest) -> InstallPacksResponse {
        if let Some(plane) = &self.plane {
            return plane.install_packs(req);
        }
        InstallPacksResponse {
            status: Some(Status::ok()),
        }
    }

    pub fn watch_subscribe(&self, req: &WatchSubscribeRequest) -> WatchSubscribeResponse {
        if let Some(plane) = &self.plane {
            return plane.watch_subscribe(req);
        }
        WatchSubscribeResponse {
            status: Some(Status::ok()),
        }
    }

    pub fn empty_watch_batch(&self) -> WatchBatch {
        WatchBatch {
            events: Vec::new(),
            overflow: false,
            need_rescan: false,
            generation: 0,
        }
    }

    pub fn last_watch_batch(&self) -> WatchBatch {
        if let Some(plane) = &self.plane {
            return plane.last_watch_batch();
        }
        match &self.files_since {
            Some(port) => port.last_watch_batch(),
            None => self.empty_watch_batch(),
        }
    }

    pub fn files_since(&self, req: &FilesSinceRequest) -> FilesSinceResponse {
        if let Some(plane) = &self.plane {
            return plane.files_since(req);
        }
        if let Some(port) = &self.files_since {
            return port.files_since(req);
        }
        FilesSinceResponse {
            status: Some(Status::ok()),
            paths: Vec::new(),
            truncated: false,
            generation: 0,
        }
    }

    pub fn index_status(&self, req: &IndexStatusRequest) -> IndexStatusResponse {
        if let Some(plane) = &self.plane {
            return plane.index_status(req);
        }
        IndexStatusResponse {
            status: Some(Status::ok()),
            packages: Vec::new(),
            cache_entries: 0,
        }
    }

    pub fn tier_status(&self, req: &TierStatusRequest) -> TierStatusResponse {
        if let Some(plane) = &self.plane {
            return plane.tier_status(req);
        }
        TierStatusResponse {
            status: Some(Status::ok()),
            rows: Vec::new(),
        }
    }

    pub fn reload_scripts(&self, req: &ReloadScriptsRequest) -> ReloadScriptsResponse {
        if let Some(plane) = &self.plane {
            return plane.reload_scripts(req);
        }
        ReloadScriptsResponse {
            status: Some(Status::ok()),
        }
    }

    /// Public dispatch: decode Envelope, route by method, echo request_id.
    pub fn dispatch_envelope(&self, env: &Envelope) -> Envelope {
        match env.method.as_str() {
            METHOD_GET_CONFIG => {
                let req = env.decode_body::<GetConfigRequest>().unwrap_or_default();
                Envelope::reply(METHOD_GET_CONFIG, env.request_id, self.get_config(&req))
            }
            METHOD_SET_CONFIG => {
                let req = env.decode_body::<SetConfigRequest>().unwrap_or_default();
                Envelope::reply(METHOD_SET_CONFIG, env.request_id, self.set_config(&req))
            }
            METHOD_RELOAD_CONFIG => {
                let req = env.decode_body::<ReloadConfigRequest>().unwrap_or_default();
                Envelope::reply(METHOD_RELOAD_CONFIG, env.request_id, self.reload_config(&req))
            }
            METHOD_INSTALL_PACKS => {
                let req = env.decode_body::<InstallPacksRequest>().unwrap_or_default();
                Envelope::reply(METHOD_INSTALL_PACKS, env.request_id, self.install_packs(&req))
            }
            METHOD_WATCH_SUBSCRIBE => {
                let req = env.decode_body::<WatchSubscribeRequest>().unwrap_or_default();
                Envelope::reply(METHOD_WATCH_SUBSCRIBE, env.request_id, self.watch_subscribe(&req))
            }
            METHOD_FILES_SINCE => {
                let req = env.decode_body::<FilesSinceRequest>().unwrap_or_default();
                Envelope::reply(METHOD_FILES_SINCE, env.request_id, self.files_since(&req))
            }
            METHOD_INDEX_STATUS => {
                let req = env.decode_body::<IndexStatusRequest>().unwrap_or_default();
                Envelope::reply(METHOD_INDEX_STATUS, env.request_id, self.index_status(&req))
            }
            METHOD_TIER_STATUS => {
                let req = env.decode_body::<TierStatusRequest>().unwrap_or_default();
                Envelope::reply(METHOD_TIER_STATUS, env.request_id, self.tier_status(&req))
            }
            METHOD_RELOAD_SCRIPTS => {
                let req = env.decode_body::<ReloadScriptsRequest>().unwrap_or_default();
                Envelope::reply(METHOD_RELOAD_SCRIPTS, env.request_id, self.reload_scripts(&req))
            }
            other => Envelope::reply(
                other,
                env.request_id,
                Status::error(2, format!("unknown method: {other}")),
            ),
        }
    }

    pub fn dispatch_payload(&self, payload: &[u8]) -> Result<Vec<u8>, CodecError> {
        let env = Envelope::decode(payload).unwrap_or_default();
        if env.method.is_empty() {
            return Err(CodecError::Incomplete);
        }
        Ok(self.dispatch_envelope(&env).encode_to_vec())
    }

    /// Drain WatchBatch / TierReady as Envelope pushes (`request_id == 0`).
    pub fn drain_pushes(&self) -> Vec<Envelope> {
        let mut out = Vec::new();
        if let Some(plane) = &self.plane {
            for batch in plane.take_watch_batches() {
                out.push(Envelope::push(METHOD_WATCH_BATCH, batch));
            }
        }
        for ready in self.take_tier_ready() {
            out.push(Envelope::push(METHOD_TIER_READY, ready));
        }
        out
    }

    /// Mux control channel: inner payload is length-prefixed Envelope, never `$/` JSON-RPC.
    pub fn handle_mux_payload(&self, length_prefixed: &[u8]) -> Result<Vec<u8>, CodecError> {
        let inner = decode_exact(length_prefixed)?;
        let reply = self.dispatch_payload(&inner)?;
        encode_frame(&reply)
    }

    pub fn encode_watch_batch(&self) -> Result<Vec<u8>, CodecError> {
        encode_frame(
            &Envelope::push(METHOD_WATCH_BATCH, self.last_watch_batch()).encode_to_vec(),
        )
    }

    pub fn encode_files_since(&self, req: &FilesSinceRequest) -> Result<Vec<u8>, CodecError> {
        encode_frame(
            &Envelope::reply(METHOD_FILES_SINCE, 1, self.files_since(req)).encode_to_vec(),
        )
    }

    pub fn encode_envelope_frame(&self, env: &Envelope) -> Result<Vec<u8>, CodecError> {
        encode_frame(&self.dispatch_envelope(env).encode_to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubPort {
        paths: Vec<String>,
        truncated: bool,
        generation: u64,
    }

    impl FilesSincePort for StubPort {
        fn files_since(&self, _req: &FilesSinceRequest) -> FilesSinceResponse {
            FilesSinceResponse {
                status: Some(Status::ok()),
                paths: self.paths.clone(),
                truncated: self.truncated,
                generation: self.generation,
            }
        }

        fn last_watch_batch(&self) -> WatchBatch {
            WatchBatch {
                events: vec![WatchEvent {
                    path: "a.java".into(),
                    kind: "modify".into(),
                }],
                overflow: false,
                need_rescan: false,
                generation: self.generation,
            }
        }
    }

    struct StubPlane {
        toml: String,
        set_ok: bool,
    }

    impl ControlPlane for StubPlane {
        fn get_config(&self, _req: &GetConfigRequest) -> GetConfigResponse {
            GetConfigResponse {
                status: Some(Status::ok()),
                toml: self.toml.clone(),
            }
        }
        fn set_config(&self, _req: &SetConfigRequest) -> SetConfigResponse {
            SetConfigResponse {
                status: Some(if self.set_ok {
                    Status::ok()
                } else {
                    Status::error(1, "invalid toml")
                }),
            }
        }
        fn reload_config(&self, _req: &ReloadConfigRequest) -> ReloadConfigResponse {
            ReloadConfigResponse {
                status: Some(Status::ok()),
            }
        }
        fn install_packs(&self, req: &InstallPacksRequest) -> InstallPacksResponse {
            let ok = !req.packs.iter().any(|p| p == "bad-hash");
            InstallPacksResponse {
                status: Some(if ok {
                    Status::ok()
                } else {
                    Status::error(1, "hash mismatch")
                }),
            }
        }
        fn watch_subscribe(&self, _req: &WatchSubscribeRequest) -> WatchSubscribeResponse {
            WatchSubscribeResponse {
                status: Some(Status::ok()),
            }
        }
        fn files_since(&self, _req: &FilesSinceRequest) -> FilesSinceResponse {
            FilesSinceResponse {
                status: Some(Status::ok()),
                paths: vec!["dirty.rs".into()],
                truncated: true,
                generation: 9,
            }
        }
        fn last_watch_batch(&self) -> WatchBatch {
            WatchBatch {
                events: vec![WatchEvent {
                    path: "dirty.rs".into(),
                    kind: "create".into(),
                }],
                overflow: true,
                need_rescan: true,
                generation: 9,
            }
        }
        fn take_watch_batches(&self) -> Vec<WatchBatch> {
            vec![self.last_watch_batch()]
        }
        fn index_status(&self, _req: &IndexStatusRequest) -> IndexStatusResponse {
            IndexStatusResponse {
                status: Some(Status::ok()),
                packages: vec![IndexPackage {
                    package_id: "lib".into(),
                    generation: 2,
                }],
                cache_entries: 4,
            }
        }
        fn tier_status(&self, _req: &TierStatusRequest) -> TierStatusResponse {
            TierStatusResponse {
                status: Some(Status::ok()),
                rows: vec![TierRow {
                    package_id: "lib".into(),
                    tier: "graph".into(),
                }],
            }
        }
        fn take_tier_ready(&self) -> Vec<TierReady> {
            vec![TierReady {
                package_id: "lib".into(),
                tier: "graph".into(),
            }]
        }
        fn reload_scripts(&self, _req: &ReloadScriptsRequest) -> ReloadScriptsResponse {
            ReloadScriptsResponse {
                status: Some(Status::ok()),
            }
        }
    }

    #[test]
    fn empty_answers_are_ok_and_not_truncated() {
        let srv = ControlServer::new("packs = []\n");
        assert_eq!(srv.get_config(&GetConfigRequest {}).toml, "packs = []\n");
        assert!(srv.set_config(&SetConfigRequest { patch_toml: String::new() }).status.unwrap().is_ok());
        assert!(srv.reload_config(&ReloadConfigRequest {}).status.unwrap().is_ok());
        assert!(srv
            .install_packs(&InstallPacksRequest { packs: vec!["python".into()] })
            .status
            .unwrap()
            .is_ok());
        assert!(srv.watch_subscribe(&WatchSubscribeRequest {}).status.unwrap().is_ok());
        let batch = srv.empty_watch_batch();
        assert!(batch.events.is_empty());
        assert!(!batch.overflow);
        let fs = srv.files_since(&FilesSinceRequest { since: None });
        assert!(!fs.truncated);
        assert!(fs.paths.is_empty());
        let idx = srv.index_status(&IndexStatusRequest {});
        assert!(idx.packages.is_empty());
        assert_eq!(idx.cache_entries, 0);
        assert!(srv.tier_status(&TierStatusRequest {}).rows.is_empty());
        assert!(srv.reload_scripts(&ReloadScriptsRequest {}).status.unwrap().is_ok());
        assert_eq!(ControlServer::default().config_toml, "");
        assert!(srv.last_watch_batch().events.is_empty());
        assert_eq!(ControlServer::new("a"), ControlServer::new("a"));
        let _ = format!("{:?}", srv);
        let stock = ControlServer::new("");
        assert!(!stock.is_progressive_connected());
        assert!(!stock.push_tier_ready("p", "graph"));
        assert!(stock.take_tier_ready().is_empty());
        let prog = ControlServer::new("").with_progressive(true);
        assert!(prog.is_progressive_connected());
        assert!(prog.push_tier_ready("lib", "graph"));
        let pushed = prog.take_tier_ready();
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].package_id, "lib");
        assert_eq!(pushed[0].tier, "graph");
        assert!(prog.take_tier_ready().is_empty());
    }

    #[test]
    fn wired_files_since_is_not_empty_and_not_jsonrpc() {
        let port = Arc::new(StubPort {
            paths: vec!["src/A.java".into(), "src/B.java".into()],
            truncated: true,
            generation: 8,
        });
        let srv = ControlServer::new("").with_files_since(port);
        let fs = srv.files_since(&FilesSinceRequest {
            since: Some(files_since_request::Since::SinceGeneration(1)),
        });
        assert_eq!(fs.paths, vec!["src/A.java", "src/B.java"]);
        assert!(fs.truncated);
        assert_eq!(fs.generation, 8);
        let batch = srv.last_watch_batch();
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.generation, 8);
        assert_ne!(srv, ControlServer::new(""));
    }

    #[test]
    fn envelope_dispatch_echoes_request_id_and_routes_methods() {
        let srv = ControlServer::new("packs = [\"rust\"]\n");
        for (method, body) in [
            (METHOD_GET_CONFIG, GetConfigRequest {}.encode_to_vec()),
            (METHOD_SET_CONFIG, SetConfigRequest { patch_toml: String::new() }.encode_to_vec()),
            (METHOD_RELOAD_CONFIG, ReloadConfigRequest {}.encode_to_vec()),
            (METHOD_INSTALL_PACKS, InstallPacksRequest { packs: vec![] }.encode_to_vec()),
            (METHOD_WATCH_SUBSCRIBE, WatchSubscribeRequest {}.encode_to_vec()),
            (METHOD_FILES_SINCE, FilesSinceRequest { since: None }.encode_to_vec()),
            (METHOD_INDEX_STATUS, IndexStatusRequest {}.encode_to_vec()),
            (METHOD_TIER_STATUS, TierStatusRequest {}.encode_to_vec()),
            (METHOD_RELOAD_SCRIPTS, ReloadScriptsRequest {}.encode_to_vec()),
        ] {
            let env = Envelope {
                method: method.into(),
                request_id: 42,
                body,
            };
            let reply = srv.dispatch_envelope(&env);
            assert_eq!(reply.method, method);
            assert_eq!(reply.request_id, 42);
            assert!(!reply.body.is_empty() || method == METHOD_GET_CONFIG);
        }
        let get = srv.dispatch_envelope(&Envelope::request(METHOD_GET_CONFIG, 3, GetConfigRequest {}));
        let cfg = get.decode_body::<GetConfigResponse>().unwrap();
        assert_eq!(cfg.toml, "packs = [\"rust\"]\n");
        assert!(cfg.status.unwrap().is_ok());
        let unknown = srv.dispatch_envelope(&Envelope {
            method: "NotAMethod".into(),
            request_id: 9,
            body: vec![],
        });
        assert_eq!(unknown.request_id, 9);
        let st = unknown.decode_body::<Status>().unwrap();
        assert!(!st.is_ok());
        assert!(st.message.contains("unknown method"));
    }

    #[test]
    fn plane_overrides_stubs_and_pushes_use_request_id_zero() {
        let plane = Arc::new(StubPlane {
            toml: "packs = [\"python\"]\n".into(),
            set_ok: false,
        });
        let srv = ControlServer::new("ignored").with_plane(plane).with_progressive(true);
        assert_eq!(srv.get_config(&GetConfigRequest {}).toml, "packs = [\"python\"]\n");
        assert!(!srv
            .set_config(&SetConfigRequest {
                patch_toml: "[[".into(),
            })
            .status
            .unwrap()
            .is_ok());
        assert!(!srv
            .install_packs(&InstallPacksRequest {
                packs: vec!["bad-hash".into()],
            })
            .status
            .unwrap()
            .is_ok());
        assert!(srv
            .install_packs(&InstallPacksRequest {
                packs: vec!["ty".into()],
            })
            .status
            .unwrap()
            .is_ok());
        let idx = srv.index_status(&IndexStatusRequest {});
        assert_eq!(idx.packages[0].package_id, "lib");
        assert_eq!(idx.cache_entries, 4);
        assert_eq!(srv.tier_status(&TierStatusRequest {}).rows[0].tier, "graph");
        assert!(srv.files_since(&FilesSinceRequest { since: None }).truncated);
        assert!(srv.last_watch_batch().overflow);
        srv.push_tier_ready("extra", "syntax");
        let pushes = srv.drain_pushes();
        assert!(pushes.iter().all(|e| e.request_id == 0));
        assert!(pushes.iter().any(|e| e.method == METHOD_WATCH_BATCH));
        assert!(pushes.iter().any(|e| e.method == METHOD_TIER_READY));
        assert_ne!(srv, ControlServer::new("ignored"));
        let _ = format!("{:?}", srv);
    }

    #[test]
    fn mux_and_progressive_fixture_are_protobuf_not_dollar_slash() {
        let port = Arc::new(StubPort {
            paths: vec!["src/A.java".into()],
            truncated: false,
            generation: 3,
        });
        let srv = ControlServer::new("").with_files_since(port).with_progressive(true);
        let fs = srv
            .encode_files_since(&FilesSinceRequest {
                since: Some(files_since_request::Since::SinceGeneration(1)),
            })
            .unwrap();
        let batch = srv.encode_watch_batch().unwrap();
        assert_ne!(fs.first().copied(), Some(b'{'));
        assert_ne!(batch.first().copied(), Some(b'{'));
        assert!(!String::from_utf8_lossy(&fs).contains("$/"));
        assert!(!String::from_utf8_lossy(&batch).contains("$/"));
        let inner = encode_frame(
            &Envelope::request(
                METHOD_FILES_SINCE,
                1,
                FilesSinceRequest {
                    since: Some(files_since_request::Since::SinceUnixMs(9)),
                },
            )
            .encode_to_vec(),
        )
        .unwrap();
        let reply = srv.handle_mux_payload(&inner).unwrap();
        assert_ne!(reply.first().copied(), Some(b'{'));
        assert!(!reply.is_empty());
        assert!(!fs.is_empty());
        assert!(!batch.is_empty());
        assert!(matches!(
            crate::codec::decode_frame(&fs).unwrap(),
            crate::codec::DecodeOutcome::Complete { .. }
        ));
        assert!(matches!(
            crate::codec::decode_frame(&batch).unwrap(),
            crate::codec::DecodeOutcome::Complete { .. }
        ));
        assert!(srv.handle_mux_payload(b"{").is_err());
        assert!(srv.dispatch_payload(&[]).is_err());
        let framed = srv
            .encode_envelope_frame(&Envelope::request(METHOD_GET_CONFIG, 1, GetConfigRequest {}))
            .unwrap();
        assert!(matches!(
            crate::codec::decode_frame(&framed).unwrap(),
            crate::codec::DecodeOutcome::Complete { .. }
        ));
    }
}
