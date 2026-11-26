// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, Endpoint, ServerConfig, TransportConfig};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::RootCertStore;

pub const ASH_ALPN: &[u8] = b"ash-quic-v1";

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Vollständiges lokales PKI-Bundle (Root-CA + mTLS-Zertifikate für Server & Mesh-Clients).
#[derive(Debug, Clone)]
pub struct GeneratedPki {
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
    pub node_cert_pem: String,
    pub node_key_pem: String,
}

/// Erzeugt die lokale Ash-Net Root-CA sowie universelle SAN-Zertifikate für `auth-gateway`,
/// `irc-bridge`, `game-node-roadhouse`, `game-node-frontier`, `game-node-raid` und `localhost`.
pub fn generate_ash_pki() -> Result<GeneratedPki> {
    ensure_crypto_provider();

    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CommonName, "Ash-Net Enochian Root CA");
    ca_dn.push(DnType::OrganizationName, "Men of Letters Andromeda Relay");
    ca_params.distinguished_name = ca_dn;

    let ca_key = KeyPair::generate().context("generate Root CA keypair")?;
    let ca_cert = ca_params
        .self_signed(&ca_key)
        .context("self-sign Root CA")?;

    let mut leaf_params = CertificateParams::default();
    leaf_params.is_ca = IsCa::NoCa;
    leaf_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    leaf_params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];
    let mut leaf_dn = DistinguishedName::new();
    leaf_dn.push(DnType::CommonName, "ash-node");
    leaf_params.distinguished_name = leaf_dn;
    leaf_params.subject_alt_names = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::DnsName("ash-node".try_into()?),
        SanType::DnsName("auth-gateway".try_into()?),
        SanType::DnsName("irc-bridge".try_into()?),
        SanType::DnsName("game-node-roadhouse".try_into()?),
        SanType::DnsName("game-node-frontier".try_into()?),
        SanType::DnsName("game-node-raid".try_into()?),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
    ];

    let leaf_key = KeyPair::generate().context("generate leaf keypair")?;
    let leaf_cert = leaf_params
        .signed_by(&leaf_key, &ca_cert, &ca_key)
        .context("sign leaf cert with Root CA")?;

    Ok(GeneratedPki {
        ca_cert_pem: ca_cert.pem(),
        ca_key_pem: ca_key.serialize_pem(),
        node_cert_pem: leaf_cert.pem(),
        node_key_pem: leaf_key.serialize_pem(),
    })
}

/// Schreibt `ca.crt`, `ca.key` sowie Zertifikate für alle Dienste in ein Zielverzeichnis (`pki-init`).
pub fn write_pki_directory(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).context("create PKI directory")?;
    let ca_path = dir.join("ca.crt");
    let node_crt = dir.join("node.crt");
    if ca_path.exists() && node_crt.exists() {
        return Ok(());
    }
    let pki = generate_ash_pki()?;
    fs::write(&ca_path, &pki.ca_cert_pem)?;
    fs::write(dir.join("ca.key"), &pki.ca_key_pem)?;

    for service in [
        "node",
        "auth-gateway",
        "irc-bridge",
        "game-node-roadhouse",
        "game-node-frontier",
        "game-node-raid",
    ] {
        fs::write(dir.join(format!("{service}.crt")), &pki.node_cert_pem)?;
        fs::write(dir.join(format!("{service}.key")), &pki.node_key_pem)?;
    }
    Ok(())
}

fn load_certs_from_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse PEM certificates")
}

fn load_private_key_from_pem(pem: &[u8]) -> Result<PrivateKeyDer<'static>> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::private_key(&mut reader)
        .context("parse PEM private key")?
        .context("no private key found in PEM")
}

fn default_transport_config() -> Arc<TransportConfig> {
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(4)));
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into().unwrap()));
    transport.datagram_receive_buffer_size(Some(512 * 1024));
    transport.datagram_send_buffer_size(512 * 1024);
    Arc::new(transport)
}

/// Erstellt einen QUIC-Server-Endpoint. Wenn `mtls_ca_pem` gesetzt ist, wird zwingend mTLS
/// (Client-Zertifikatsprüfung gegen die Ash-Net Root-CA) für das Server-Mesh erzwungen.
pub fn build_quic_server(
    bind_addr: SocketAddr,
    cert_pem: &[u8],
    key_pem: &[u8],
    mtls_ca_pem: Option<&[u8]>,
) -> Result<Endpoint> {
    ensure_crypto_provider();
    let certs = load_certs_from_pem(cert_pem)?;
    let key = load_private_key_from_pem(key_pem)?;

    let mut tls_config = if let Some(ca_bytes) = mtls_ca_pem {
        let mut roots = RootCertStore::empty();
        for cert in load_certs_from_pem(ca_bytes)? {
            roots.add(cert).context("add CA cert to root store")?;
        }
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .context("build mTLS client verifier")?;
        rustls::ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .context("configure mTLS rustls server")?
    } else {
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .context("configure TLS 1.3 rustls server")?
    };

    tls_config.alpn_protocols = vec![ASH_ALPN.to_vec()];
    let quic_tls = QuicServerConfig::try_from(tls_config).context("build QuicServerConfig")?;
    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_tls));
    server_config.transport_config(default_transport_config());

    Endpoint::server(server_config, bind_addr).context("bind QUIC server endpoint")
}

/// Erstellt einen QUIC-Client-Endpoint (optional mit Client-Zertifikat für mTLS im Server-Mesh).
pub fn build_quic_client(
    bind_addr: SocketAddr,
    ca_pem: &[u8],
    mtls_identity: Option<(&[u8], &[u8])>,
) -> Result<Endpoint> {
    ensure_crypto_provider();
    let mut roots = RootCertStore::empty();
    for cert in load_certs_from_pem(ca_pem)? {
        roots.add(cert).context("add CA cert to client root store")?;
    }

    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
    let mut tls_config = if let Some((cert_pem, key_pem)) = mtls_identity {
        let certs = load_certs_from_pem(cert_pem)?;
        let key = load_private_key_from_pem(key_pem)?;
        builder
            .with_client_auth_cert(certs, key)
            .context("configure mTLS client identity")?
    } else {
        builder.with_no_client_auth()
    };

    tls_config.alpn_protocols = vec![ASH_ALPN.to_vec()];
    let quic_tls = QuicClientConfig::try_from(tls_config).context("build QuicClientConfig")?;
    let mut client_config = ClientConfig::new(Arc::new(quic_tls));
    client_config.transport_config(default_transport_config());

    let mut endpoint = Endpoint::client(bind_addr).context("bind QUIC client endpoint")?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}
