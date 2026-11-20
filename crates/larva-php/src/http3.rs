//! HTTP/3 (QUIC) server support via [cloudflare/quiche](https://github.com/cloudflare/quiche).
//!
//! This module is compiled only with the `http3` feature. It runs a QUIC/HTTP3
//! listener on a UDP socket, reusing the same request-handling core as the TCP
//! (HTTP/1.1 + HTTP/2) server via [`Server::handle_request_parts`].
//!
//! Design: the TCP server (hyper) and the UDP/QUIC server are two "transports"
//! that both delegate to the same core handler (a Strategy/Adapter split).
//! Protocol selection is automatic:
//!   - Browser with HTTP/3 support  -> QUIC/UDP (ALPN "h3")
//!   - Browser without HTTP/3        -> TCP HTTP/2 (ALPN "h2") or HTTP/1.1
//!
//! The TCP server advertises HTTP/3 via an `Alt-Svc` header so capable browsers
//! upgrade to QUIC while others transparently keep using TCP.

#[cfg(feature = "http3")]
pub mod imp {
    use std::collections::HashMap;
    use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
    use std::sync::Arc;
    use std::time::Duration;

    use quiche::h3;
    use quiche::h3::NameValue;

    use crate::errors::{Error, Result};
    use crate::server::Server;

    /// QUIC/UDP datagram buffer size.
    const MAX_DATAGRAM_SIZE: usize = 1350;

    /// A response that could not be fully written in one pass (flow control).
    struct PartialResponse {
        headers: Option<Vec<h3::Header>>,
        body: Vec<u8>,
        written: usize,
    }

    /// Per-connection state.
    struct Client {
        conn: quiche::Connection,
        http3_conn: Option<h3::Connection>,
        partial_responses: HashMap<u64, PartialResponse>,
    }

    /// HTTP/3 (QUIC) server.
    ///
    /// Holds the shared [`Server`] and delegates every HTTP/3 request to
    /// [`Server::handle_request_parts`], so behavior is identical to HTTP/1.1
    /// and HTTP/2.
    pub struct Http3Server {
        addr: String,
        cert_path: String,
        key_path: String,
        server: Arc<Server>,
    }

    impl Http3Server {
        pub fn new(
            addr: impl Into<String>,
            cert_path: impl Into<String>,
            key_path: impl Into<String>,
            server: Arc<Server>,
        ) -> Self {
            Self {
                addr: addr.into(),
                cert_path: cert_path.into(),
                key_path: key_path.into(),
                server,
            }
        }

        /// Run the HTTP/3 server (blocking). Intended to run on a dedicated
        /// thread. Returns only on a fatal socket error.
        pub fn run(&self) -> Result<()> {
            let local: SocketAddr = self
                .addr
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| Error::Ffi("invalid http3 address".to_string()))?;

            let socket = UdpSocket::bind(local)?;
            socket.set_read_timeout(Some(Duration::from_millis(100)))?;

            let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION)
                .map_err(|e| Error::Ffi(e.to_string()))?;
            config
                .load_cert_chain_from_pem_file(&self.cert_path)
                .map_err(|e| Error::Ffi(format!("load cert chain: {}", e)))?;
            config
                .load_priv_key_from_pem_file(&self.key_path)
                .map_err(|e| Error::Ffi(format!("load private key: {}", e)))?;
            config
                .set_application_protos(h3::APPLICATION_PROTOCOL)
                .map_err(|e| Error::Ffi(e.to_string()))?;
            config.set_max_idle_timeout(30_000);
            config.set_max_recv_udp_payload_size(MAX_DATAGRAM_SIZE);
            config.set_max_send_udp_payload_size(MAX_DATAGRAM_SIZE);
            config.set_initial_max_data(10_000_000);
            config.set_initial_max_stream_data_bidi_local(1_000_000);
            config.set_initial_max_stream_data_bidi_remote(1_000_000);
            config.set_initial_max_stream_data_uni(1_000_000);
            config.set_initial_max_streams_bidi(100);
            config.set_initial_max_streams_uni(100);
            config.set_disable_active_migration(true);

            let h3_config =
                h3::Config::new().map_err(|e| Error::Ffi(format!("h3 config: {}", e)))?;

            let mut clients: HashMap<Vec<u8>, Client> = HashMap::new();
            let mut buf = [0u8; 65535];
            let mut out = [0u8; MAX_DATAGRAM_SIZE];

            loop {
                // Read one datagram (or time out to drive QUIC timers).
                let (len, from) = match socket.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        for c in clients.values_mut() {
                            c.conn.on_timeout();
                        }
                        send_pending(&socket, &mut clients, &mut out);
                        continue;
                    }
                    Err(e) => return Err(Error::Io(e)),
                };

                let hdr =
                    match quiche::Header::from_slice(&mut buf[..len], quiche::MAX_CONN_ID_LEN) {
                        Ok(h) => h,
                        Err(_) => continue,
                    };

                // Version negotiation for unsupported QUIC versions.
                if !quiche::version_is_supported(hdr.version) {
                    if let Ok(n) = quiche::negotiate_version(&hdr.scid, &hdr.dcid, &mut out) {
                        let _ = socket.send_to(&out[..n], from);
                    }
                    continue;
                }

                let key = hdr.dcid.to_vec();

                // Find or create the connection.
                if !clients.contains_key(&key) {
                    if hdr.ty != quiche::Type::Initial {
                        continue;
                    }
                    let scid = hdr.dcid.clone();
                    let conn = quiche::accept(&scid, None, local, from, &mut config)
                        .map_err(|e| Error::Ffi(format!("quiche accept: {}", e)))?;
                    clients.insert(
                        scid.to_vec(),
                        Client {
                            conn,
                            http3_conn: None,
                            partial_responses: HashMap::new(),
                        },
                    );
                }

                let client = clients.get_mut(&key).unwrap();

                let recv_info = quiche::RecvInfo { to: local, from };
                if client.conn.recv(&mut buf[..len], recv_info).is_err() {
                    continue;
                }

                // Establish the HTTP/3 layer once QUIC is up.
                if (client.conn.is_established() || client.conn.is_in_early_data())
                    && client.http3_conn.is_none()
                {
                    if let Ok(h3c) = h3::Connection::with_transport(&mut client.conn, &h3_config) {
                        client.http3_conn = Some(h3c);
                    }
                }

                // Flush any flow-control-blocked responses.
                if client.http3_conn.is_some() {
                    for stream_id in client.conn.writable() {
                        handle_writable(client, stream_id);
                    }
                }

                // Process HTTP/3 events.
                if let Some(h3c) = client.http3_conn.as_mut() {
                    loop {
                        match h3c.poll(&mut client.conn) {
                            Ok((stream_id, h3::Event::Headers { list, .. })) => {
                                handle_request(
                                    &self.server,
                                    &mut client.conn,
                                    h3c,
                                    stream_id,
                                    &list,
                                    &mut client.partial_responses,
                                );
                            }
                            Ok((_, h3::Event::Data))
                            | Ok((_, h3::Event::Finished))
                            | Ok((_, h3::Event::Reset { .. }))
                            | Ok((_, h3::Event::PriorityUpdate))
                            | Ok((_, h3::Event::GoAway)) => {}
                            Err(h3::Error::Done) => break,
                            Err(_) => break,
                        }
                    }
                }

                send_pending(&socket, &mut clients, &mut out);
            }
        }
    }

    /// Flush pending QUIC packets for all connections and garbage-collect closed ones.
    fn send_pending(
        socket: &UdpSocket,
        clients: &mut HashMap<Vec<u8>, Client>,
        out: &mut [u8],
    ) {
        for client in clients.values_mut() {
            loop {
                match client.conn.send(out) {
                    Ok((write, send_info)) => {
                        if socket.send_to(&out[..write], send_info.to).is_err() {
                            break;
                        }
                    }
                    Err(quiche::Error::Done) => break,
                    Err(_) => {
                        client.conn.close(false, 0x1, b"fail").ok();
                        break;
                    }
                }
            }
        }
        clients.retain(|_, c| !c.conn.is_closed());
    }

    /// Handle an incoming HTTP/3 request by delegating to the shared core.
    fn handle_request(
        server: &Arc<Server>,
        conn: &mut quiche::Connection,
        http3_conn: &mut h3::Connection,
        stream_id: u64,
        headers: &[h3::Header],
        partial_responses: &mut HashMap<u64, PartialResponse>,
    ) {
        // Stop reading the request stream; we decide the response from headers.
        let _ = conn.stream_shutdown(stream_id, quiche::Shutdown::Read, 0);

        let mut method = String::from("GET");
        let mut path = String::from("/");
        let mut req_headers: Vec<(String, String)> = Vec::new();
        for h in headers {
            match h.name() {
                b":method" => method = String::from_utf8_lossy(h.value()).to_string(),
                b":path" => path = String::from_utf8_lossy(h.value()).to_string(),
                b":authority" | b":scheme" | b":status" => {}
                name => {
                    req_headers.push((
                        String::from_utf8_lossy(name).to_string(),
                        String::from_utf8_lossy(h.value()).to_string(),
                    ));
                }
            }
        }

        // Delegate to the shared request handler.
        let (status, resp_headers, body) =
            match server.handle_request_parts(&method, &path, &req_headers, &[]) {
                Ok(v) => v,
                Err(_) => (
                    hyper::StatusCode::INTERNAL_SERVER_ERROR,
                    Vec::new(),
                    b"Internal Server Error".to_vec(),
                ),
            };

        // Build HTTP/3 response headers.
        let mut out_headers = vec![h3::Header::new(
            b":status",
            status.as_u16().to_string().as_bytes(),
        )];
        for (name, value) in &resp_headers {
            if name.eq_ignore_ascii_case("content-length")
                || name.eq_ignore_ascii_case("connection")
                || name.eq_ignore_ascii_case("transfer-encoding")
            {
                continue;
            }
            out_headers.push(h3::Header::new(name.as_bytes(), value.as_bytes()));
        }
        out_headers.push(h3::Header::new(
            b"content-length",
            body.len().to_string().as_bytes(),
        ));

        // Send headers, handling flow control.
        match http3_conn.send_response(conn, stream_id, &out_headers, false) {
            Ok(_) => {}
            Err(h3::Error::StreamBlocked) => {
                partial_responses.insert(
                    stream_id,
                    PartialResponse {
                        headers: Some(out_headers),
                        body,
                        written: 0,
                    },
                );
                return;
            }
            Err(_) => return,
        }

        let written = match http3_conn.send_body(conn, stream_id, &body, true) {
            Ok(v) => v,
            Err(h3::Error::Done) => 0,
            Err(_) => return,
        };

        if written < body.len() {
            partial_responses.insert(
                stream_id,
                PartialResponse {
                    headers: None,
                    body,
                    written,
                },
            );
        }
    }

    /// Continue writing a flow-control-blocked response on a writable stream.
    fn handle_writable(client: &mut Client, stream_id: u64) {
        if !client.partial_responses.contains_key(&stream_id) {
            return;
        }

        let conn = &mut client.conn;
        let http3_conn = client.http3_conn.as_mut().unwrap();
        let resp = client.partial_responses.get_mut(&stream_id).unwrap();

        if let Some(headers) = resp.headers.take() {
            if http3_conn.send_response(conn, stream_id, &headers, false).is_err() {
                resp.headers = Some(headers);
                return;
            }
        }

        let body = &resp.body[resp.written..];
        match http3_conn.send_body(conn, stream_id, body, true) {
            Ok(written) => resp.written += written,
            Err(h3::Error::Done) => {}
            Err(_) => {
                client.partial_responses.remove(&stream_id);
                return;
            }
        }

        if resp.written == resp.body.len() {
            client.partial_responses.remove(&stream_id);
        }
    }

    /// Generate a self-signed certificate/key pair if the given paths do not exist.
    /// Returns `(cert_path, key_path)` ready for quiche.
    pub fn ensure_cert(cert_path: &str, key_path: &str) -> Result<(String, String)> {
        use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};

        if std::path::Path::new(cert_path).exists() && std::path::Path::new(key_path).exists() {
            return Ok((cert_path.to_string(), key_path.to_string()));
        }

        let mut params = CertificateParams::new(vec!["localhost".to_string()])
            .map_err(|e| Error::Ffi(format!("cert params: {}", e)))?;
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, "localhost");

        let key_pair = KeyPair::generate().map_err(|e| Error::Ffi(format!("keygen: {}", e)))?;
        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| Error::Ffi(format!("self_signed: {}", e)))?;

        std::fs::write(cert_path, cert.pem())
            .map_err(|e| Error::Ffi(format!("write cert: {}", e)))?;
        std::fs::write(key_path, key_pair.serialize_pem())
            .map_err(|e| Error::Ffi(format!("write key: {}", e)))?;

        Ok((cert_path.to_string(), key_path.to_string()))
    }
}

#[cfg(feature = "http3")]
pub use imp::*;

#[cfg(not(feature = "http3"))]
pub mod imp {
    use std::sync::Arc;
    use crate::server::Server;

    /// Stub HTTP/3 server used when the `http3` feature is disabled.
    pub struct Http3Server;

    impl Http3Server {
        pub fn new(
            _addr: impl Into<String>,
            _cert_path: impl Into<String>,
            _key_path: impl Into<String>,
            _server: Arc<Server>,
        ) -> Self {
            Http3Server
        }

        pub fn run(&self) -> crate::errors::Result<()> {
            Err(crate::errors::Error::Ffi(
                "HTTP/3 support is not enabled (build with --features http3)".to_string(),
            ))
        }
    }

    /// Stub cert helper (no-op when disabled).
    pub fn ensure_cert(cert_path: &str, key_path: &str) -> crate::errors::Result<(String, String)> {
        Ok((cert_path.to_string(), key_path.to_string()))
    }
}

#[cfg(not(feature = "http3"))]
pub use imp::*;