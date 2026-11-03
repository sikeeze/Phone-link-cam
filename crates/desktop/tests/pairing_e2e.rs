//! Uji end-to-end server HTTPS+WSS PhoneCam (fase 1).
//!
//! Memakai router produksi (phonecam_desktop::server::router) dengan
//! axum-server dari TcpListener, dan client WebSocket nyata
//! (tokio-tungstenite + rustls), tanpa browser:
//!
//! - HTTPS self-signed melayani `/` (frontend) dan `/health` (B1).
//! - WSS handshake, `hello` -> `auth` PIN benar -> `auth_ok` (B4).
//! - PIN salah 5x -> `auth_fail` LOCKED + retry_after 60 detik (B2).
//! - `ping` -> `pong` (B4 heartbeat).
//!
//! Verifikator TLS di sini menerima sertifikat self-signed yang dihasilkan
//! test ini sendiri (hanya untuk pengujian, tidak pernah di produksi).

#![forbid(unsafe_code)]

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, WebSocketStream};

use phonecam_desktop::AppState;

/// Verifikator yang menerima sertifikat apapun (HANYA untuk test).
#[derive(Debug)]
struct AcceptAnyCert;

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}

/// Client TLS config yang mempercayai sertifikat self-signed test ini.
fn tls_client_config() -> Arc<ClientConfig> {
    let _roots = RootCertStore::empty(); // ponytail: tidak dipakai; verifier kustom.
    Arc::new(
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCert))
            .with_no_client_auth(),
    )
}

/// Pesan signaling (B4): `{ v: 1, type, payload }`.
fn msg(t: &str, payload: &str) -> String {
    format!("{{\"v\":1,\"type\":\"{t}\",\"payload\":{payload}}}")
}

/// Bangun request WS dengan header Origin (B2: wajib).
fn ws_request(addr: &SocketAddr, origin: &str) -> Request {
    let uri: Uri = format!("wss://{addr}/ws").parse().expect("uri WS valid");
    Request::builder()
        .uri(uri)
        .header("Origin", origin)
        .header("Host", addr.to_string())
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .body(())
        .expect("request WS valid")
}

/// Hubungkan WS ke server uji dengan Origin yang diberikan.
async fn connect_ws(
    addr: &SocketAddr,
    origin: &str,
) -> WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (stream, _resp) = tokio_tungstenite::connect_async_tls_with_config(
        ws_request(addr, origin),
        None,
        false,
        Some(Connector::Rustls(tls_client_config())),
    )
    .await
    .expect("handshake WS berhasil");
    stream
}

/// Baca pesan teks JSON berikutnya; panic bila koneksi tertutup/timeouts.
async fn recv_json(
    ws: &mut WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout_at(deadline.into(), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                return serde_json::from_str(&t)
                    .unwrap_or_else(|_| panic!("balasan bukan JSON: {t}"));
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => panic!("error WS: {e}"),
            Ok(None) => panic!("koneksi tertutup sebelum balasan"),
            Err(_) => panic!("timeout menunggu balasan"),
        }
    }
}

/// GET HTTPS biasa; kembalikan (status, body).
async fn https_get(addr: &SocketAddr, path: &str) -> (u16, String) {
    let connector = tokio_rustls::TlsConnector::from(tls_client_config());
    let tcp = tokio::net::TcpStream::connect(addr)
        .await
        .expect("TCP connect");
    let domain = ServerName::try_from("127.0.0.1").expect("domain valid");
    let mut tls = connector.connect(domain, tcp).await.expect("TLS connect");
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    tls.write_all(req.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    tls.read_to_end(&mut buf).await.expect("read");
    let body = String::from_utf8_lossy(&buf).into_owned();
    let status = body
        .split("\r\n")
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    (status, body)
}

/// POST HTTPS dengan body JSON; kembalikan (status, body).
async fn https_post(addr: &SocketAddr, path: &str, body: &str) -> (u16, String) {
    let connector = tokio_rustls::TlsConnector::from(tls_client_config());
    let tcp = tokio::net::TcpStream::connect(addr)
        .await
        .expect("TCP connect");
    let domain = ServerName::try_from("127.0.0.1").expect("domain valid");
    let mut tls = connector.connect(domain, tcp).await.expect("TLS connect");
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let req = format!(
        "POST {path} HTTP/1.1
Host: {addr}
Content-Type: application/json
Content-Length: {}
Connection: close

{body}",
        body.len()
    );
    tls.write_all(req.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    tls.read_to_end(&mut buf).await.expect("read");
    let body = String::from_utf8_lossy(&buf).into_owned();
    let status = body
        .split("
")
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    (status, body)
}

// --- harness ---------------------------------------------------------------

/// Server uji: router produksi di 127.0.0.1:0 (port acak, loopback).
struct TestServer {
    addr: SocketAddr,
    state: Arc<AppState>,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

impl TestServer {
    async fn start() -> Self {
        let settings = phonecam_core::settings::Settings::default();
        let state = Arc::new(AppState::new(settings));

        // Bind 127.0.0.1:0: port acak, loopback (B1: antarmuka privat).
        // Catatan:axum_server::from_tcp_rustls di Windows accept-loop-nya macet;
        // bind_rustls + Handle::listening() untuk ambil port riil.
        let cert = phonecam_desktop::tls::generate(&[IpAddr::V4([127, 0, 0, 1].into())])
            .expect("generate cert");
        let cfg = axum_server::tls_rustls::RustlsConfig::from_pem(
            cert.cert_pem.as_bytes().to_vec(),
            cert.key_pem.as_bytes().to_vec(),
        )
        .await
        .expect("RustlsConfig");

        let handle = axum_server::Handle::new();
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().expect("addr valid");
        let server = axum_server::bind_rustls(bind_addr, cfg).handle(handle.clone());
        let app = phonecam_desktop::server::router().with_state(state.clone());
        tokio::spawn(server.serve(app.into_make_service_with_connect_info::<SocketAddr>()));

        // Ambil alamat riil setelah listening (port 0 -> port pilihan OS).
        let addr = tokio::time::timeout(Duration::from_secs(5), handle.listening())
            .await
            .expect("timeout menunggu listening")
            .expect("server gagal listening");

        // Matikan server saat test selesai (graceful).
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = rx.await;
            handle.shutdown();
        });

        Self {
            addr,
            state,
            _shutdown: tx,
        }
    }

    /// PIN aktif saat ini (B2).
    fn pin(&self) -> String {
        self.state.pairing.current().pin.as_str().to_string()
    }

    /// Buat PIN baru (B2: satu kali pakai; test butuh PIN segar per iterasi).
    fn regenerate_pin(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.state.pairing.regenerate(now).pin.as_str().to_string()
    }
}

// --- test ------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn https_serves_frontend_and_health() {
    let server = TestServer::start().await;

    let (status, body) = https_get(&server.addr, "/health").await;
    assert_eq!(status, 200);
    assert!(body.contains("\"status\":\"ok\""), "body: {body}");
    assert!(body.contains("pin_remaining_secs"), "body: {body}");

    let (status, body) = https_get(&server.addr, "/").await;
    assert_eq!(status, 200);
    assert!(body.contains("<html"), "index: {body}");
    // index.html tidak di-cache (B1: pembaruan langsung terlihat).
    assert!(
        body.contains("cache-control: no-cache"),
        "index.html harus no-cache: {body}"
    );

    // Aset ber-hash boleh di-cache kuat (B1). Nama file berubah tiap build
    // (hash Vite), jadi ambil nama css pertama yang tersedia saat ini.
    let css = std::fs::read_dir("../../frontend/dist/assets")
        .expect("dist assets harus ada (build frontend)")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|n| n.ends_with(".css"))
        .expect("minimal satu file css di dist/assets");
    let (status, body) = https_get(&server.addr, &format!("/assets/{css}")).await;
    assert_eq!(status, 200, "aset statis harus dilayani");
    assert!(
        body.contains("content-type: text/css"),
        "aset css harus punya MIME benar: {body}"
    );
    assert!(
        body.contains("cache-control: public, max-age=31536000, immutable"),
        "aset harus immutable: {body}"
    );
}

#[tokio::test]
async fn ws_rejects_foreign_origin() {
    let server = TestServer::start().await;
    // B2: Origin asing ditolak pada handshake WS.
    let err = tokio_tungstenite::connect_async_tls_with_config(
        ws_request(&server.addr, "https://evil.example.com"),
        None,
        false,
        Some(Connector::Rustls(tls_client_config())),
    )
    .await;
    assert!(err.is_err(), "Origin asing harus ditolak (B2)");
}

#[tokio::test]
async fn pairing_flow_hello_then_auth_ok() {
    let server = TestServer::start().await;
    let origin = format!("https://{}", server.addr);
    let mut ws = connect_ws(&server.addr, &origin).await;

    // B4: hello dulu, baru auth.
    ws.send(Message::Text(
        msg(
            "hello",
            "{\"device_id\":\"test\",\"name\":\"Pixel 8\",\"user_agent\":\"ua\",\"capabilities\":{},\"transport\":\"webrtc\"}",
        )
        .into(),
    ))
    .await
    .expect("kirim hello");

    let pin = server.pin();
    ws.send(Message::Text(
        msg("auth", &format!("{{\"credential\":{{\"pin\":\"{pin}\"}}}}")).into(),
    ))
    .await
    .expect("kirim auth");

    let reply = recv_json(&mut ws).await;
    assert_eq!(reply["type"], "auth_ok", "balasan: {reply}");
    assert!(
        reply["payload"]["token"].is_string(),
        "token reconnect hilang: {reply}"
    );
    assert!(
        reply["payload"]["source_id"].is_string(),
        "source_id hilang: {reply}"
    );
    assert_eq!(reply["payload"]["quality"], "balanced");
}

#[tokio::test]
async fn five_wrong_pins_then_locked() {
    let server = TestServer::start().await;
    let origin = format!("https://{}", server.addr);

    // B2: 5 percobaan salah dari IP yang sama -> kunci 60 detik.
    for i in 0..5u32 {
        let mut ws = connect_ws(&server.addr, &origin).await;
        ws.send(Message::Text(
            msg(
                "auth",
                &format!("{{\"credential\":{{\"pin\":\"{i:06}\"}}}}"),
            )
            .into(),
        ))
        .await
        .expect("kirim auth");
        let reply = recv_json(&mut ws).await;
        assert_eq!(reply["type"], "auth_fail", "iterasi {i}: {reply}");
        if i < 4 {
            assert_eq!(reply["payload"]["code"], "BAD_PIN", "iterasi {i}: {reply}");
        } else {
            assert_eq!(reply["payload"]["code"], "LOCKED", "iterasi {i}: {reply}");
            assert_eq!(retry_after_secs(&reply), 60, "retry_after: {reply}");
        }
        let _ = ws.close(None).await;
    }

    // Percobaan ke-6 dengan PIN benar tetap dikunci (B2).
    let mut ws = connect_ws(&server.addr, &origin).await;
    let pin = server.pin();
    ws.send(Message::Text(
        msg("auth", &format!("{{\"credential\":{{\"pin\":\"{pin}\"}}}}")).into(),
    ))
    .await
    .expect("kirim auth");
    let reply = recv_json(&mut ws).await;
    assert_eq!(
        reply["payload"]["code"], "LOCKED",
        "harus tetap dikunci: {reply}"
    );
}

#[tokio::test]
async fn ping_gets_pong() {
    let server = TestServer::start().await;
    let origin = format!("https://{}", server.addr);
    let mut ws = connect_ws(&server.addr, &origin).await;

    ws.send(Message::Text(msg("ping", "{}").into()))
        .await
        .expect("kirim ping");
    let reply = recv_json(&mut ws).await;
    assert_eq!(
        reply["type"], "pong",
        "ping harus dibalas pong (B4): {reply}"
    );
}

/// `retry_after` boleh angka atau string (serde flexible); ambil sebagai u32.
fn retry_after_secs(reply: &serde_json::Value) -> u32 {
    let v = &reply["payload"]["retry_after"];
    v.as_u64().unwrap_or(0) as u32
}

#[tokio::test]
async fn source_slot_freed_after_disconnect() {
    // B2/B3 regression: slot ke-6 tidak bocor. Sumber hanya dihapus setelah
    // `bye` eksplisit atau anggaran reconnect 60 detik habis; di sini kita
    // pakai `bye` (B4) agar slot langsung dibebaskan.
    let server = TestServer::start().await;
    let origin = format!("https://{}", server.addr);

    for _ in 0..7 {
        let mut ws = connect_ws(&server.addr, &origin).await;
        ws.send(Message::Text(
            msg(
                "hello",
                "{\"device_id\":\"test\",\"name\":\"Pixel 8\",\"user_agent\":\"ua\",\"capabilities\":{},\"transport\":\"webrtc\"}",
            )
            .into(),
        ))
        .await
        .expect("kirim hello");
        // B2: PIN satu kali pakai — buat PIN segar tiap iterasi.
        let pin = server.regenerate_pin();
        ws.send(Message::Text(
            msg("auth", &format!("{{\"credential\":{{\"pin\":\"{pin}\"}}}}")).into(),
        ))
        .await
        .expect("kirim auth");
        let reply = recv_json(&mut ws).await;
        assert_eq!(reply["type"], "auth_ok", "balasan: {reply}");
        // B4: `bye` eksplisit → source dihapus (B3: CLOSED langsung).
        ws.send(Message::Text(msg("bye", "{}").into()))
            .await
            .expect("kirim bye");
        let _ = ws.close(None).await;
        // Beri waktu server memproses close.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Setelah 7 pasang-sambung, registry harusnya kosong (max 6, B2).
    let sources = server.state.sources.read().list().len();
    assert_eq!(
        sources, 0,
        "registry bocor: {sources} sumber masih terdaftar"
    );
}

#[tokio::test]
async fn token_reconnect_restores_source() {
    // B2/B3: WS putus tanpa `bye` → RECONNECTING; HP reconnect dengan token
    // dan sumber yang sama kembali LIVE (source_id sama).
    let server = TestServer::start().await;
    let origin = format!("https://{}", server.addr);

    let mut ws = connect_ws(&server.addr, &origin).await;
    ws.send(Message::Text(
        msg(
            "hello",
            "{\"device_id\":\"dev1\",\"name\":\"Pixel 8\",\"user_agent\":\"ua\",\"capabilities\":{},\"transport\":\"webrtc\"}",
        )
        .into(),
    ))
    .await
    .expect("kirim hello");
    let pin = server.pin();
    ws.send(Message::Text(
        msg("auth", &format!("{{\"credential\":{{\"pin\":\"{pin}\"}}}}")).into(),
    ))
    .await
    .expect("kirim auth");
    let reply = recv_json(&mut ws).await;
    assert_eq!(reply["type"], "auth_ok");
    let token = reply["payload"]["token"]
        .as_str()
        .expect("token")
        .to_string();
    let source_id = reply["payload"]["source_id"]
        .as_str()
        .expect("source_id")
        .to_string();

    // WS putus tiba-tiba (tidak ada `bye`) → RECONNECTING.
    let _ = ws.close(None).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    {
        let sources = server.state.sources.read();
        let s = sources.get(&source_id).expect("sumber harus tetap ada");
        assert_eq!(
            s.status.state.0,
            phonecam_core::session::SessionState::Reconnecting,
            "state harus RECONNECTING setelah putus tanpa bye"
        );
    }

    // HP reconnect dengan token (B2: token berlaku selama sesi desktop hidup).
    let mut ws2 = connect_ws(&server.addr, &origin).await;
    ws2.send(Message::Text(
        msg(
            "hello",
            "{\"device_id\":\"dev1\",\"name\":\"Pixel 8\",\"user_agent\":\"ua\",\"capabilities\":{},\"transport\":\"webrtc\"}",
        )
        .into(),
    ))
    .await
    .expect("kirim hello");
    ws2.send(Message::Text(
        msg(
            "auth",
            &format!("{{\"credential\":{{\"token\":\"{token}\"}}}}"),
        )
        .into(),
    ))
    .await
    .expect("kirim auth token");
    let reply = recv_json(&mut ws2).await;
    assert_eq!(reply["type"], "auth_ok", "reconnect via token: {reply}");
    assert_eq!(
        reply["payload"]["source_id"].as_str().expect("source_id"),
        source_id,
        "source_id harus sama setelah reconnect"
    );

    // State kembali LIVE (B3).
    {
        let sources = server.state.sources.read();
        let s = sources.get(&source_id).expect("sumber harus tetap ada");
        assert_eq!(
            s.status.state.0,
            phonecam_core::session::SessionState::Live,
            "state harus LIVE setelah reconnect"
        );
    }
    // Hanya 1 sumber di registry (tidak menduplikat).
    assert_eq!(server.state.sources.read().list().len(), 1);
}

#[tokio::test]
async fn token_unknown_rejected_as_bad_pin() {
    // B2: token tidak dikenal → auth_fail BAD_PIN (mencegah tebakan token).
    let server = TestServer::start().await;
    let origin = format!("https://{}", server.addr);
    let mut ws = connect_ws(&server.addr, &origin).await;
    ws.send(Message::Text(
        msg(
            "hello",
            "{\"device_id\":\"d\",\"name\":\"d\",\"user_agent\":\"u\",\"capabilities\":{},\"transport\":\"webrtc\"}",
        )
        .into(),
    ))
    .await
    .expect("kirim hello");
    ws.send(Message::Text(
        msg(
            "auth",
            "{\"credential\":{\"token\":\"deadbeefdeadbeefdeadbeefdeadbeef\"}}",
        )
        .into(),
    ))
    .await
    .expect("kirim auth token");
    let reply = recv_json(&mut ws).await;
    assert_eq!(reply["type"], "auth_fail", "balasan: {reply}");
    assert_eq!(reply["payload"]["code"], "BAD_PIN");
}

#[tokio::test]
async fn qr_endpoint_serves_svg_with_pairing_url() {
    // QR harus memuat URL pairing lengang (B2): skema HTTPS + IP + PIN.
    let server = TestServer::start().await;
    let (status, body) = https_get(&server.addr, "/qr").await;
    assert_eq!(status, 200, "QR harus dilayani: {body}");
    assert!(
        body.contains("image/svg+xml"),
        "content-type harus SVG: {body}"
    );
    assert!(body.contains("<svg"), "body harus SVG: {body}");
    assert!(!body.trim().is_empty(), "SVG tidak boleh kosong");

    // Bandingkan dengan QR referensi untuk URL yang sama.
    let url = format!("https://{}/?pin={}", server.addr, server.pin());
    let ref_code = qrcode::QrCode::new(url.as_bytes()).expect("QR referensi");
    let expected_w = (ref_code.width() + 8) * 8;
    let w = body
        .split("width=\"")
        .nth(1)
        .and_then(|s| s.split(' ').next())
        .and_then(|s| s.trim_end_matches('"').parse::<usize>().ok());
    assert_eq!(
        w,
        Some(expected_w),
        "lebar SVG harus {}: {body}",
        expected_w
    );
}

#[tokio::test]
async fn api_source_active_demotes_and_404() {
    // B6: endpoint Jadikan aktif menurunkan sumber lain; 404 bila tak dikenal.
    let server = TestServer::start().await;
    {
        let mut sources = server.state.sources.write();
        for id in ["s1", "s2"] {
            sources
                .insert(phonecam_core::source::Source::new(
                    id.into(),
                    phonecam_core::source::SourceInfo {
                        device_id: "dev".into(),
                        name: "HP".into(),
                        user_agent: "UA".into(),
                        transport: phonecam_core::protocol::Transport::WebRtc,
                        connection_kind: phonecam_core::source::ConnectionKind::Wifi,
                    },
                    "tok".into(),
                ))
                .expect("insert sumber");
        }
    }
    {
        let sources = server.state.sources.read();
        assert_eq!(
            sources.active().map(|s| s.id.as_str()),
            Some("s1"),
            "sumber pertama harus aktif"
        );
    }
    let (status, _body) = https_post(&server.addr, "/api/source/s2/active", "").await;
    assert_eq!(status, 204, "jadikan aktif harus 204");
    {
        let sources = server.state.sources.read();
        assert_eq!(sources.active().map(|s| s.id.as_str()), Some("s2"));
        assert_eq!(
            sources.get("s1").map(|s| s.role),
            Some(phonecam_core::protocol::SourceRole::Preview)
        );
    }
    // B6: rename perangkat.
    let (status, _body) = https_post(
        &server.addr,
        "/api/source/s2/rename",
        "{\"name\": \"HP Belakang\"}",
    )
    .await;
    assert_eq!(status, 204, "rename harus 204");
    {
        let sources = server.state.sources.read();
        assert_eq!(
            sources.get("s2").map(|s| s.info.name.as_str()),
            Some("HP Belakang")
        );
    }
    // Sumber tak dikenal -> 404.
    let (status, _body) = https_post(&server.addr, "/api/source/tidak-ada/active", "").await;
    assert_eq!(status, 404);
}
