//! The SSH client paths against a real SSH server on localhost (russh's own
//! server), so an upgrade of russh is checked end to end: sign-in with a
//! password, a key file and a certificate, a key stored in LiteTerm, a shell
//! channel, and a direct-tcpip channel like a port forward opens.

use std::sync::Arc;
use std::time::Duration;

use russh::keys::ssh_key::certificate::{Builder, CertType};
use russh::keys::{Algorithm, Certificate, PrivateKey, PublicKey};
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{authenticate, AuthSpec, ConnParams, KeySource};

#[derive(Clone)]
struct TestServer {
    /// The plain key it trusts, if any
    user_key: Option<PublicKey>,
    /// The CA whose certificates it trusts, if any
    ca: Option<PublicKey>,
}

impl server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == "ops" && password == "pw" { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_publickey(&mut self, user: &str, key: server::VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
        let trusted = self.user_key.as_ref().is_some_and(|k| k.key_data() == key.public_key.key_data());
        Ok(if user == "ops" && trusted { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_openssh_certificate(&mut self, user: &str, cert: &Certificate) -> Result<Auth, Self::Error> {
        let by_ca = self.ca.as_ref().is_some_and(|ca| cert.signature_key() == ca.key_data());
        let for_user = cert.valid_principals().iter().any(|p| p == user);
        Ok(if by_ca && for_user { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        tokio::spawn(echo(channel));
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pw: u32,
        _ph: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        session.data(channel, b"welcome\r\n".to_vec())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _originator: &str,
        _originator_port: u32,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if host == "db.internal" && port == 5432 {
            reply.accept().await;
            tokio::spawn(echo(channel));
        } else {
            reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
        }
        Ok(())
    }
}

/// Everything the client sends comes back
async fn echo(channel: Channel<Msg>) {
    let (mut r, mut w) = tokio::io::split(channel.into_stream());
    let _ = tokio::io::copy(&mut r, &mut w).await;
}

struct TestClient;

impl russh::client::Handler for TestClient {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _key: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

fn new_key() -> PrivateKey {
    PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).unwrap()
}

/// A server on a free localhost port; every connection gets `handler`
async fn start_server(handler: TestServer) -> u16 {
    let config = Arc::new(server::Config {
        keys: vec![new_key()],
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::from_millis(1)),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let config = config.clone();
            let handler = handler.clone();
            tokio::spawn(async move {
                if let Ok(running) = server::run_stream(config, sock, handler).await {
                    let _ = running.await;
                }
            });
        }
    });
    port
}

async fn dial(port: u16) -> russh::client::Handle<TestClient> {
    let config = Arc::new(russh::client::Config::default());
    russh::client::connect(config, ("127.0.0.1", port), TestClient).await.unwrap()
}

fn hop(auth: AuthSpec) -> ConnParams {
    ConnParams { address: "127.0.0.1".into(), port: 0, username: "ops".into(), auth, label: "test".into(), jump: None }
}

/// A key file, and optionally a certificate for it next to it, in a fresh folder
fn key_files(name: &str, key: &PrivateKey, cert: Option<&Certificate>) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("liteterm-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("id_ed25519");
    std::fs::write(&path, key.to_openssh(russh::keys::ssh_key::LineEnding::LF).unwrap().as_bytes()).unwrap();
    if let Some(cert) = cert {
        std::fs::write(dir.join("id_ed25519-cert.pub"), cert.to_openssh().unwrap()).unwrap();
    }
    (dir, path.to_string_lossy().into_owned())
}

fn sign(ca: &PrivateKey, key: &PrivateKey, valid_after: u64, valid_before: u64) -> Certificate {
    let mut b = Builder::new_with_random_nonce(
        &mut russh::keys::key::safe_rng(),
        key.public_key().key_data().clone(),
        valid_after,
        valid_before,
    )
    .unwrap();
    b.serial(1).unwrap();
    b.key_id("e2e").unwrap();
    b.cert_type(CertType::User).unwrap();
    b.valid_principal("ops").unwrap();
    b.sign(ca).unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

#[tokio::test]
async fn a_password_signs_in_and_a_wrong_one_does_not() {
    let port = start_server(TestServer { user_key: None, ca: None }).await;
    let mut ok = dial(port).await;
    assert!(authenticate(&mut ok, &hop(AuthSpec::Password("pw".into()))).await.ok().unwrap());
    let mut bad = dial(port).await;
    assert!(!authenticate(&mut bad, &hop(AuthSpec::Password("nope".into()))).await.ok().unwrap());
}

#[tokio::test]
async fn a_key_file_signs_in() {
    let key = new_key();
    let port = start_server(TestServer { user_key: Some(key.public_key().clone()), ca: None }).await;
    let (dir, path) = key_files("key", &key, None);
    let mut handle = dial(port).await;
    let signed_in = authenticate(&mut handle, &hop(AuthSpec::Key { source: KeySource::File(path), passphrase: None })).await.ok();
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(signed_in, Some(true));
}

#[tokio::test]
async fn a_key_stored_in_liteterm_signs_in() {
    let key = new_key();
    let port = start_server(TestServer { user_key: Some(key.public_key().clone()), ca: None }).await;
    let text = key.to_openssh(russh::keys::ssh_key::LineEnding::LF).unwrap();
    let mut handle = dial(port).await;
    let auth = AuthSpec::Key { source: KeySource::Stored(text), passphrase: None };
    assert_eq!(authenticate(&mut handle, &hop(auth)).await.ok(), Some(true));
}

#[tokio::test]
async fn a_certificate_next_to_the_key_signs_in_where_the_key_alone_would_not() {
    let (ca, key) = (new_key(), new_key());
    // Trusts the CA only, not the key
    let port = start_server(TestServer { user_key: None, ca: Some(ca.public_key().clone()) }).await;

    let cert = sign(&ca, &key, now() - 60, now() + 3600);
    let (dir, path) = key_files("cert", &key, Some(&cert));
    let mut handle = dial(port).await;
    let signed_in = authenticate(&mut handle, &hop(AuthSpec::Key { source: KeySource::File(path.clone()), passphrase: None })).await.ok();
    assert_eq!(signed_in, Some(true));

    // Expired: skipped, the plain key is refused, and the error names the file
    let old = sign(&ca, &key, now() - 7200, now() - 3600);
    std::fs::write(std::path::Path::new(&path).with_file_name("id_ed25519-cert.pub"), old.to_openssh().unwrap()).unwrap();
    let mut handle = dial(port).await;
    let err = authenticate(&mut handle, &hop(AuthSpec::Key { source: KeySource::File(path), passphrase: None })).await.err().map(|e| e.msg);
    std::fs::remove_dir_all(dir).unwrap();
    assert!(err.as_deref().is_some_and(|m| m.contains("id_ed25519-cert.pub")), "{err:?}");
}

#[tokio::test]
async fn a_shell_and_a_forward_channel_carry_data() {
    let port = start_server(TestServer { user_key: None, ca: None }).await;
    let mut handle = dial(port).await;
    assert!(authenticate(&mut handle, &hop(AuthSpec::Password("pw".into()))).await.ok().unwrap());

    // The terminal: PTY, shell, the server's greeting, then an echoed key
    let mut shell = handle.channel_open_session().await.unwrap();
    shell.request_pty(true, "xterm-256color", 80, 24, 0, 0, &[]).await.unwrap();
    shell.request_shell(true).await.unwrap();
    let mut seen = Vec::new();
    let until = |seen: &Vec<u8>, want: &[u8]| seen.windows(want.len()).any(|w| w == want);
    let mut writer = shell.make_writer();
    let mut sent = false;
    while !until(&seen, b"ping") {
        let msg = tokio::time::timeout(Duration::from_secs(5), shell.wait()).await.expect("no output").expect("closed");
        if let russh::ChannelMsg::Data { data } = msg {
            seen.extend_from_slice(&data);
        }
        if !sent && until(&seen, b"welcome") {
            writer.write_all(b"ping").await.unwrap();
            writer.flush().await.unwrap();
            sent = true;
        }
    }

    // A local forward's channel: the server connects on, here it echoes
    let channel = handle.channel_open_direct_tcpip("db.internal", 5432, "127.0.0.1", 50000).await.unwrap();
    let mut stream = channel.into_stream();
    stream.write_all(b"SELECT 1").await.unwrap();
    let mut buf = [0u8; 8];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut buf)).await.unwrap().unwrap();
    assert_eq!(&buf, b"SELECT 1");

    // A destination the server can't reach is refused, not left hanging
    let refused = handle.channel_open_direct_tcpip("nowhere", 1, "127.0.0.1", 50001).await;
    assert!(matches!(refused, Err(russh::Error::ChannelOpenFailure(_))), "{refused:?}");
}

/// russh's own SSH agent on a private pipe or socket, made the one this
/// process uses (SSH_AUTH_SOCK). Returns where it listens.
async fn start_agent() -> String {
    #[cfg(windows)]
    let (path, incoming) = {
        use tokio::net::windows::named_pipe::ServerOptions;
        let path = format!(r"\\.\pipe\liteterm-test-agent-{}", std::process::id());
        let first = ServerOptions::new().first_pipe_instance(true).create(&path).unwrap();
        let incoming = futures::stream::unfold((first, path.clone()), |(server, path)| async move {
            server.connect().await.ok()?;
            let next = ServerOptions::new().create(&path).ok()?;
            Some((Ok::<_, std::io::Error>(server), (next, path)))
        });
        (path, incoming)
    };
    #[cfg(unix)]
    let (path, incoming) = {
        let path = std::env::temp_dir().join(format!("liteterm-test-agent-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let incoming = futures::stream::unfold(listener, |listener| async move {
            let accepted = listener.accept().await.map(|(s, _)| s);
            Some((accepted, listener))
        });
        (path.to_string_lossy().into_owned(), incoming)
    };
    tokio::spawn(russh::keys::agent::server::serve(Box::pin(incoming), ()));
    std::env::set_var("SSH_AUTH_SOCK", &path);
    path
}

#[tokio::test]
async fn the_agent_signs_in_with_the_key_the_server_trusts() {
    let (trusted, other) = (new_key(), new_key());
    let port = start_server(TestServer { user_key: Some(trusted.public_key().clone()), ca: None }).await;
    let path = start_agent().await;
    #[cfg(windows)]
    let mut agent = russh::keys::agent::client::AgentClient::connect_named_pipe(&path).await.unwrap();
    #[cfg(unix)]
    let mut agent = russh::keys::agent::client::AgentClient::connect_uds(&path).await.unwrap();
    // The untrusted key first: it is tried, refused, and the next one used
    agent.add_identity(&other, &[]).await.unwrap();
    agent.add_identity(&trusted, &[]).await.unwrap();

    let listed = super::list_agent_keys().await.unwrap();
    assert_eq!(listed.identities.len(), 2);
    let mut handle = dial(port).await;
    let result = authenticate(&mut handle, &hop(AuthSpec::Agent)).await.map_err(|e| e.msg);
    assert_eq!(result, Ok(true));
}
