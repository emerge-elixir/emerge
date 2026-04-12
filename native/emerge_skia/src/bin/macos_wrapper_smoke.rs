#[cfg(all(feature = "macos", target_os = "macos"))]
mod app {
    use std::{
        env,
        io::{Read, Write},
        os::unix::net::UnixStream,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    const PROTOCOL_NAME: &str = "emerge_skia_macos";
    const PROTOCOL_VERSION: u16 = 2;
    const FRAME_INIT: u8 = 1;
    const FRAME_INIT_OK: u8 = 2;
    const FRAME_REQUEST: u8 = 3;
    const FRAME_REPLY: u8 = 4;
    const FRAME_ERROR: u8 = 6;
    const REQUEST_SHUTDOWN_HOST: u16 = 0x0015;

    pub fn run() -> Result<(), String> {
        let project_root = project_root();
        let wrapper_binary = std::env::current_exe()
            .map_err(|err| format!("failed to locate wrapper executable: {err}"))?;
        let host_binary = wrapper_binary.with_file_name("macos_host");

        if !host_binary.is_file() {
            return Err(format!(
                "macOS host binary not found next to wrapper: {}",
                host_binary.display()
            ));
        }

        let socket_path = default_socket_path();
        let mut host_child = launch_host(&host_binary, &socket_path)?;

        let result = (|| {
            wait_for_host(&socket_path)?;
            let hello_reply = init(&socket_path)?;
            println!(
                "wrapper connected to host id={} pid={} socket={}",
                hello_reply.host_id,
                hello_reply.host_pid,
                socket_path.display()
            );

            let status = launch_beam_child(&project_root, &socket_path)?;

            if !status.success() {
                return Err(format!("BEAM child exited with status {status}"));
            }

            let hello_after = init(&socket_path)?;
            println!(
                "wrapper reconnected after BEAM child exit to host id={} pid={}",
                hello_after.host_id, hello_after.host_pid
            );

            if hello_after.host_id != hello_reply.host_id
                || hello_after.host_pid != hello_reply.host_pid
            {
                return Err(
                    "macOS host identity changed unexpectedly during wrapper smoke".to_string(),
                );
            }

            shutdown_host(&socket_path)?;
            Ok(())
        })();

        let _ = wait_for_child_exit(&mut host_child, Duration::from_secs(5));

        if let Err(err) = result {
            let _ = kill_child(&mut host_child);
            return Err(err);
        }

        println!("wrapper smoke passed");
        Ok(())
    }

    struct HelloReply {
        host_id: u64,
        host_pid: u32,
    }

    fn launch_host(host_binary: &Path, socket_path: &Path) -> Result<Child, String> {
        Command::new(host_binary)
            .arg("--socket")
            .arg(socket_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|err| format!("failed to launch macOS host: {err}"))
    }

    fn launch_beam_child(
        project_root: &Path,
        socket_path: &Path,
    ) -> Result<std::process::ExitStatus, String> {
        let mise = env::var("MISE_BIN")
            .ok()
            .map(PathBuf::from)
            .or_else(|| find_executable("mise"))
            .unwrap_or_else(|| PathBuf::from("/usr/local/bin/mise"));

        Command::new(&mise)
            .args(["x", "--", "mix", "run", "macos_wrapper_beam_child.exs"])
            .current_dir(project_root)
            .env("EMERGE_SKIA_MACOS_HOST_SOCKET", socket_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|err| format!("failed to launch BEAM child via {}: {err}", mise.display()))
    }

    fn wait_for_host(socket_path: &Path) -> Result<(), String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);

        while std::time::Instant::now() < deadline {
            if let Ok(reply) = init(socket_path) {
                println!(
                    "host ready id={} pid={} socket={}",
                    reply.host_id,
                    reply.host_pid,
                    socket_path.display()
                );
                return Ok(());
            }

            thread::sleep(Duration::from_millis(50));
        }

        Err(format!(
            "timed out waiting for macOS host socket {}",
            socket_path.display()
        ))
    }

    fn init(socket_path: &Path) -> Result<HelloReply, String> {
        let mut stream = UnixStream::connect(socket_path).map_err(|err| {
            format!(
                "failed connecting to host socket {}: {err}",
                socket_path.display()
            )
        })?;

        write_frame(
            &mut stream,
            &encode_frame(FRAME_INIT, 0, 0, 0, &init_payload()),
        )
        .map_err(|err| format!("failed sending init request: {err}"))?;

        let response = read_frame(&mut stream)
            .map_err(|err| format!("failed reading init response: {err}"))?;
        decode_init_response(&response)
    }

    fn shutdown_host(socket_path: &Path) -> Result<(), String> {
        let mut stream = UnixStream::connect(socket_path)
            .map_err(|err| format!("failed connecting to host socket for shutdown: {err}"))?;

        write_frame(
            &mut stream,
            &encode_frame(FRAME_INIT, 0, 0, 0, &init_payload()),
        )
        .map_err(|err| format!("failed sending init before shutdown: {err}"))?;

        let init_response = read_frame(&mut stream)
            .map_err(|err| format!("failed reading init response before shutdown: {err}"))?;
        let _ = decode_init_response(&init_response)?;

        write_frame(
            &mut stream,
            &encode_frame(FRAME_REQUEST, 1, 0, REQUEST_SHUTDOWN_HOST, &[]),
        )
        .map_err(|err| format!("failed sending shutdown request: {err}"))?;

        let response = read_frame(&mut stream)
            .map_err(|err| format!("failed reading shutdown response: {err}"))?;

        match decode_frame(&response)? {
            Frame {
                frame_type: FRAME_REPLY,
                request_id: 1,
                tag: REQUEST_SHUTDOWN_HOST,
                ..
            } => Ok(()),
            Frame {
                frame_type: FRAME_ERROR,
                payload,
                ..
            } => Err(decode_error_payload(&payload)?),
            other => Err(format!("unexpected shutdown response: {:?}", other)),
        }
    }

    fn decode_init_response(response: &[u8]) -> Result<HelloReply, String> {
        match decode_frame(response)? {
            Frame {
                frame_type: FRAME_INIT_OK,
                payload,
                ..
            } => decode_init_ok_payload(&payload),
            Frame {
                frame_type: FRAME_ERROR,
                payload,
                ..
            } => Err(decode_error_payload(&payload)?),
            other => Err(format!("unexpected init response: {:?}", other)),
        }
    }

    fn decode_error_payload(payload: &[u8]) -> Result<String, String> {
        String::from_utf8(payload.to_vec())
            .map_err(|err| format!("invalid utf-8 error payload: {err}"))
    }

    fn init_payload() -> Vec<u8> {
        let protocol_name = PROTOCOL_NAME.as_bytes();
        let mut out = Vec::with_capacity(2 + protocol_name.len() + 2);
        out.extend_from_slice(&(protocol_name.len() as u16).to_be_bytes());
        out.extend_from_slice(protocol_name);
        out.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
        out
    }

    #[derive(Debug)]
    struct Frame {
        frame_type: u8,
        request_id: u32,
        _session_id: u64,
        tag: u16,
        payload: Vec<u8>,
    }

    fn encode_frame(
        frame_type: u8,
        request_id: u32,
        session_id: u64,
        tag: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 4 + 8 + 2 + payload.len());
        out.push(frame_type);
        out.extend_from_slice(&request_id.to_be_bytes());
        out.extend_from_slice(&session_id.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn decode_frame(bytes: &[u8]) -> Result<Frame, String> {
        if bytes.len() < 15 {
            return Err("frame too short".to_string());
        }

        Ok(Frame {
            frame_type: bytes[0],
            request_id: u32::from_be_bytes(bytes[1..5].try_into().unwrap()),
            _session_id: u64::from_be_bytes(bytes[5..13].try_into().unwrap()),
            tag: u16::from_be_bytes(bytes[13..15].try_into().unwrap()),
            payload: bytes[15..].to_vec(),
        })
    }

    fn decode_init_ok_payload(payload: &[u8]) -> Result<HelloReply, String> {
        if payload.len() < 2 {
            return Err("invalid init_ok payload".to_string());
        }

        let name_len = u16::from_be_bytes(payload[0..2].try_into().unwrap()) as usize;

        if payload.len() != 2 + name_len + 2 + 8 + 4 {
            return Err("invalid init_ok payload size".to_string());
        }

        let protocol_name = String::from_utf8(payload[2..2 + name_len].to_vec())
            .map_err(|err| format!("invalid protocol name: {err}"))?;
        let version = u16::from_be_bytes(payload[2 + name_len..4 + name_len].try_into().unwrap());

        if protocol_name != PROTOCOL_NAME || version != PROTOCOL_VERSION {
            return Err(format!(
                "unexpected init_ok protocol {protocol_name} version {version}"
            ));
        }

        let host_id = u64::from_be_bytes(payload[4 + name_len..12 + name_len].try_into().unwrap());
        let host_pid =
            u32::from_be_bytes(payload[12 + name_len..16 + name_len].try_into().unwrap());
        Ok(HelloReply { host_id, host_pid })
    }

    fn read_frame(stream: &mut UnixStream) -> std::io::Result<Vec<u8>> {
        let mut len_buf = [0_u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0_u8; len];
        stream.read_exact(&mut payload)?;
        Ok(payload)
    }

    fn write_frame(stream: &mut UnixStream, payload: &[u8]) -> std::io::Result<()> {
        stream.write_all(&(payload.len() as u32).to_be_bytes())?;
        stream.write_all(payload)?;
        stream.flush()
    }

    fn default_socket_path() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("emerge_skia_macos_wrapper_{stamp}.sock"))
    }

    fn project_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .expect("native/emerge_skia should have a project root parent")
    }

    fn find_executable(name: &str) -> Option<PathBuf> {
        let path = env::var_os("PATH")?;

        env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    }

    fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;

        while std::time::Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        return Ok(());
                    }

                    return Err(format!("child exited with non-zero status {status}"));
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(err) => return Err(format!("failed waiting for child exit: {err}")),
            }
        }

        Err("timed out waiting for child exit".to_string())
    }

    fn kill_child(child: &mut Child) -> Result<(), String> {
        match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => child
                .kill()
                .map_err(|err| format!("failed killing child process: {err}")),
            Err(err) => Err(format!("failed checking child status: {err}")),
        }
    }
}

#[cfg(all(feature = "macos", target_os = "macos"))]
fn main() {
    if let Err(reason) = app::run() {
        eprintln!("macOS wrapper smoke failed: {reason}");
        std::process::exit(1);
    }
}

#[cfg(not(all(feature = "macos", target_os = "macos")))]
fn main() {
    eprintln!("macos_wrapper_smoke can only run on macOS");
    std::process::exit(1);
}
