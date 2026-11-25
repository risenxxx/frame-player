//! Google Cast, discovery only: version 1's `cast/discovery.rs` compiled as it is (mDNS browse plus the
//! unicast sweep of `lan_sweep.rs`). Connecting, the LAN file server and the session are not ported:
//! `native:cast` answers the picker's list, and the stub refuses `cast_connect` with a message.
#![allow(dead_code)]

#[path = "../../../src-tauri/src/cast/discovery.rs"]
pub mod discovery;

/// What `discovery.rs` takes from `castv2.rs`: the TLS connection to a device's 8009, which the probe
/// opens to read the device's name. Version 1's own function, with the certificate check it needs
/// (a Cast device's certificate is self-signed and not for PKI).
pub(super) mod castv2 {
    use std::net::IpAddr;
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::net::TcpStream;
    use tokio_rustls::rustls;

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

    #[derive(Debug)]
    struct AcceptDeviceCert(rustls::crypto::CryptoProvider);

    impl rustls::client::danger::ServerCertVerifier for AcceptDeviceCert {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }

    pub(in crate::cast) async fn tls_connect(ip: IpAddr, port: u16) -> Result<tokio_rustls::client::TlsStream<TcpStream>, String> {
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((ip, port)))
            .await
            .map_err(|_| "unreachable".to_string())?
            .map_err(|e| format!("unreachable: {e}"))?;
        let provider = rustls::crypto::ring::default_provider();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(provider.clone()))
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("tls: {e}"))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptDeviceCert(provider)))
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        tokio::time::timeout(CONNECT_TIMEOUT, connector.connect(rustls::pki_types::ServerName::IpAddress(ip.into()), tcp))
            .await
            .map_err(|_| "unreachable".to_string())?
            .map_err(|e| format!("unreachable: {e}"))
    }
}


pub use discovery::CastDeviceInfo;

// ---- Version 1's `cast_discover_start`, `cast_discover_stop` and `cast_devices` (cast/mod.rs).

static DISCOVERY: std::sync::Mutex<Option<discovery::Discovery>> = std::sync::Mutex::new(None);

pub fn discover_start(hints: Vec<String>) -> Result<(), String> {
    let mut d = DISCOVERY.lock().unwrap_or_else(|p| p.into_inner());
    if d.is_none() {
        *d = Some(discovery::Discovery::start(crate::lan_sweep::parse_hints(&hints))?);
    }
    Ok(())
}

pub fn discover_stop() {
    let d = DISCOVERY.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(d) = d {
        d.stop();
    }
}

pub fn devices() -> Vec<CastDeviceInfo> {
    DISCOVERY.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(discovery::Discovery::list).unwrap_or_default()
}
