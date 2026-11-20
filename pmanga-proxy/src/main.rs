use axum_server::tls_rustls::RustlsConfig;
use rcgen::generate_simple_self_signed;
use std::{net::SocketAddr, path::Path as FsPath};

#[tokio::main]
async fn main() {
    let app = pmanga_proxy_core::router();

    // Load or generate a self-signed certificate so the proxy can be reached
    // over HTTPS from GitHub Pages without mixed-content blocks.
    //
    // The cert is persisted to disk so it stays the same across restarts —
    // this means Firefox's one-time "Accept the Risk" exception stays valid
    // and doesn't need to be re-accepted every time the proxy is restarted.
    let cert_path = FsPath::new("cert.pem");
    let key_path = FsPath::new("key.pem");

    let (cert_pem, key_pem) = if cert_path.exists() && key_path.exists() {
        eprintln!("pmanga-proxy: loading existing TLS cert from cert.pem / key.pem");
        let cert = std::fs::read_to_string(cert_path).expect("failed to read cert.pem");
        let key = std::fs::read_to_string(key_path).expect("failed to read key.pem");
        (cert, key)
    } else {
        eprintln!(
            "pmanga-proxy: generating new self-signed TLS cert (saved to cert.pem / key.pem)"
        );
        let subject_alt_names = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "192.168.1.79".to_string(),
        ];
        let cert = generate_simple_self_signed(subject_alt_names)
            .expect("failed to generate self-signed certificate");
        let cert_pem = cert.cert.pem();
        let key_pem = cert.signing_key.serialize_pem();
        std::fs::write(cert_path, &cert_pem).expect("failed to write cert.pem");
        std::fs::write(key_path, &key_pem).expect("failed to write key.pem");
        (cert_pem, key_pem)
    };

    let tls_config = RustlsConfig::from_pem(cert_pem.into_bytes(), key_pem.into_bytes())
        .await
        .expect("failed to build TLS config");

    let addr = "0.0.0.0:7331".parse::<SocketAddr>().unwrap();
    eprintln!("pmanga-proxy listening on https://{}", addr);
    eprintln!("First time? Visit https://192.168.1.79:7331 in Firefox and accept the certificate.");
    eprintln!("The cert is saved to disk — you only need to do this once.");

    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
