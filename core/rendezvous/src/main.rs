//! `anyremote-rdv` — servidor de encuentro + relay de anyremote (C-17 / D-04).
//!
//! Uso:
//!   anyremote-rdv [--bind 0.0.0.0:21120] [--max-pairs 64]
//!
//! Un único puerto TCP sirve las tres funciones del protocolo
//! `anyremote-rdv-v1`: registro de hosts (`register`/`hb`/`invite`),
//! resolución de ids (`resolve`) y pipeline de relay por ticket (`relay`).
//! Diseño y modelo de amenazas: `specs/02` §10.
//!
//! Salida: eventos por stderr (inicio/errores) — sin secretos.

use anyremote_rendezvous::{serve, Limits};

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "anyremote-rdv — servidor de encuentro + relay ({})",
            anyremote_rendezvous::PROTO
        );
        eprintln!("  anyremote-rdv [--bind 0.0.0.0:21120] [--max-pairs 64]");
        std::process::exit(0);
    }
    let bind = flag_value(&args, "--bind").unwrap_or_else(|| "0.0.0.0:21120".into());
    let limits = Limits {
        max_pairs: flag_value(&args, "--max-pairs")
            .and_then(|v| v.parse().ok())
            .unwrap_or(Limits::default().max_pairs),
        ..Limits::default()
    };
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("no se pudo bindear {bind}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "{}: anyremote-rdv escuchando en {} (parejas relay max {})",
        anyremote_rendezvous::PROTO,
        listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| bind),
        limits.max_pairs
    );
    if let Err(e) = serve(listener, limits).await {
        eprintln!("servidor detenido: {e}");
        std::process::exit(1);
    }
}
