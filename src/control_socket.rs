//! Unix-socket Adapter: length-prefixed Envelope beside stdio LSP.

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use progressive_lsp_control::{
    decode_frame, encode_frame, CodecError, ControlServer, DecodeOutcome, Envelope,
};
use progressive_lsp_core::{LogComponent, LogLevel, LogPort, LogScope};

use crate::serve_host::ServeHost;

fn emit_control(log: &dyn LogPort, level: LogLevel, message: &str) {
    let _g = LogScope::enter(
        LogScope::new()
            .operation("control")
            .component(LogComponent::control()),
    );
    match level {
        LogLevel::Error => log.error(message),
        LogLevel::Warn => log.warn(message),
        LogLevel::Info => log.info(message),
        LogLevel::Debug => log.debug(message),
        LogLevel::Trace => log.trace(message),
    }
}

/// Bind `PATH`, removing a stale socket file. Parent dirs are created.
pub fn bind_control_socket(path: &Path, log: Arc<dyn LogPort>) -> io::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            emit_control(
                &*log,
                LogLevel::Warn,
                &format!("control socket bind failed {}: {e}", path.display()),
            );
            return Err(e);
        }
    }
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    match UnixListener::bind(path) {
        Ok(listener) => {
            emit_control(
                &*log,
                LogLevel::Info,
                &format!("control socket bound {}", path.display()),
            );
            Ok(listener)
        }
        Err(e) => {
            emit_control(
                &*log,
                LogLevel::Warn,
                &format!("control socket bind failed {}: {e}", path.display()),
            );
            Err(e)
        }
    }
}

/// Absolute path advertised in `experimental.progressiveLsp.socket`.
pub fn advertised_socket_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn run_accept_loop(
    listener: UnixListener,
    server: Arc<ControlServer>,
    host: Arc<ServeHost>,
    log: Arc<dyn LogPort>,
) {
    let _ = listener.set_nonblocking(false);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let srv = Arc::clone(&server);
                let plane = Arc::clone(&host);
                let conn_log = Arc::clone(&log);
                thread::spawn(move || {
                    if let Err(e) = handle_control_conn(stream, srv, plane, Arc::clone(&conn_log)) {
                        emit_control(
                            &*conn_log,
                            LogLevel::Warn,
                            &format!("control connection error: {e}"),
                        );
                    }
                });
            }
            Err(e) => {
                emit_control(
                    &*log,
                    LogLevel::Warn,
                    &format!("control accept failed: {e}"),
                );
                break;
            }
        }
    }
}

/// Accept loop. Disk poll happens on each accepted connection's read timeout — not `thread::sleep`.
pub fn spawn_control_accept(
    listener: UnixListener,
    server: Arc<ControlServer>,
    host: Arc<ServeHost>,
    log: Arc<dyn LogPort>,
) {
    thread::spawn(move || run_accept_loop(listener, server, host, log));
}

fn handle_control_conn(
    stream: UnixStream,
    server: Arc<ControlServer>,
    host: Arc<ServeHost>,
    log: Arc<dyn LogPort>,
) -> io::Result<()> {
    let mut stream = stream;
    stream.set_read_timeout(Some(Duration::from_millis(80)))?;
    stream.set_write_timeout(Some(Duration::from_millis(80)))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                drain_frames(&mut buf, &server, &host, &mut stream, &*log)?;
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                host.poll_disk_watch();
                write_pushes(&server, &mut stream)?;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn drain_frames<W: Write>(
    buf: &mut Vec<u8>,
    server: &ControlServer,
    host: &ServeHost,
    writer: &mut W,
    log: &dyn LogPort,
) -> io::Result<()> {
    loop {
        match decode_frame(buf) {
            Ok(DecodeOutcome::Complete { payload, consumed }) => {
                let _ = buf.drain(..consumed);
                let env = Envelope::from_bytes(payload.as_slice()).unwrap_or_default();
                if env.method.is_empty() {
                    emit_control(log, LogLevel::Warn, "empty method dropped");
                    continue;
                }
                let reply = server.dispatch_envelope(&env);
                write_frame(writer, &reply.to_bytes())?;
                host.poll_disk_watch();
                write_pushes(server, writer)?;
            }
            Ok(DecodeOutcome::Incomplete { .. }) => break,
            Err(CodecError::PayloadTooLarge(n)) => {
                emit_control(
                    log,
                    LogLevel::Warn,
                    &format!("control payload too large ({n})"),
                );
                buf.clear();
                break;
            }
            Err(_) => break,
        }
    }
    Ok(())
}

fn write_pushes<W: Write>(server: &ControlServer, writer: &mut W) -> io::Result<()> {
    for push in server.drain_pushes() {
        write_frame(writer, &push.to_bytes())?;
    }
    Ok(())
}

fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    let frame = encode_frame(payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writer.write_all(&frame)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_control::{GetConfigRequest, GetConfigResponse, METHOD_GET_CONFIG};
    use std::os::unix::net::UnixStream;

    #[test]
    fn advertised_path_is_absolute_when_cwd_known() {
        let abs = advertised_socket_path(Path::new("/tmp/control.sock"));
        assert!(abs.is_absolute());
        assert_eq!(abs, PathBuf::from("/tmp/control.sock"));
        let rel = advertised_socket_path(Path::new("run/control.sock"));
        assert!(rel.is_absolute() || rel.ends_with("run/control.sock"));
    }

    #[test]
    fn bind_and_one_rpc_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control.sock");
        let log = progressive_lsp_core::FakeLog::new();
        let listener = bind_control_socket(&path, Arc::new(log.clone())).unwrap();
        assert!(path.exists());
        assert!(
            log.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Info
                    && r.operation.as_deref() == Some("control")
                    && r.message.contains("bound")),
            "{:?}",
            log.records()
        );
        let server = Arc::new(ControlServer::new("packs = [\"rust\"]\n"));
        let prefix = tempfile::tempdir().unwrap();
        let layout = progressive_lsp_core::PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        let host = Arc::new(ServeHost::new(layout).unwrap());
        spawn_control_accept(listener, server, host, Arc::new(log.clone()));
        let mut client = UnixStream::connect(&path).unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let env = Envelope::request(METHOD_GET_CONFIG, 11, GetConfigRequest {});
        client
            .write_all(&encode_frame(&env.to_bytes()).unwrap())
            .unwrap();
        client.flush().unwrap();
        let mut got = vec![0u8; 4096];
        let n = client.read(&mut got).unwrap();
        match decode_frame(&got[..n]).unwrap() {
            DecodeOutcome::Complete { payload, .. } => {
                let reply = Envelope::from_bytes(payload.as_slice()).unwrap();
                assert_eq!(reply.request_id, 11);
                assert_eq!(reply.method, METHOD_GET_CONFIG);
                let cfg = reply.decode_body::<GetConfigResponse>().unwrap();
                assert!(cfg.status.unwrap().is_ok());
            }
            other => panic!("{other:?}"),
        }
    }

    fn control_host(
        log: progressive_lsp_core::FakeLog,
    ) -> (tempfile::TempDir, Arc<ServeHost>, ControlServer) {
        let prefix = tempfile::tempdir().unwrap();
        let layout = progressive_lsp_core::PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        let host = Arc::new(ServeHost::new_with_log(layout, Arc::new(log.clone())).unwrap());
        (prefix, host, ControlServer::new(""))
    }

    #[test]
    fn bind_fail_payload_too_large_empty_method_and_accept_err_emit_control() {
        let log = progressive_lsp_core::FakeLog::new();
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("control.sock");
        assert!(bind_control_socket(&path, Arc::new(log.clone())).is_err());
        assert!(
            log.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("control")
                    && r.component.as_ref().map(|c| c.as_str()) == Some("control")
                    && r.message.contains("bind failed")),
            "{:?}",
            log.records()
        );

        let log2 = progressive_lsp_core::FakeLog::new();
        let (_prefix, host, server) = control_host(log2.clone());
        let mut huge = Vec::new();
        huge.extend_from_slice(&(progressive_lsp_control::MAX_PAYLOAD_BYTES + 1).to_be_bytes());
        huge.extend_from_slice(&[0u8; 8]);
        let mut out = Vec::new();
        drain_frames(&mut huge, &server, &host, &mut out, &log2).unwrap();
        assert!(
            log2.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("control")
                    && r.message.contains("payload too large")),
            "{:?}",
            log2.records()
        );
        for r in log2.records() {
            assert!(!r.message.contains('\0'), "payload bytes in message");
        }

        let log3 = progressive_lsp_core::FakeLog::new();
        let (_prefix, host, server) = control_host(log3.clone());
        let empty = Envelope {
            method: String::new(),
            request_id: 1,
            body: b"LEAK_ENVELOPE_BODY".to_vec(),
        };
        let mut buf = encode_frame(&empty.to_bytes()).unwrap();
        let mut out = Vec::new();
        drain_frames(&mut buf, &server, &host, &mut out, &log3).unwrap();
        assert!(
            log3.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("control")
                    && r.message.contains("empty method")),
            "{:?}",
            log3.records()
        );
        for r in log3.records() {
            assert!(
                !r.message.contains("LEAK_ENVELOPE_BODY"),
                "body leaked: {}",
                r.message
            );
        }

        let log4 = progressive_lsp_core::FakeLog::new();
        let (_prefix, host, server) = control_host(log4.clone());
        let (a, b) = UnixStream::pair().unwrap();
        drop(b);
        let fd = {
            use std::os::unix::io::IntoRawFd;
            a.into_raw_fd()
        };
        let fake = unsafe {
            use std::os::unix::io::FromRawFd;
            UnixListener::from_raw_fd(fd)
        };
        run_accept_loop(fake, Arc::new(server), host, Arc::new(log4.clone()));
        assert!(
            log4.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("control")
                    && r.message.contains("accept failed")),
            "{:?}",
            log4.records()
        );
    }
}
