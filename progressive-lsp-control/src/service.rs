//! Control facade. FilesSince is protobuf only — never `$/` JSON-RPC.

use std::sync::Arc;

use prost::Message;

use crate::codec::{decode_exact, decode_frame, encode_frame, CodecError, DecodeOutcome};
use crate::messages::*;

/// Port so the control crate does not own watch internals.
pub trait FilesSincePort: Send + Sync {
    fn files_since(&self, req: &FilesSinceRequest) -> FilesSinceResponse;
    fn last_watch_batch(&self) -> WatchBatch;
}

/// Same domain services as LSP, different encoding.
#[derive(Clone, Default)]
pub struct ControlServer {
    pub config_toml: String,
    files_since: Option<Arc<dyn FilesSincePort>>,
    pending_tier_ready: Arc<std::sync::Mutex<Vec<TierReady>>>,
    progressive_connected: bool,
}

impl std::fmt::Debug for ControlServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlServer")
            .field("config_toml", &self.config_toml)
            .field("has_files_since", &self.files_since.is_some())
            .field("progressive_connected", &self.progressive_connected)
            .finish()
    }
}

impl PartialEq for ControlServer {
    fn eq(&self, other: &Self) -> bool {
        self.config_toml == other.config_toml
            && self.files_since.is_some() == other.files_since.is_some()
    }
}

impl Eq for ControlServer {}

impl ControlServer {
    pub fn new(config_toml: impl Into<String>) -> Self {
        Self {
            config_toml: config_toml.into(),
            files_since: None,
            pending_tier_ready: Arc::new(std::sync::Mutex::new(Vec::new())),
            progressive_connected: false,
        }
    }

    pub fn with_files_since(mut self, port: Arc<dyn FilesSincePort>) -> Self {
        self.files_since = Some(port);
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
        std::mem::take(&mut *self.pending_tier_ready.lock().expect("tier ready lock"))
    }

    pub fn get_config(&self, _req: &GetConfigRequest) -> GetConfigResponse {
        GetConfigResponse {
            status: Some(Status::ok()),
            toml: self.config_toml.clone(),
        }
    }

    pub fn set_config(&self, _req: &SetConfigRequest) -> SetConfigResponse {
        SetConfigResponse {
            status: Some(Status::ok()),
        }
    }

    pub fn reload_config(&self, _req: &ReloadConfigRequest) -> ReloadConfigResponse {
        ReloadConfigResponse {
            status: Some(Status::ok()),
        }
    }

    pub fn install_packs(&self, _req: &InstallPacksRequest) -> InstallPacksResponse {
        InstallPacksResponse {
            status: Some(Status::ok()),
        }
    }

    pub fn watch_subscribe(&self, _req: &WatchSubscribeRequest) -> WatchSubscribeResponse {
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
        match &self.files_since {
            Some(port) => port.last_watch_batch(),
            None => self.empty_watch_batch(),
        }
    }

    pub fn files_since(&self, req: &FilesSinceRequest) -> FilesSinceResponse {
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

    pub fn index_status(&self, _req: &IndexStatusRequest) -> IndexStatusResponse {
        IndexStatusResponse {
            status: Some(Status::ok()),
            packages: Vec::new(),
            cache_entries: 0,
        }
    }

    pub fn tier_status(&self, _req: &TierStatusRequest) -> TierStatusResponse {
        TierStatusResponse {
            status: Some(Status::ok()),
            rows: Vec::new(),
        }
    }

    pub fn reload_scripts(&self, _req: &ReloadScriptsRequest) -> ReloadScriptsResponse {
        ReloadScriptsResponse {
            status: Some(Status::ok()),
        }
    }

    /// Mux control channel: inner payload is length-prefixed proto, never `$/` JSON-RPC.
    pub fn handle_mux_payload(&self, length_prefixed: &[u8]) -> Result<Vec<u8>, CodecError> {
        let inner = decode_exact(length_prefixed)?;
        let req = FilesSinceRequest::decode(inner.as_slice()).unwrap_or_default();
        let resp = self.files_since(&req);
        encode_frame(&resp.encode_to_vec())
    }

    pub fn encode_watch_batch(&self) -> Result<Vec<u8>, CodecError> {
        encode_frame(&self.last_watch_batch().encode_to_vec())
    }

    pub fn encode_files_since(&self, req: &FilesSinceRequest) -> Result<Vec<u8>, CodecError> {
        encode_frame(&self.files_since(req).encode_to_vec())
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
            &FilesSinceRequest {
                since: Some(files_since_request::Since::SinceUnixMs(9)),
            }
            .encode_to_vec(),
        )
        .unwrap();
        let reply = srv.handle_mux_payload(&inner).unwrap();
        assert_ne!(reply.first().copied(), Some(b'{'));
        assert!(!reply.is_empty());
        assert!(!fs.is_empty());
        assert!(!batch.is_empty());
        assert!(matches!(
            decode_frame(&fs).unwrap(),
            DecodeOutcome::Complete { .. }
        ));
        assert!(matches!(
            decode_frame(&batch).unwrap(),
            DecodeOutcome::Complete { .. }
        ));
        assert!(srv.handle_mux_payload(b"{").is_err());
    }
}
