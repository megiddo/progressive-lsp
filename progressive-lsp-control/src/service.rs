//! Control facade. FilesSince is protobuf only — never `$/` JSON-RPC.

use std::sync::Arc;

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
}

impl std::fmt::Debug for ControlServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlServer")
            .field("config_toml", &self.config_toml)
            .field("has_files_since", &self.files_since.is_some())
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
        }
    }

    pub fn with_files_since(mut self, port: Arc<dyn FilesSincePort>) -> Self {
        self.files_since = Some(port);
        self
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
        assert_eq!(
            ControlServer::new("a"),
            ControlServer {
                config_toml: "a".into(),
                files_since: None
            }
        );
        let _ = format!("{:?}", srv);
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
}
