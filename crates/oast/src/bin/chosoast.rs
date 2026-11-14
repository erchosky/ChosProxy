//! `chosoast`: a self-hosted ChosOAST interaction server.
//!
//! Run it on a machine you control, delegate a DNS zone to it (NS record)
//! and point ChosProxy's "Self-hosted" OAST mode at its API with the token
//! and certificate fingerprint it prints. See docs/OAST.md.

#![allow(clippy::print_stdout, clippy::print_stderr)] // a command-line tool

use chosproxy_oast::cert::SelfSigned;
use chosproxy_oast::dns::ZoneAddresses;
use chosproxy_oast::server::{OastServer, ServerConfig};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: chosoast --zone <oast.example.com> --ip <public IPv4> [options]

options:
  --ipv6 <addr>        AAAA answer for names in the zone
  --dns <addr:port>    DNS listener          (default 0.0.0.0:53)
  --http <addr:port>   HTTP listener         (default 0.0.0.0:80)
  --https <addr:port>  HTTPS listener        (default 0.0.0.0:443)
  --smtp <addr:port>   SMTP listener: records mail, never relays it (off
                       unless given; port 25 needs the right to bind it)
  --api <addr:port>    polling API (TLS)     (default 0.0.0.0:8443)
  --data <dir>         certificate and token (default ./chosoast-data)
  --keep <n>           interactions kept in memory (default 100000)
  --no-dns | --no-http | --no-https   disable a listener";

struct Args {
    zone: String,
    addresses: ZoneAddresses,
    dns: Option<SocketAddr>,
    smtp: Option<SocketAddr>,
    http: Option<SocketAddr>,
    https: Option<SocketAddr>,
    api: SocketAddr,
    data: PathBuf,
    keep: usize,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let addr = |v: Option<String>, flag: &str| -> Result<SocketAddr, String> {
        v.ok_or(format!("{flag} needs a value"))?
            .parse()
            .map_err(|_| format!("{flag}: invalid address"))
    };
    let mut out = Args {
        zone: String::new(),
        addresses: ZoneAddresses::default(),
        dns: Some(([0, 0, 0, 0], 53).into()),
        smtp: None,
        http: Some(([0, 0, 0, 0], 80).into()),
        https: Some(([0, 0, 0, 0], 443).into()),
        api: ([0, 0, 0, 0], 8443).into(),
        data: PathBuf::from("chosoast-data"),
        keep: 100_000,
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--zone" => {
                out.zone = args
                    .next()
                    .ok_or("--zone needs a value")?
                    .to_ascii_lowercase()
            }
            "--ip" => {
                out.addresses.ipv4 = Some(
                    args.next()
                        .ok_or("--ip needs a value")?
                        .parse()
                        .map_err(|_| "--ip: invalid IPv4 address")?,
                )
            }
            "--ipv6" => {
                out.addresses.ipv6 = Some(
                    args.next()
                        .ok_or("--ipv6 needs a value")?
                        .parse()
                        .map_err(|_| "--ipv6: invalid IPv6 address")?,
                )
            }
            "--dns" => out.dns = Some(addr(args.next(), "--dns")?),
            "--smtp" => out.smtp = Some(addr(args.next(), "--smtp")?),
            "--http" => out.http = Some(addr(args.next(), "--http")?),
            "--https" => out.https = Some(addr(args.next(), "--https")?),
            "--api" => out.api = addr(args.next(), "--api")?,
            "--data" => out.data = PathBuf::from(args.next().ok_or("--data needs a value")?),
            "--keep" => {
                out.keep = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|n| *n > 0)
                    .ok_or("--keep needs a positive number")?
            }
            "--no-dns" => out.dns = None,
            "--no-http" => out.http = None,
            "--no-https" => out.https = None,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if out.zone.is_empty() || !out.zone.contains('.') {
        return Err("--zone is required (a domain delegated to this server)".into());
    }
    if out.addresses.ipv4.is_none() {
        return Err("--ip is required (the address names in the zone resolve to)".into());
    }
    Ok(out)
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(path)?, data)
}

/// Loads or creates the certificate and the access token.
fn load_or_create(dir: &Path, zone: &str) -> Result<(SelfSigned, String, bool), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let (cert_path, key_path, token_path) =
        (dir.join("cert.der"), dir.join("key.der"), dir.join("token"));
    let cert = match (std::fs::read(&cert_path), std::fs::read(&key_path)) {
        (Ok(c), Ok(k)) => SelfSigned::from_parts(c, k),
        _ => {
            let cert = SelfSigned::generate(zone)
                .map_err(|e| format!("cannot create a certificate: {e}"))?;
            write_private(&cert_path, &cert.cert_der).map_err(|e| e.to_string())?;
            write_private(&key_path, &cert.key_der).map_err(|e| e.to_string())?;
            cert
        }
    };
    let (token, created) = match std::fs::read_to_string(&token_path) {
        Ok(t) if t.trim().len() >= 32 => (t.trim().to_string(), false),
        _ => {
            let mut raw = [0u8; 32];
            ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut raw)
                .map_err(|_| "no secure random source")?;
            let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
            let _ = std::fs::remove_file(&token_path);
            write_private(&token_path, token.as_bytes()).map_err(|e| e.to_string())?;
            (token, true)
        }
    };
    Ok((cert, token, created))
}

#[tokio::main]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let (cert, token, created) = match load_or_create(&args.data, &args.zone) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let tls = match cert.server_config() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let server = match OastServer::start(ServerConfig {
        zone: args.zone.clone(),
        addresses: args.addresses,
        http: args.http,
        https: args.https.map(|a| (a, tls.clone())),
        dns: args.dns,
        smtp: args.smtp,
        keep: args.keep,
    })
    .await
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot listen: {e} (ports below 1024 need privileges)");
            std::process::exit(1);
        }
    };
    let api = match tokio::net::TcpListener::bind(args.api).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot listen on {}: {e}", args.api);
            std::process::exit(1);
        }
    };
    println!(
        "ChosOAST {} — zone {}",
        env!("CARGO_PKG_VERSION"),
        args.zone
    );
    println!(
        "  DNS   {:?}\n  HTTP  {:?}\n  HTTPS {:?}",
        server.dns_addr(),
        server.http_addr(),
        server.https_addr()
    );
    println!("  API   {} (TLS)", args.api);
    println!("  certificate SHA-256: {}", cert.fingerprint);
    if created {
        println!(
            "  access token (shown once, stored in {}): {token}",
            args.data.join("token").display()
        );
    } else {
        println!(
            "  access token: stored in {}",
            args.data.join("token").display()
        );
    }
    println!(
        "Interactions are kept in memory only; enter the API address, token and fingerprint in ChosProxy → OAST."
    );
    chosproxy_oast::api::serve(api, tls, token, args.zone, server.log().clone()).await;
}
