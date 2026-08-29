//! `progressive.v1` codec: length-prefixed protobuf. FilesSince is not LSP.

pub mod codec;
pub mod messages;
pub mod service;

pub use codec::{
    decode_frame, encode_frame, CodecError, DecodeOutcome, MAX_PAYLOAD_BYTES,
};
pub use messages::*;
pub use prost;
pub use service::{ControlPlane, ControlServer, FilesSincePort};

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    #[test]
    fn proto_round_trip_every_rpc_type() {
        let status = Status {
            code: 0,
            message: "ok".into(),
        };
        assert_round_trip(&status);

        let cfg = GetConfigResponse {
            status: Some(status.clone()),
            toml: "packs = []\n".into(),
        };
        assert_round_trip(&cfg);
        assert_round_trip(&GetConfigRequest {});
        assert_round_trip(&SetConfigRequest {
            patch_toml: "packs = [\"rust\"]".into(),
        });
        assert_round_trip(&SetConfigResponse {
            status: Some(status.clone()),
        });
        assert_round_trip(&ReloadConfigRequest {});
        assert_round_trip(&ReloadConfigResponse {
            status: Some(status.clone()),
        });
        assert_round_trip(&InstallPacksRequest {
            packs: vec!["python".into(), "rust".into()],
        });
        assert_round_trip(&InstallPacksResponse {
            status: Some(status.clone()),
        });
        assert_round_trip(&WatchSubscribeRequest {});
        assert_round_trip(&WatchSubscribeResponse {
            status: Some(status.clone()),
        });

        let batch = WatchBatch {
            events: vec![WatchEvent {
                path: "src/A.java".into(),
                kind: "modify".into(),
            }],
            overflow: true,
            need_rescan: true,
            generation: 7,
        };
        assert_round_trip(&batch);

        let fs_gen = FilesSinceRequest {
            since: Some(files_since_request::Since::SinceGeneration(3)),
        };
        assert_round_trip(&fs_gen);
        let fs_ms = FilesSinceRequest {
            since: Some(files_since_request::Since::SinceUnixMs(99)),
        };
        assert_round_trip(&fs_ms);
        assert_round_trip(&FilesSinceResponse {
            status: Some(status.clone()),
            paths: vec!["a".into()],
            truncated: true,
            generation: 4,
        });

        assert_round_trip(&IndexStatusRequest {});
        assert_round_trip(&IndexStatusResponse {
            status: Some(status.clone()),
            packages: vec![IndexPackage {
                package_id: "p".into(),
                generation: 1,
            }],
            cache_entries: 2,
        });
        assert_round_trip(&TierStatusRequest {});
        assert_round_trip(&TierStatusResponse {
            status: Some(status),
            rows: vec![TierRow {
                package_id: "p".into(),
                tier: "syntax".into(),
            }],
        });
        assert_round_trip(&TierReady {
            package_id: "p".into(),
            tier: "graph".into(),
        });
        assert_round_trip(&ReloadScriptsRequest {});
        assert_round_trip(&ReloadScriptsResponse { status: None });
        assert_round_trip(&Envelope::request(METHOD_GET_CONFIG, 1, GetConfigRequest {}));
        assert_eq!(METHOD_TIER_READY, "TierReady");
        assert_eq!(METHOD_WATCH_BATCH, "WatchBatch");
    }

    fn assert_round_trip<T: Message + Default + PartialEq + std::fmt::Debug>(msg: &T) {
        let framed = encode_frame(&msg.encode_to_vec()).unwrap();
        let (payload, consumed) = match decode_frame(&framed).unwrap() {
            DecodeOutcome::Complete { payload, consumed } => (payload, consumed),
            other => panic!("expected complete, got {other:?}"),
        };
        assert_eq!(consumed, framed.len());
        let decoded = T::decode(payload.as_slice()).unwrap();
        assert_eq!(&decoded, msg);
    }

    #[test]
    fn empty_defaults_round_trip() {
        let empty = FilesSinceResponse::default();
        assert!(!empty.truncated);
        assert!(empty.paths.is_empty());
        assert_round_trip(&empty);
        assert_round_trip(&WatchBatch::default());
        assert_round_trip(&IndexStatusResponse::default());
        assert_round_trip(&GetConfigRequest::default());
        assert_round_trip(&FilesSinceRequest::default());
    }
}
