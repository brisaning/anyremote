//! `anyremoted` — daemon de anyremote (integración de los ciclos S0–C8).
//!
//! Uso:
//!   anyremoted listen  [--bind 127.0.0.1:21118] [--clipboard off|memory|win] [--grant keyboard,mouse] [--max-secs 600]
//!   anyremoted connect <host:puerto> | --profile <alias>  [--clipboard ...] [--grant ...] [--send-file <ruta>] [--max-secs ...]
//!   anyremoted profiles [--config-dir <dir>] [--list] [--add <alias,host,puerto>] [--remove <alias>]
//!
//! Salida: eventos JSONL por stdout (para UI) + log de auditoría en <config>/audit/.

use std::path::PathBuf;

use anyremote_daemon::{resolve_profile, run_connect, run_listen, ClipboardMode, DaemonConfig};

fn config_dir(flag_dir: Option<String>) -> PathBuf {
    flag_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            anyremote_profiles::ProfilesStore::default_path()
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn parse_grant(args: &[String], default: &[&str]) -> Vec<String> {
    match flag_value(args, "--grant") {
        Some(v) => v
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect(),
        None => default.iter().map(|s| s.to_string()).collect(),
    }
}

/// Normaliza destinos de CLI: `host,puerto` y `alias,host,puerto` → `host:puerto`.
fn normalize_cli_target(s: &str) -> String {
    let t = s.trim();
    let parts: Vec<&str> = t.split(',').collect();
    match parts.as_slice() {
        [host, port] if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() && !host.contains(':') => {
            format!("{host}:{port}")
        }
        [_, host, port] if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => {
            format!("{host}:{port}")
        }
        _ => t.to_string(),
    }
}

#[tokio::main]
async fn main() {
    eprintln!("anyremoted {}", anyremote_daemon::DAEMON_VERSION);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help").to_string();
    let cfg_dir = config_dir(flag_value(&args, "--config-dir"));

    let result = match cmd.as_str() {
        "listen" => {
            let mut cfg = DaemonConfig::new(cfg_dir);
            if let Some(b) = flag_value(&args, "--bind") {
                cfg.bind = b;
            }
            if let Some(c) = flag_value(&args, "--clipboard") {
                cfg.clipboard = ClipboardMode::parse(&c).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(2);
                });
            }
            cfg.grant = parse_grant(&args, &["keyboard", "mouse"]);
            if let Some(v) = flag_value(&args, "--max-secs") {
                cfg.max_secs = v.parse().unwrap_or(600);
            }
            cfg.on_event = Some(Box::new(|line: String| {
                println!("{line}");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }));
            run_listen(cfg).await
        }
        "connect" => {
            let mut cfg = DaemonConfig::new(cfg_dir);
            let target = if let Some(alias) = flag_value(&args, "--profile") {
                let dir = cfg.config_dir.clone();
                resolve_profile(&alias, &dir).unwrap_or_else(|e| {
                    eprintln!("perfil: {e}");
                    std::process::exit(2);
                })
            } else {
                args.iter()
                    .find(|a| !a.starts_with("--") && cmd.as_str() != a.as_str())
                    .map(|s| normalize_cli_target(s))
                    .unwrap_or_default()
            };
            if target.is_empty() {
                eprintln!("uso: anyremoted connect <host:puerto> | --profile <alias>");
                std::process::exit(2);
            }
            cfg.target = Some(target);
            if let Some(c) = flag_value(&args, "--clipboard") {
                cfg.clipboard = ClipboardMode::parse(&c).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(2);
                });
            }
            cfg.grant = parse_grant(&args, &["keyboard", "mouse"]);
            if let Some(f) = flag_value(&args, "--send-file") {
                cfg.send_file = Some(PathBuf::from(f));
            }
            if let Some(v) = flag_value(&args, "--max-secs") {
                cfg.max_secs = v.parse().unwrap_or(600);
            }
            cfg.on_event = Some(Box::new(|line: String| {
                println!("{line}");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }));
            run_connect(cfg).await
        }
        "profiles" => {
            std::process::exit(run_profiles(&args, &cfg_dir));
        }
        _ => {
            eprintln!("anyremoted — integración anyremote (S0..C8)");
            eprintln!("  anyremoted listen   [--bind a:p] [--clipboard off|memory|win] [--grant k,m] [--max-secs n]");
            eprintln!("  anyremoted connect <host:puerto> | --profile <alias> [--send-file ruta] [--clipboard ...]");
            eprintln!("  anyremoted profiles [--list] [--add alias,host,puerto] [--remove alias]");
            std::process::exit(2);
        }
    };

    match result {
        Ok(out) => {
            eprintln!(
                "sesión cerrada: heartbeats={} rtt_min={:?}ms archivos={} bytes_sent={}",
                out.report.heartbeats_ok,
                out.report.rtt_min_ms,
                out.report.files_transferred,
                out.report.bytes_sent
            );
        }
        Err(e) => {
            let line = serde_json::json!({"event": "error", "detail": e.to_string()}).to_string();
            println!("{line}");
            std::process::exit(1);
        }
    }
}

fn run_profiles(args: &[String], cfg_dir: &std::path::Path) -> i32 {
    use anyremote_profiles::{ConnectionProfile, ProfilesStore};
    let file = cfg_dir.join(anyremote_profiles::keys::PROFILES_FILE);
    let store = ProfilesStore::at(file);

    if let Some(spec) = flag_value(args, "--add") {
        let parts: Vec<&str> = spec.split(',').collect();
        if parts.len() != 3 {
            eprintln!("--add espera alias,host,puerto");
            return 2;
        }
        let port: u16 = match parts[2].parse() {
            Ok(p) => p,
            Err(_) => {
                eprintln!("puerto inválido: {}", parts[2]);
                return 2;
            }
        };
        match ConnectionProfile::new(parts[0], parts[1], port) {
            Ok(p) => match store.upsert(p) {
                Ok(()) => {
                    println!("{}", serde_json::json!({"event":"profile_saved","alias": parts[0]}));
                    return 0;
                }
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            },
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    }
    if let Some(alias) = flag_value(args, "--remove") {
        match store.remove(&alias) {
            Ok(true) => {
                println!("{}", serde_json::json!({"event":"profile_removed","alias": alias}));
                return 0;
            }
            Ok(false) => {
                eprintln!("perfil no encontrado: {alias}");
                return 1;
            }
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        }
    }

    // --list (por defecto)
    match store.load() {
        Ok(list) => {
            for p in list {
                println!(
                    "{}",
                    serde_json::json!({"event":"profile","alias": p.id, "target": p.resolve(), "codec": p.codec})
                );
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
