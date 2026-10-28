//! `anyremoted` — daemon de anyremote (integración de los ciclos S0–C8).
//!
//! Uso:
//!   anyremoted listen  [--bind 127.0.0.1:21118] [--clipboard off|memory|win] [--grant keyboard,mouse] [--allow-input] [--max-secs 600] [--max-sessions 1]
//!   anyremoted connect <host:puerto> | --profile <alias>  [--clipboard ...] [--grant ...] [--send-file <ruta>] [--max-secs ...]
//!   anyremoted profiles [--config-dir <dir>] [--list] [--add <alias,host,puerto>] [--remove <alias>]
//!   anyremoted id [--config-dir <dir>]     (ID estable de este equipo, D-03)
//!
//! Salida: eventos JSONL por stdout (para UI) + log de auditoría en <config>/audit/.
//! stdin: en `connect`, líneas `{"input":{key|pointer|button}}` → eventos de
//! entrada reenviados al controlado (RF-02a) y `bye` para cerrar la sesión; en
//! `listen`, `{"perm":...}` (toggles D-07) y `bye` para detener la escucha
//! (v0.8.2: con `--max-sessions 0` el listener vuelve a aceptar tras cada
//! sesión y solo `bye`/señal lo detiene).

use std::path::{Path, PathBuf};

use anyremote_audio::{AudioMode, AudioSource};
use anyremote_codec::CodecFormat;
use anyremote_daemon::video::VideoMode;
use anyremote_daemon::{resolve_profile, run_connect, run_listen, ClipboardMode, DaemonConfig};

fn config_dir(flag_dir: Option<String>) -> PathBuf {
    flag_dir.map(PathBuf::from).unwrap_or_else(|| {
        anyremote_profiles::ProfilesStore::default_path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    })
}

/// Instrumentación de campo (C-5): TODO panico (en cualquier hilo) se añade a
/// `<config_dir>/panic.log` además de salir por stderr. Así, si el daemon del
/// anfitrión muere, el artefacto queda en la misma carpeta que `audit/` (que ya
/// se copía para diagnóstico) en lugar de perderse con la ventana de la app.
fn install_panic_hook(config_dir: &Path) {
    let dir = config_dir.to_path_buf();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "payload no textual".to_string()
        };
        let loc = info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "?".to_string());
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let text = format!(
            "ts={ts} hilo={:?} ubicacion={loc}\npanico: {msg}\n",
            std::thread::current().name().unwrap_or("<sin nombre>")
        );
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("panic.log"))
        {
            use std::io::Write;
            let _ = f.write_all(text.as_bytes());
        }
        eprint!("{text}");
    }));
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
        [host, port]
            if port.chars().all(|c| c.is_ascii_digit())
                && !port.is_empty()
                && !host.contains(':') =>
        {
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
    let cmd = args
        .first()
        .map(String::as_str)
        .unwrap_or("help")
        .to_string();
    let cfg_dir = config_dir(flag_value(&args, "--config-dir"));
    install_panic_hook(&cfg_dir);

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
            cfg.one_way_files = args.iter().any(|a| a == "--one-way-files");
            // RF-02a (D-07): inyección de teclado/ratón — denegada salvo flag explícito.
            cfg.allow_input = args.iter().any(|a| a == "--allow-input");
            // RF audio: fuente de captura (off por defecto; `test` sin hardware).
            if let Some(a) = flag_value(&args, "--audio") {
                cfg.audio = AudioMode::parse(&a).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(2);
                });
            }
            // Fuentes: `system` (loopback, defecto: YouTube/altavoces), `mic`
            // o ambas separadas por coma (`system,mic` → se mezclan).
            if let Some(s) = flag_value(&args, "--audio-source") {
                let mut list: Vec<AudioSource> = Vec::new();
                for part in s.split(',') {
                    let src = AudioSource::parse(part.trim()).unwrap_or_else(|e| {
                        eprintln!("{e}");
                        std::process::exit(2);
                    });
                    if !list.contains(&src) {
                        list.push(src);
                    }
                }
                cfg.audio_sources = list;
            }
            if let Some(v) = flag_value(&args, "--max-secs") {
                cfg.max_secs = v.parse().unwrap_or(600);
            }
            // v0.8.2: sesiones que acepta el listener — 1 (one-shot, histórico)
            // o 0 = ilimitado: vuelve a aceptar tras cerrar cada sesión (la app
            // «Escuchar (red)» pasa 0 para permitir reconexiones).
            if let Some(v) = flag_value(&args, "--max-sessions") {
                cfg.max_sessions = v.parse().unwrap_or(1);
            }
            // Hito M2: flags de vídeo (solo tiene efecto en listen).
            if let Some(v) = flag_value(&args, "--video") {
                cfg.video = VideoMode::parse(&v).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(2);
                });
            }
            if let Some(sz) = flag_value(&args, "--video-size") {
                let s = sz.to_ascii_lowercase();
                if s == "native" || s == "auto" {
                    cfg.video_size = (0, 0); // sentinela: resolución nativa del capturer
                } else {
                    let parts: Vec<&str> = s.split('x').collect();
                    match (
                        parts.len(),
                        parts[0].parse::<u32>(),
                        parts.get(1).and_then(|h| h.parse::<u32>().ok()),
                    ) {
                        (2, Ok(w), Some(h)) if w > 0 && h > 0 => cfg.video_size = (w, h),
                        _ => {
                            eprintln!("--video-size espera native|WxH (p. ej. 960x540)");
                            std::process::exit(2);
                        }
                    }
                }
            }
            if let Some(f) = flag_value(&args, "--video-fps") {
                cfg.video_fps = f.parse().unwrap_or(10).clamp(1, 60);
            }
            // QW-3: tope de ancho de banda del video en kbps (ej. 3000 = 3 Mbps).
            if let Some(v) = flag_value(&args, "--max-bitrate") {
                let kbps: u64 = v.parse().unwrap_or_else(|_| {
                    eprintln!("--max-bitrate espera kbps (ej. 3000)");
                    std::process::exit(2);
                });
                cfg.video_max_bps = Some((kbps * 1000).max(150_000));
            }
            if let Some(c) = flag_value(&args, "--codec") {
                cfg.codec = match c.as_str() {
                    "vp9" => CodecFormat::Vp9,
                    "deflate" => CodecFormat::Deflate,
                    "raw" => CodecFormat::Raw,
                    other => {
                        eprintln!("--codec espera vp9|deflate|raw (recibido {other})");
                        std::process::exit(2);
                    }
                };
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
            cfg.one_way_files = args.iter().any(|a| a == "--one-way-files");
            // RF audio: reproductor (off por defecto; `test` decodifica sin dispositivo).
            if let Some(a) = flag_value(&args, "--audio") {
                cfg.audio = AudioMode::parse(&a).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(2);
                });
            }
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
        "id" => {
            // C-9: identidad estable del equipo (D-03) — la app y los CLI la
            // leen aquí para mostrar el ID SIN iniciar la escucha. Carga (o
            // genera la primera vez) el mismo `identity.key` que usa `listen`.
            match anyremote_identity::Identity::load_or_generate(&cfg_dir) {
                Ok(id) => println!("{}", id.node_id()),
                Err(e) => {
                    eprintln!("identidad: {e:?}");
                    std::process::exit(1);
                }
            }
            std::process::exit(0);
        }
        _ => {
            eprintln!(
                "anyremoted — integración anyremote (S0..C8) + vídeo M2 + entrada RF-02a + audio"
            );
            eprintln!("  anyremoted listen   [--bind a:p] [--video off|on|test] [--video-size native|1280x720] [--video-fps 10] [--max-bitrate kbps] [--codec vp9|deflate|raw] [--audio off|test|on] [--audio-source system|mic|system,mic] [--clipboard ...] [--allow-input] [--max-secs n] [--max-sessions n] (0 = escucha ilimitada)");
            eprintln!("  anyremoted connect <host:puerto> | --profile <alias> [--send-file ruta] [--one-way-files] [--audio off|test|on] [--clipboard ...]   (stdin: lineas de entrada o bye)");
            eprintln!("  anyremoted profiles [--list] [--add alias,host,puerto] [--remove alias]");
            eprintln!("  anyremoted id                                       (ID estable de este equipo, D-03)");
            std::process::exit(2);
        }
    };

    match result {
        Ok(out) => {
            eprintln!(
                "sesión cerrada: heartbeats={} rtt_min={} archivos={} bytes_sent={}",
                out.report.heartbeats_ok,
                out.report
                    .rtt_min_ms
                    .map(|v| format!("{v}ms"))
                    .unwrap_or_else(|| "-".into()),
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
                    println!(
                        "{}",
                        serde_json::json!({"event":"profile_saved","alias": parts[0]})
                    );
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
                println!(
                    "{}",
                    serde_json::json!({"event":"profile_removed","alias": alias})
                );
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
