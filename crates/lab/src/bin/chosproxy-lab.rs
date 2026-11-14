//! `chosproxy-lab` — run the local lab server for manual testing.
//!
//! Usage: `chosproxy-lab [--port N] [--tls-port N] [--allow-remote]`
//! Serves HTTP on `--port` (default 9090) and HTTPS with a self-signed
//! certificate on `--tls-port` (default 9443). Binds 127.0.0.1 unless
//! `--allow-remote` is given.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut port: u16 = 9090;
    let mut tls_port: u16 = 9443;
    let mut ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .ok_or_else(|| std::io::Error::other("--port needs a number"))?;
            }
            "--tls-port" => {
                tls_port = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .ok_or_else(|| std::io::Error::other("--tls-port needs a number"))?;
            }
            "--allow-remote" => ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            "-h" | "--help" => {
                eprintln!("usage: chosproxy-lab [--port N] [--tls-port N] [--allow-remote]");
                return Ok(());
            }
            other => return Err(std::io::Error::other(format!("unknown argument {other}"))),
        }
    }
    let lab = chosproxy_lab::LabServer::start(SocketAddr::new(ip, port)).await?;
    let lab_tls = chosproxy_lab::LabServer::start_tls(SocketAddr::new(ip, tls_port)).await?;
    eprintln!(
        "ChosProxy lab listening on {} and {} (self-signed)",
        lab.url(),
        lab_tls.url()
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}
