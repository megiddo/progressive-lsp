//! M6 exit: verified install, stock initialize control off, proto FilesSince/WatchBatch.

use std::io::Cursor;
use std::sync::Arc;

use progressive_lsp_control::{
    encode_frame, files_since_request, ControlServer, FilesSincePort, FilesSinceRequest,
    FilesSinceResponse, WatchBatch, WatchEvent,
};
use progressive_lsp_install::{sha256, FakeRemoteTransport, Installer, DIST_PROTO, MUSL_TRIPLES};
use progressive_lsp_protocol::{
    framing, LspFacade, CHANNEL_CONTROL, CHANNEL_LSP,
};
use serde_json::json;

#[test]
fn install_packs_python_produces_verified_prefix() {
    let dir = tempfile::tempdir().unwrap();
    progressive_lsp::run_install(progressive_lsp::InstallOpts {
        prefix: dir.path().to_path_buf(),
        packs: vec!["python".into()],
    })
    .unwrap();
    let prefix = progressive_lsp_core::PrefixLayout::from_path(dir.path());
    let found = progressive_lsp_engine::discover_pack(&prefix, "python").unwrap();
    let bytes = std::fs::read(&found.path).unwrap();
    assert!(progressive_lsp_engine::is_pack_stub(&bytes));
    let hex = progressive_lsp_install::hex_encode(&sha256(&bytes));
    progressive_lsp::verify_existing(&found.path, &hex).unwrap();
    assert!(dir.path().join("engines/python/manifest.json").is_file());
}

#[test]
fn stock_initialize_has_control_off() {
    let facade = LspFacade::new(None, false);
    let body = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
    let mut stdin = framing::encode_message(serde_json::to_vec(&body).unwrap());
    stdin.extend_from_slice(&framing::encode_message(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"exit"})).unwrap(),
    ));
    let mut out = Vec::new();
    facade.serve(Cursor::new(stdin), &mut out).unwrap();
    let texts = framing::decode_all(&out).unwrap();
    let resp: serde_json::Value = serde_json::from_slice(&texts[0]).unwrap();
    let cap = &resp["result"]["capabilities"]["experimental"]["progressiveLsp"];
    assert_eq!(cap["version"], "v1");
    assert!(cap["socket"].is_null());
    assert_eq!(cap["mux"], false);
}

#[test]
fn progressive_files_since_and_watch_batch_are_protobuf_only() {
    struct Port;
    impl FilesSincePort for Port {
        fn files_since(&self, _req: &FilesSinceRequest) -> FilesSinceResponse {
            FilesSinceResponse {
                status: Some(progressive_lsp_control::Status::ok()),
                paths: vec!["src/A.java".into()],
                truncated: false,
                generation: 4,
            }
        }
        fn last_watch_batch(&self) -> WatchBatch {
            WatchBatch {
                events: vec![WatchEvent {
                    path: "src/A.java".into(),
                    kind: "modify".into(),
                }],
                overflow: false,
                need_rescan: false,
                generation: 4,
            }
        }
    }
    let srv = ControlServer::new("")
        .with_files_since(Arc::new(Port))
        .with_progressive(true);
    let fs = srv
        .encode_files_since(&FilesSinceRequest {
            since: Some(files_since_request::Since::SinceGeneration(1)),
        })
        .unwrap();
    let batch = srv.encode_watch_batch().unwrap();
    assert_ne!(fs.first().copied(), Some(b'{'), "FilesSince must not be JSON-RPC");
    assert_ne!(batch.first().copied(), Some(b'{'), "WatchBatch must not be JSON-RPC");
    for bytes in [&fs, &batch] {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("$/"));
        assert!(!text.contains("filesSince"));
        assert!(!text.contains("workspace/filesSince"));
    }
    let facade = LspFacade::new(None, false);
    for method in [
        "$/progressive/filesSince",
        "workspace/filesSince",
        "$/filesSince",
        "$/progressive/watchBatch",
    ] {
        let req = progressive_lsp_protocol::rpc::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params: serde_json::Value::Null,
        };
        let resp = facade.handle_request(&req).unwrap();
        assert_eq!(resp["error"]["code"], -32601, "{method}");
    }
    assert_eq!(DIST_PROTO, "progressive.v1");
    assert_eq!(CHANNEL_LSP, 0);
    assert_eq!(CHANNEL_CONTROL, 1);
}

#[test]
fn fake_remote_transport_hash_mismatch_and_atomic_replace() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("host/bin/progressive-lsp");
    let mut remote = FakeRemoteTransport::new();
    remote.corrupt_hash = true;
    let installer = Installer::new(remote);
    let plan = installer
        .plan(&dest, b"new".to_vec(), sha256(b"new"), true)
        .unwrap();
    let err = installer.apply(&plan).unwrap_err();
    assert!(matches!(err, progressive_lsp_core::InstallError::Hash { .. }));
    assert!(!dest.exists());
    let ops = installer.transport().ops();
    assert!(ops.iter().any(|o| o.starts_with("put ")));
    assert!(!ops.iter().any(|o| o.starts_with("rename ")));

    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&dest, b"old").unwrap();
    let installer = Installer::new(FakeRemoteTransport::new());
    let bytes = b"verified".to_vec();
    let plan = installer
        .plan(&dest, bytes.clone(), sha256(&bytes), true)
        .unwrap();
    installer.apply(&plan).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert!(!plan.tmp.exists());
    let ops = installer.transport().ops();
    assert!(ops.iter().any(|o| o.starts_with("rename ")));
    assert!(ops.iter().any(|o| o.starts_with("hash ")));
    assert!(MUSL_TRIPLES.contains(&"x86_64-unknown-linux-musl"));
    let _ = encode_frame(b"");
}
