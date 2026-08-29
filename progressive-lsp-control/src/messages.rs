//! Prost types matching `proto/progressive/v1/control.proto`.

use prost::Message;

#[derive(Clone, PartialEq, Eq, Message)]
pub struct Status {
    #[prost(int32, tag = "1")]
    pub code: i32,
    #[prost(string, tag = "2")]
    pub message: String,
}

impl Status {
    pub fn ok() -> Self {
        Self {
            code: 0,
            message: String::new(),
        }
    }

    pub fn error(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.code == 0
    }
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct GetConfigRequest {}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct GetConfigResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
    #[prost(string, tag = "2")]
    pub toml: String,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct SetConfigRequest {
    #[prost(string, tag = "1")]
    pub patch_toml: String,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct SetConfigResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct ReloadConfigRequest {}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct ReloadConfigResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct InstallPacksRequest {
    #[prost(string, repeated, tag = "1")]
    pub packs: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct InstallPacksResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct WatchSubscribeRequest {}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct WatchSubscribeResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct WatchEvent {
    #[prost(string, tag = "1")]
    pub path: String,
    #[prost(string, tag = "2")]
    pub kind: String,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct WatchBatch {
    #[prost(message, repeated, tag = "1")]
    pub events: Vec<WatchEvent>,
    #[prost(bool, tag = "2")]
    pub overflow: bool,
    #[prost(bool, tag = "3")]
    pub need_rescan: bool,
    #[prost(uint64, tag = "4")]
    pub generation: u64,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct FilesSinceRequest {
    #[prost(oneof = "files_since_request::Since", tags = "1, 2")]
    pub since: Option<files_since_request::Since>,
}

pub mod files_since_request {
    #[derive(Clone, PartialEq, Eq, ::prost::Oneof)]
    pub enum Since {
        #[prost(uint64, tag = "1")]
        SinceGeneration(u64),
        #[prost(uint64, tag = "2")]
        SinceUnixMs(u64),
    }
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct FilesSinceResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
    #[prost(string, repeated, tag = "2")]
    pub paths: Vec<String>,
    #[prost(bool, tag = "3")]
    pub truncated: bool,
    #[prost(uint64, tag = "4")]
    pub generation: u64,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct IndexStatusRequest {}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct IndexPackage {
    #[prost(string, tag = "1")]
    pub package_id: String,
    #[prost(uint64, tag = "2")]
    pub generation: u64,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct IndexStatusResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
    #[prost(message, repeated, tag = "2")]
    pub packages: Vec<IndexPackage>,
    #[prost(uint64, tag = "3")]
    pub cache_entries: u64,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct TierStatusRequest {}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct TierRow {
    #[prost(string, tag = "1")]
    pub package_id: String,
    #[prost(string, tag = "2")]
    pub tier: String,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct TierStatusResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
    #[prost(message, repeated, tag = "2")]
    pub rows: Vec<TierRow>,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct TierReady {
    #[prost(string, tag = "1")]
    pub package_id: String,
    #[prost(string, tag = "2")]
    pub tier: String,
}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct ReloadScriptsRequest {}

#[derive(Clone, PartialEq, Eq, Message)]
pub struct ReloadScriptsResponse {
    #[prost(message, optional, tag = "1")]
    pub status: Option<Status>,
}

/// Public dispatch contract. Wrap every control frame in this envelope.
#[derive(Clone, PartialEq, Eq, Message)]
pub struct Envelope {
    #[prost(string, tag = "1")]
    pub method: String,
    #[prost(uint64, tag = "2")]
    pub request_id: u64,
    #[prost(bytes = "vec", tag = "3")]
    pub body: Vec<u8>,
}

impl Envelope {
    pub fn request(method: impl Into<String>, request_id: u64, body: impl Message) -> Self {
        Self {
            method: method.into(),
            request_id,
            body: body.encode_to_vec(),
        }
    }

    pub fn reply(method: impl Into<String>, request_id: u64, body: impl Message) -> Self {
        Self::request(method, request_id, body)
    }

    pub fn push(method: impl Into<String>, body: impl Message) -> Self {
        Self::request(method, 0, body)
    }

    pub fn decode_body<T: Message + Default>(&self) -> Result<T, prost::DecodeError> {
        T::decode(self.body.as_slice())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, prost::DecodeError> {
        Self::decode(bytes)
    }
}

/// RPC / push names. Case-sensitive; match the API RPC table exactly.
pub const METHOD_GET_CONFIG: &str = "GetConfig";
pub const METHOD_SET_CONFIG: &str = "SetConfig";
pub const METHOD_RELOAD_CONFIG: &str = "ReloadConfig";
pub const METHOD_INSTALL_PACKS: &str = "InstallPacks";
pub const METHOD_WATCH_SUBSCRIBE: &str = "WatchSubscribe";
pub const METHOD_WATCH_BATCH: &str = "WatchBatch";
pub const METHOD_FILES_SINCE: &str = "FilesSince";
pub const METHOD_INDEX_STATUS: &str = "IndexStatus";
pub const METHOD_TIER_STATUS: &str = "TierStatus";
pub const METHOD_TIER_READY: &str = "TierReady";
pub const METHOD_RELOAD_SCRIPTS: &str = "ReloadScripts";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_ok_and_error() {
        let ok = Status::ok();
        assert!(ok.is_ok());
        assert_eq!(ok.code, 0);
        assert_eq!(ok.message, "");
        let err = Status::error(5, "hash");
        assert!(!err.is_ok());
        assert_eq!(err.message, "hash");
        assert_eq!(err.code, 5);
        assert_ne!(ok, err);
        assert_ne!(err, Status::default());
        assert_eq!(Status::ok(), Status::default());
    }

    #[test]
    fn envelope_request_reply_push_and_decode() {
        let req = Envelope::request(METHOD_GET_CONFIG, 7, GetConfigRequest {});
        assert_eq!(req.method, METHOD_GET_CONFIG);
        assert_eq!(req.request_id, 7);
        assert!(req.decode_body::<GetConfigRequest>().is_ok());
        let push = Envelope::push(
            METHOD_WATCH_BATCH,
            WatchBatch {
                events: vec![],
                overflow: false,
                need_rescan: false,
                generation: 1,
            },
        );
        assert_eq!(push.request_id, 0);
        assert_eq!(push.method, METHOD_WATCH_BATCH);
        let reply = Envelope::reply(METHOD_GET_CONFIG, 7, GetConfigResponse::default());
        assert_eq!(reply.request_id, 7);
        assert_ne!(req.encode_to_vec(), push.encode_to_vec());
        assert_eq!(req.to_bytes(), req.encode_to_vec());
        assert_eq!(Envelope::from_bytes(&req.to_bytes()).unwrap(), req);
    }

    #[test]
    fn files_since_oneof_tags_are_distinct() {
        let a = FilesSinceRequest {
            since: Some(files_since_request::Since::SinceGeneration(1)),
        };
        let b = FilesSinceRequest {
            since: Some(files_since_request::Since::SinceUnixMs(1)),
        };
        assert_ne!(a.encode_to_vec(), b.encode_to_vec());
    }
}
