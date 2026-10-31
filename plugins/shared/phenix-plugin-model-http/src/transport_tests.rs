use super::*;
use phenix_provider_protocols::{EndpointMode, ResolvedDestination};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Child, Command, Stdio},
};

const SSE: &str = concat!(
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"pinned\"},",
    "\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
    "data: [DONE]\n\n"
);

fn config(endpoint: String, ca_file: Option<String>) -> Config {
    Config {
        endpoint,
        model: "fixture".into(),
        protocol: phenix_provider_protocols::Protocol::OpenaiChat,
        provider_max_tokens: None,
        credential: Some(
            phenix_provider_contracts::CredentialSecret::new(b"fixture-secret".to_vec()).unwrap(),
        ),
        presentation: phenix_provider_contracts::ApiKeyPresentation::Bearer,
        ca_file,
    }
}

fn run(
    config: Config,
    resolve: impl FnOnce(
        &str,
    ) -> Result<ResolvedDestination, phenix_provider_protocols::EndpointProblem>,
) -> Result<ModelReply, BasicError> {
    let request = ModelRequest {
        session: "fixture".into(),
        run: "fixture".into(),
        history: vec![],
        tools: vec![],
        skills: vec![],
        context: vec![],
        limits: phenix_basic_contracts::Limits {
            http_timeout_ms: Some(5000),
            ..Default::default()
        },
    };
    run_with_resolver(
        config,
        b"{}".to_vec(),
        request,
        Arc::new(Mutex::new(String::new())),
        Arc::new(AtomicBool::new(false)),
        || {},
        resolve,
    )
}

#[test]
fn ipv6_literal_loopback_uses_real_curl_resolve_entry() {
    let listener = match TcpListener::bind("[::1]:0") {
        Ok(listener) => listener,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
            ) =>
        {
            eprintln!("IPv6 loopback unavailable: {error}");
            return;
        }
        Err(error) => panic!("IPv6 loopback bind failed: {error}"),
    };
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline, "no IPv6 connection");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("IPv6 accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n{}") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{SSE}", SSE.len()).unwrap();
        String::from_utf8(request).unwrap()
    });
    let outcome = run(
        config(format!("http://[::1]:{port}/completions"), None),
        |endpoint| {
            let destination = phenix_provider_protocols::resolve_destination(
                endpoint,
                EndpointMode::LocalLoopback,
            )?;
            assert_eq!(destination.resolve_entry, format!("[::1]:{port}:[::1]"));
            Ok(destination)
        },
    );
    let captured = server.join().unwrap();
    assert_eq!(outcome.unwrap().text, "pinned");
    assert!(captured.contains(&format!("Host: [::1]:{port}\r\n")));
    assert!(captured.contains("Authorization: Bearer fixture-secret\r\n"));
}

struct Fixture(Child);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn pinned_invalid_hostname_preserves_host_sni_and_tls_verification() {
    assert!(
        curl::Version::get().feature_ssl(),
        "TLS-enabled system curl is required; use nix develop"
    );
    // Test-only authorization of a loopback fixture. Production always calls
    // resolve_destination, which rejects hostname answers containing loopback.
    let mut fixture = Fixture(Command::new("python3").args(["-u", "-c", r#"
import json, pathlib, socket, ssl, subprocess, tempfile
with tempfile.TemporaryDirectory() as directory:
    cert = pathlib.Path(directory) / 'cert.pem'
    key = pathlib.Path(directory) / 'key.pem'
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                    '-days', '1', '-subj', '/CN=pinned.invalid',
                    '-addext', 'subjectAltName=DNS:pinned.invalid',
                    '-keyout', str(key), '-out', str(cert)],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    names = []
    context.set_servername_callback(lambda sock, name, ctx: names.append(name))
    with socket.socket() as server:
        server.bind(('127.0.0.1', 0))
        server.listen()
        server.settimeout(10)
        print(json.dumps({'port': server.getsockname()[1], 'ca': str(cert)}), flush=True)
        for _ in range(3):
            client, _ = server.accept()
            client.settimeout(5)
            request = b''
            try:
                with context.wrap_socket(client, server_side=True) as stream:
                    while not request.endswith(b'\r\n\r\n{}'):
                        chunk = stream.recv(4096)
                        if not chunk:
                            break
                        request += chunk
                    body = %s.encode()
                    stream.sendall(('HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: '
                                    + str(len(body)) + '\r\nConnection: close\r\n\r\n').encode() + body)
            except (ssl.SSLError, ConnectionError):
                client.close()
            print(json.dumps({'sni': names[-1], 'request': request.decode()}), flush=True)
"#.replace("%s", &serde_json::to_string(SSE).unwrap()).as_str()])
        .stdout(Stdio::piped()).spawn().expect("python3 and openssl are required"));
    let mut output = BufReader::new(fixture.0.stdout.take().unwrap());
    let mut read_record = || {
        let mut line = String::new();
        assert_ne!(
            output.read_line(&mut line).unwrap(),
            0,
            "TLS fixture exited early"
        );
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    let ready = read_record();
    let port = ready["port"].as_u64().unwrap();
    let ca = ready["ca"].as_str().unwrap();
    for (host, trust_ca, success) in [
        ("pinned.invalid", true, true),
        ("wrong.invalid", true, false),
        ("pinned.invalid", false, false),
    ] {
        let mut resolutions = 0;
        let dns_answer = std::cell::Cell::new(std::net::Ipv4Addr::LOCALHOST);
        let outcome = run(
            config(
                format!("https://{host}:{port}/completions"),
                trust_ca.then(|| ca.to_owned()),
            ),
            |endpoint| {
                resolutions += 1;
                // Change the injected DNS answer before curl runs. Only the
                // authorized snapshot can reach the fixture; .invalid cannot
                // be resolved by curl's ordinary DNS path either.
                assert_eq!(resolutions, 1);
                let address = dns_answer.replace(std::net::Ipv4Addr::new(192, 0, 2, 1));
                Ok(ResolvedDestination {
                    url: endpoint.into(),
                    // The first authorized fixture address refuses connections;
                    // curl must retain fallback within the pinned snapshot.
                    resolve_entry: format!("{host}:{port}:127.0.0.2,{address}"),
                })
            },
        );
        assert_eq!(resolutions, 1);
        assert_eq!(dns_answer.get(), std::net::Ipv4Addr::new(192, 0, 2, 1));
        let captured = read_record();
        assert_eq!(captured["sni"], host);
        if success {
            assert_eq!(outcome.unwrap().text, "pinned");
            let request = captured["request"].as_str().unwrap();
            assert!(request.starts_with("POST /completions HTTP/1.1\r\n"));
            assert!(request.contains(&format!("Host: {host}:{port}\r\n")));
            assert!(request.contains("Authorization: Bearer fixture-secret\r\n"));
        } else {
            assert_eq!(outcome.unwrap_err(), failed("HTTP transport failed"));
            assert_eq!(
                captured["request"], "",
                "TLS failure must precede credential transmission"
            );
        }
    }
    assert!(fixture.0.wait().unwrap().success());
}
