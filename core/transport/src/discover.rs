//! Resolución de IDs por descubrimiento LAN (C-16, RF-03; diseño `specs/02` §9).
//!
//! Consulta activa UDP **broadcast** + respuesta **unicast**, sin servidor
//! (D-04 preveía un servidor propio para WAN/Fase 4; para LAN no hace falta
//! infraestructura): el controlador emite `ANYREMOTE/1 WHO <id>` y el
//! anfitrión —con `listen` en marcha— responde `ANYREMOTE/1 HERE <id>
//! <puerto>` **desde su propio socket**, así la IP de origen de la respuesta
//! es la dirección correcta del anfitrión (multi-homing sin ambigüedad).
//!
//! **La resolución NO autentica**: cualquiera puede responder (y un impostor
//! puede dar una dirección falsa). La identidad la verifica **C2** en el
//! handshake, y `connect` por ID exige además que el par sea exactamente ese
//! ID antes de recordar TOFU/guardar pin (specs/02 §9). El caché
//! `resolved/<ID>.txt` solo se escribe tras una respuesta viva.
//!
//! Throttling por origen (≤1 respuesta/500 ms) = patrón H-RD-02.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;

/// Descubrimiento por defecto: broadcast en el puerto 21119 (junto a 21118).
pub const DEFAULT_DISCOVER: &str = "255.255.255.255:21119";

const MAGIC: &str = "ANYREMOTE/1";

/// `--discover` → dirección de consulta (connect): `ip:puerto` o solo `puerto`
/// (→ broadcast por defecto).
pub fn parse_target(s: &str) -> Result<SocketAddr, String> {
    if let Ok(a) = s.parse::<SocketAddr>() {
        return Ok(a);
    }
    if let Ok(p) = s.parse::<u16>() {
        return Ok(SocketAddr::from((std::net::Ipv4Addr::BROADCAST, p)));
    }
    Err(format!(
        "--discover espera ip:puerto o puerto (recibido {s})"
    ))
}

/// Puerto de escucha del descubrimiento (listen): `ip:puerto` o solo `puerto`.
pub fn parse_port(s: &str) -> Option<u16> {
    parse_target(s).ok().map(|a| a.port())
}

/// Consulta `ANYREMOTE/1 WHO <id>`.
pub fn query_bytes(id: &str) -> Vec<u8> {
    format!("{MAGIC} WHO {id}\n").into_bytes()
}

/// Respuesta `ANYREMOTE/1 HERE <id> <puerto_listen>`.
pub fn here_bytes(id: &str, port: u16) -> Vec<u8> {
    format!("{MAGIC} HERE {id} {port}\n").into_bytes()
}

/// Parsea una consulta → el ID pedido (validado con la forma del `NodeId`).
pub fn parse_query(buf: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(buf).ok()?.trim();
    let mut it = s.split_whitespace();
    if it.next()? != MAGIC || it.next()? != "WHO" {
        return None;
    }
    let id = it.next()?;
    if it.next().is_some() {
        return None;
    }
    anyremote_identity::NodeId::parse(id)
        .ok()
        .map(|n| n.to_string())
}

/// Parsea una respuesta HERE contra el ID esperado → puerto de escucha.
pub fn parse_reply(buf: &[u8], expected_id: &str) -> Option<u16> {
    let s = std::str::from_utf8(buf).ok()?.trim();
    let mut it = s.split_whitespace();
    if it.next()? != MAGIC || it.next()? != "HERE" {
        return None;
    }
    let id = it.next()?;
    let port: u16 = it.next()?.parse().ok()?;
    if it.next().is_some() || port == 0 {
        return None;
    }
    let want = anyremote_identity::NodeId::parse(expected_id).ok()?;
    if anyremote_identity::NodeId::parse(id).ok()? != want {
        return None;
    }
    Some(port)
}

/// Servidor de descubrimiento del anfitrión: responde HERE **solo** a
/// consultas por NUESTRO id, con throttling por origen (H-RD-02). Se crea
/// durante `listen` y se detiene con el `Drop` (aborta la tarea) cuando la
/// escucha termina.
pub struct DiscoveryServer {
    handle: tokio::task::JoinHandle<()>,
    local: SocketAddr,
}

impl DiscoveryServer {
    /// Bind `0.0.0.0:port` (recibe broadcast y unicast — tests en loopback)
    /// y atiende consultas en background. `listen_port` es el puerto que
    /// anunciamos (el del listener real).
    pub async fn spawn(port: u16, node_id: String, listen_port: u16) -> std::io::Result<Self> {
        let sock = UdpSocket::bind(("0.0.0.0", port)).await?;
        let local = sock.local_addr()?;
        let handle = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let mut last: HashMap<SocketAddr, Instant> = HashMap::new();
            loop {
                let Ok((n, src)) = sock.recv_from(&mut buf).await else {
                    break;
                };
                let Some(want) = parse_query(&buf[..n]) else {
                    continue;
                };
                if want != node_id {
                    continue; // el ID pedido no es el nuestro
                }
                // Throttling por origen (H-RD-02): una respuesta / 500 ms.
                let now = Instant::now();
                if last
                    .get(&src)
                    .is_some_and(|t| now.duration_since(*t) < Duration::from_millis(500))
                {
                    continue;
                }
                last.insert(src, now);
                let _ = sock.send_to(&here_bytes(&node_id, listen_port), src).await;
            }
        });
        Ok(Self { handle, local })
    }

    /// Dirección real de escucha (con puerto si se pidió 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }
}

impl Drop for DiscoveryServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Consulta activa: emite WHO a `target` (hasta `tries` veces) y espera HERE
/// del `id` → `ip_del_anfitrión:puerto`. La IP sale del **origen** de la
/// respuesta. Timeout por intento: 900 ms (~2,7 s en total).
pub async fn query(target: SocketAddr, id: &str, tries: u8) -> Result<SocketAddr, String> {
    let sock = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("bind udp: {e}"))?;
    // Requerido para enviar a 255.255.255.255; inofensivo en unicast.
    sock.set_broadcast(true)
        .map_err(|e| format!("broadcast: {e}"))?;
    let who = query_bytes(id);
    let mut buf = [0u8; 512];
    for _ in 0..tries.max(1) {
        sock.send_to(&who, target)
            .await
            .map_err(|e| format!("enviar consulta: {e}"))?;
        match tokio::time::timeout(Duration::from_millis(900), sock.recv_from(&mut buf)).await {
            Ok(Ok((n, src))) => {
                if let Some(port) = parse_reply(&buf[..n], id) {
                    return Ok(SocketAddr::new(src.ip(), port));
                }
                // Respuesta ajena/basura: se ignora y se reintenta.
            }
            Ok(Err(e)) => return Err(format!("recibir respuesta: {e}")),
            Err(_elapsed) => {} // sin respuesta → nuevo intento
        }
    }
    Err(format!("sin respuesta del anfitrión {id} en {target}"))
}

/// Caché de resoluciones: `resolved/<ID>.txt` con `ip:port`. Solo se escribe
/// tras una respuesta viva (nunca se asume dirección); se lee cuando la
/// broadcast no responde (firewall/subred).
pub fn save_resolved(dir: &Path, id: &str, addr: SocketAddr) -> Result<(), crate::TransportError> {
    let d = dir.join("resolved");
    std::fs::create_dir_all(&d).map_err(|e| crate::TransportError::Io(e.to_string()))?;
    std::fs::write(d.join(format!("{id}.txt")), format!("{addr}\n"))
        .map_err(|e| crate::TransportError::Io(e.to_string()))
}

pub fn load_resolved(dir: &Path, id: &str) -> Option<SocketAddr> {
    let txt = std::fs::read_to_string(dir.join("resolved").join(format!("{id}.txt"))).ok()?;
    txt.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "GLV5Q76BAQ";

    #[test]
    fn query_and_reply_roundtrip() {
        assert_eq!(parse_query(&query_bytes(ID)).as_deref(), Some(ID));
        assert_eq!(parse_reply(&here_bytes(ID, 21118), ID), Some(21118));
    }

    #[test]
    fn parse_rejects_garbage_version_and_wrong_id() {
        assert_eq!(parse_query(b"hola mundo"), None);
        assert_eq!(parse_query(b"ANYREMOTE/9 WHO GLV5Q76BAQ"), None); // versión
        assert_eq!(parse_query(b"ANYREMOTE/1 WHO GLV5Q76BAQ EXTRA"), None);
        assert_eq!(parse_query(b"ANYREMOTE/1 WHO no-es-id"), None);
        assert_eq!(parse_reply(b"ANYREMOTE/1 HERE ZZZZZZZZZZ 21118", ID), None); // id ajeno
        assert_eq!(parse_reply(&here_bytes(ID, 0), ID), None); // puerto 0
        assert_eq!(parse_reply(b"ANYREMOTE/1 HERE GLV5Q76BAQ 99999", ID), None);
        // >u16
    }

    #[tokio::test]
    async fn query_resolves_loopback_server() {
        let srv = DiscoveryServer::spawn(0, ID.to_string(), 21118)
            .await
            .expect("bind");
        // (0.0.0.0 no es destino válido en Windows → se consulta en loopback)
        let q = SocketAddr::from(([127, 0, 0, 1], srv.local_addr().port()));
        let addr = query(q, ID, 3).await.expect("respuesta");
        assert_eq!(addr, SocketAddr::from(([127, 0, 0, 1], 21118)));
    }

    #[tokio::test]
    async fn query_times_out_for_foreign_id() {
        let srv = DiscoveryServer::spawn(0, ID.to_string(), 21118)
            .await
            .expect("bind");
        let q = SocketAddr::from(([127, 0, 0, 1], srv.local_addr().port()));
        let err = query(q, "AAAAAAAAAA", 1)
            .await
            .expect_err("el servidor no responde por otro id");
        assert!(err.contains("sin respuesta"), "{err}");
    }

    #[test]
    fn resolved_cache_roundtrip() {
        let dir = std::env::temp_dir().join(format!("anyremote-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let addr: SocketAddr = "192.168.6.152:21118".parse().unwrap();
        save_resolved(&dir, ID, addr).expect("save");
        assert_eq!(load_resolved(&dir, ID), Some(addr));
        assert_eq!(load_resolved(&dir, "AAAAAAAAAA"), None);
        // archivo corrupto → None (nunca una dirección inválida)
        std::fs::write(dir.join("resolved").join("AAAAAAAAAA.txt"), "basura\n").unwrap();
        assert_eq!(load_resolved(&dir, "AAAAAAAAAA"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
