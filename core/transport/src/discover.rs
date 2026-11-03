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

/// Broadcast de la subred de `ip`/`netmask`, o `None` si no existe una
/// dirección de broadcast real: **/31 y /32 no tienen broadcast** (para /32
/// el «cálculo» da la propia IP → un unicast sin listener que, en Windows,
/// provoca ICMP y **revienta el `recv_from` del query con 10054 al instante** —
/// observado en campo v0.11.1 con el adaptador `/32` de Cloudflare WARP).
pub fn subnet_broadcast(
    ip: std::net::Ipv4Addr,
    netmask: std::net::Ipv4Addr,
) -> Option<std::net::Ipv4Addr> {
    if u32::from(netmask).count_ones() >= 31 {
        return None; // /31 (P2P) y /32 (host): sin broadcast
    }
    let bcast = std::net::Ipv4Addr::from(u32::from(ip) | !u32::from(netmask));
    if bcast == ip {
        return None;
    }
    Some(bcast)
}

/// Candidatas de consulta: el destino configurado + la **broadcast de cada
/// subred local** IPv4. En equipos con varios adaptadores la broadcast
/// limitada `255.255.255.255` sale por la interfaz equivocada (observado en
/// campo v0.11.0: `192.168.6.255` respondía al instante y `255.255.255.255`
/// nunca llegaba) — por eso se abanica a todas las subredes locales, sin
/// incluir las que no tienen broadcast real (ver `subnet_broadcast`).
pub fn query_targets(configured: SocketAddr) -> Vec<SocketAddr> {
    let mut out = vec![configured];
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if let if_addrs::IfAddr::V4(v4) = iface.addr {
                if v4.ip.is_loopback() || v4.ip.is_link_local() {
                    continue;
                }
                if let Some(bcast) = subnet_broadcast(v4.ip, v4.netmask) {
                    let addr = SocketAddr::from((bcast, configured.port()));
                    if !out.contains(&addr) {
                        out.push(addr);
                    }
                }
            }
        }
    }
    out
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

/// Consulta `ANYREMOTE/1 WHO <id>` (reintentos) sobre el destino configurado
/// abanicado a sus subredes — ver [`query_targets`].
pub async fn query(target: SocketAddr, id: &str, tries: u8) -> Result<SocketAddr, String> {
    query_with_targets(&query_targets(target), id, tries).await
}

/// Núcleo de la consulta contra destinos ya calculados (exposed para tests).
///
/// Por intento: envío a TODOS los destinos (mejor esfuerzo) y espera de una
/// respuesta válida hasta el deadline (900 ms). **Los errores de recepción no
/// abortan**: en Windows un envío a un unicast sin listener (p. ej. la
/// «broadcast» de un adaptador /32) provoca ICMP y el `recv_from` devuelve
/// 10054 al instante — observado en campo v0.11.1: tratarlo como fatal mataba
/// la query en 30 ms sin esperar nunca la respuesta buena. Se consume el reset
/// y se sigue esperando hasta el deadline del intento.
pub async fn query_with_targets(
    targets: &[SocketAddr],
    id: &str,
    tries: u8,
) -> Result<SocketAddr, String> {
    let sock = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("bind udp: {e}"))?;
    // Requerido para enviar a 255.255.255.255; inofensivo en unicast.
    sock.set_broadcast(true)
        .map_err(|e| format!("broadcast: {e}"))?;
    let who = query_bytes(id);
    let mut buf = [0u8; 512];
    for _ in 0..tries.max(1) {
        for t in targets {
            let _ = sock.send_to(&who, *t).await;
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(900);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                break; // intento agotado → nuevo envío
            }
            match tokio::time::timeout(left, sock.recv_from(&mut buf)).await {
                Ok(Ok((n, src))) => {
                    if let Some(port) = parse_reply(&buf[..n], id) {
                        return Ok(SocketAddr::new(src.ip(), port));
                    }
                    // Respuesta ajena/basura: se ignora y se sigue esperando.
                }
                // ICMP (p. ej. adaptador /32): consumido — se sigue esperando.
                Ok(Err(_)) => {}
                Err(_elapsed) => break, // fin del intento
            }
        }
    }
    Err(format!(
        "sin respuesta del anfitrión {id} ({} destinos consultados)",
        targets.len()
    ))
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

    #[test]
    fn subnet_broadcast_skips_hosts_and_p2p() {
        // LAN /24 normal → broadcast de subred.
        assert_eq!(
            subnet_broadcast(
                "192.168.6.229".parse().unwrap(),
                "255.255.255.0".parse().unwrap()
            ),
            Some("192.168.6.255".parse().unwrap())
        );
        // /32 (caso Cloudflare WARP de campo v0.11.1): SIN broadcast — el
        // «cálculo» daría la propia IP → unicast sin listener → ICMP que
        // mataba la query al instante.
        assert_eq!(
            subnet_broadcast(
                "172.16.0.2".parse().unwrap(),
                "255.255.255.255".parse().unwrap()
            ),
            None
        );
        // /31 P2P: tampoco tiene broadcast.
        assert_eq!(
            subnet_broadcast(
                "10.0.0.0".parse().unwrap(),
                "255.255.255.254".parse().unwrap()
            ),
            None
        );
        // /16 grande.
        assert_eq!(
            subnet_broadcast(
                "172.27.80.1".parse().unwrap(),
                "255.255.0.0".parse().unwrap()
            ),
            Some("172.27.255.255".parse().unwrap())
        );
    }

    /// Regresión de campo v0.11.1: UN unicast sin listener en los destinos
    /// (la «broadcast» del /32 de WARP) generaba ICMP y el `recv_from`
    /// fallaba AL INSTANTE — el código lo trataba como fatal y la query
    /// abortaba en 30 ms sin esperar la respuesta buena. Ahora el reset se
    /// consume y se espera el deadline completo del intento.
    #[tokio::test]
    async fn recv_errors_do_not_abort_the_query() {
        // Nadie escucha en 127.0.0.2:59998 → ICMP instantáneo al enviar.
        let poison: SocketAddr = "127.0.0.2:59998".parse().unwrap();
        let started = std::time::Instant::now();
        let err = query_with_targets(&[poison], "GLV5Q76BAQ", 1)
            .await
            .expect_err("sin servidor");
        let elapsed = started.elapsed();
        assert!(err.contains("sin respuesta"), "{err}");
        assert!(
            elapsed >= std::time::Duration::from_millis(800),
            "debe esperar el deadline del intento, no abortar con el ICMP (fue {elapsed:?})"
        );
    }

    #[test]
    fn query_targets_fans_out_without_duplicates() {
        let cfg: SocketAddr = "255.255.255.255:21119".parse().unwrap();
        let t = query_targets(cfg);
        assert_eq!(t[0], cfg, "el configurado va primero");
        assert!(t.iter().all(|a| a.port() == 21119), "puerto conservado");
        let mut dedup = t.clone();
        dedup.dedup();
        assert_eq!(dedup.len(), t.len(), "sin duplicados");
        // Unicast de test: también se abanica (fidelidad con producción).
        let loop_cfg: SocketAddr = "127.0.0.1:23456".parse().unwrap();
        let t2 = query_targets(loop_cfg);
        assert_eq!(t2[0], loop_cfg);
        assert!(t2.iter().all(|a| a.port() == 23456));
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
