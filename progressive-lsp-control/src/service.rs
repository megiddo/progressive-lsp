//! Control facade. M0 answers are empty and valid.

use crate::messages::*;

/// Same domain services as LSP, different encoding. Empty index/watch answers are OK.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlServer {
    pub config_toml: String,
}

impl ControlServer {
    pub fn new(config_toml: impl Into<String>) -> Self {
        Self {
            config_toml: config_toml.into(),
        }
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

    pub fn files_since(&self, _req: &FilesSinceRequest) -> FilesSinceResponse {
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
    }
}
