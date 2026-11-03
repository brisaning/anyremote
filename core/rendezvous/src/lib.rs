//! `anyremote-rendezvous` — servidor de encuentro + relay de anyremote
//! (C-17, D-04; diseño en `specs/02` §10).
//!
//! Protocolo **`anyremote-rdv-v1`**: TCP único (por defecto `:21120`),
//! mensajes JSON por línea. Tres modos según el primer mensaje de la conexión:
//! `register` (host persistente: `hb` + `invite`), `resolve` (controlador,
//! una consulta) y `relay` (admisión al pipeline por ticket).
//!
//! **Seguridad (decisión documentada en §10)**: el registro está **firmado**
//! con la clave del propio id (`Identity::sign_auth` + id derivado de la pk —
//! imposible reclamar un id ajeno, H-RD-02), con ventana de `ts` ±300 s y
//! anti-replay `(id,ts)`; la resolución es un dato semipúblico con throttling
//! por IP (H-RD-04). El canal de control va **en claro** — igual que el
//! broadcast de C-16: alterar una resolución no suplanta a nadie porque **C2
//! ancla la sesión** y las sesiones son E2E (TLS/QUIC entre pares): el relay
//! es una **tubería ciega**.
//!
//! Límites (D-08): tickets de 60 s y uso único · throttle por IP · cupo y
//! duración máxima de parejas relay · almacén en memoria (los hosts
//! re-registran cada 30 s → auto-sanación tras reinicios del servidor).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use anyremote_identity::{Identity, NodeId};

/// Versión del protocolo de encuentro.
pub const PROTO: &str = "anyremote-rdv-v1";

/// Binding canónico de un registro (lo firma el host con `sign_auth`).
pub fn registration_binding(id: &str, pk_hex: &str, port: u16, ts: u64) -> String {
    format!("{PROTO} register {id} {pk_hex} {port} {ts}")
}

const MAX_LINE: usize = 4096;

/// Límites operativos del servidor (H-RD-02/04, D-08) — configurables (tests).
#[derive(Clone, Debug)]
pub struct Limits {
    pub register_per_min: u32,
    pub resolve_per_min: u32,
    pub ts_skew_secs: u64,
    pub peer_alive_secs: u64,
    pub ticket_ttl_secs: u64,
    pub max_pairs: usize,
    pub pair_max_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            register_per_min: 30,
            resolve_per_min: 60,
            ts_skew_secs: 300,
            peer_alive_secs: 90,
            ticket_ttl_secs: 60,
            max_pairs: 64,
            pair_max_secs: 4 * 3600,
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================ protocolo ============================

/// Verifica un registro: id coherente con la pk, `ts` en ventana y firma
/// válida (H-RD-02: el id DERIVADO del pk hace imposible reclamar un id ajeno).
pub fn verify_registration(
    id: &str,
    pk_hex: &str,
    port: u16,
    ts: u64,
    sig_hex: &str,
    now: u64,
    skew: u64,
) -> Result<[u8; 32], &'static str> {
    let pk_raw = data_encoding::HEXLOWER
        .decode(pk_hex.as_bytes())
        .map_err(|_| "pk invalida")?;
    let pk: [u8; 32] = pk_raw.as_slice().try_into().map_err(|_| "pk invalida")?;
    if port == 0 {
        return Err("puerto invalido");
    }
    let claimed = NodeId::parse(id).map_err(|_| "id invalido")?;
    let derived = Identity::derive_node_id(&pk).map_err(|_| "pk invalida")?;
    if derived != claimed {
        return Err("el id no corresponde a la pk");
    }
    if ts < now.saturating_sub(skew) || ts > now + skew {
        return Err("ts fuera de ventana");
    }
    let sig_raw = data_encoding::HEXLOWER
        .decode(sig_hex.as_bytes())
        .map_err(|_| "sig invalida")?;
    let sig: [u8; 64] = sig_raw.as_slice().try_into().map_err(|_| "sig invalida")?;
    let binding = registration_binding(id, pk_hex, port, ts);
    if !Identity::verify_auth(&pk, binding.as_bytes(), &sig) {
        return Err("firma invalida");
    }
    Ok(pk)
}

/// Línea de registro firmada (cliente host).
pub fn register_line(identity: &Identity, port: u16, ts: u64) -> String {
    let pk_hex = data_encoding::HEXLOWER.encode(&identity.verifying_key_bytes());
    let id = identity.node_id().to_string();
    let binding = registration_binding(&id, &pk_hex, port, ts);
    let sig_hex = data_encoding::HEXLOWER.encode(&identity.sign_auth(binding.as_bytes()));
    serde_json::json!({
        "t": "register", "id": id, "pk": pk_hex, "port": port, "ts": ts, "sig": sig_hex,
    })
    .to_string()
}

/// Línea de consulta (cliente controlador; sin firma — throttling por IP).
pub fn resolve_line(id: &str) -> String {
    serde_json::json!({ "t": "resolve", "id": id, "ts": now_unix() }).to_string()
}

/// Línea de `hb` del host.
pub fn hb_line() -> String {
    serde_json::json!({ "t": "hb", "ts": now_unix() }).to_string()
}

/// Lectura de UNA línea de control **byte a byte**: nunca sobre-consume —
/// los bytes posteriores al `\n` pueden ser el TLS de los pares del relay.
pub async fn read_line_from<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> std::io::Result<String> {
    let mut buf = Vec::with_capacity(64);
    let mut b = [0u8; 1];
    loop {
        r.read_exact(&mut b).await?;
        if b[0] == b'\n' {
            break;
        }
        buf.push(b[0]);
        if buf.len() > MAX_LINE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "linea excesiva",
            ));
        }
    }
    String::from_utf8(buf)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "linea no UTF-8"))
}

async fn write_line<W: AsyncWriteExt + Unpin>(w: &mut W, line: &str) -> std::io::Result<()> {
    w.write_all(format!("{line}\n").as_bytes()).await?;
    w.flush().await
}

fn err_line(code: &str) -> String {
    serde_json::json!({"t":"error","code":code}).to_string()
}

fn t_of(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    v.get("t").and_then(|t| t.as_str()).map(|s| s.to_string())
}

// ============================ servidor ============================

struct Peer {
    ip: IpAddr,
    port: u16,
    tx: mpsc::Sender<String>,
    gen: u64,
    last_seen: Instant,
}

struct TicketEntry {
    created: Instant,
    slots: Vec<TcpStream>,
}

struct State {
    limits: Limits,
    peers: HashMap<NodeId, Peer>,
    tickets: HashMap<String, TicketEntry>,
    reg_limits: RateWindow,
    res_limits: RateWindow,
    seen: HashMap<(String, u64), ()>,
    next_gen: u64,
    active_pairs: usize,
}

impl State {
    fn new(limits: Limits) -> Self {
        let reg = RateWindow::new(limits.register_per_min);
        let res = RateWindow::new(limits.resolve_per_min);
        Self {
            limits,
            peers: HashMap::new(),
            tickets: HashMap::new(),
            reg_limits: reg,
            res_limits: res,
            seen: HashMap::new(),
            next_gen: 1,
            active_pairs: 0,
        }
    }

    fn sweep(&mut self) {
        let alive = Duration::from_secs(self.limits.peer_alive_secs);
        let ttl = Duration::from_secs(self.limits.ticket_ttl_secs);
        self.peers.retain(|_, p| p.last_seen.elapsed() < alive);
        self.tickets.retain(|_, t| t.created.elapsed() < ttl);
    }
}

/// Ventana fija por minuto por origen (throttling H-RD-04).
struct RateWindow {
    per_min: u32,
    map: HashMap<IpAddr, (u64, u32)>,
}

impl RateWindow {
    fn new(per_min: u32) -> Self {
        Self {
            per_min,
            map: HashMap::new(),
        }
    }

    fn allow(&mut self, ip: IpAddr) -> bool {
        let minute = now_unix() / 60;
        let allowed = {
            let e = self.map.entry(ip).or_insert((minute, 0));
            if e.0 != minute {
                *e = (minute, 0);
            }
            e.1 += 1;
            e.1 <= self.per_min
        };
        if self.map.len() > 8192 {
            self.map.clear(); // cota bruta; la ventana por minuto autolimpia
        }
        allowed
    }
}

fn random_ticket() -> String {
    let mut raw = [0u8; 24];
    let _ = getrandom::getrandom(&mut raw);
    data_encoding::HEXLOWER.encode(&raw)
}

#[derive(Deserialize)]
struct RegisterReq {
    id: String,
    pk: String,
    port: u16,
    ts: u64,
    sig: String,
}

#[derive(Deserialize)]
struct Header {
    #[allow(dead_code)]
    t: String,
}

/// Validación **pura** de un registro (cripto + ventana de reloj) — se hace
/// ANTES de partir el socket para poder contestar errores con el socket entero.
fn validate_register(line: &str, skew: u64) -> Result<RegisterReq, &'static str> {
    let req: RegisterReq = serde_json::from_str(line).map_err(|_| "json")?;
    verify_registration(
        &req.id,
        &req.pk,
        req.port,
        req.ts,
        &req.sig,
        now_unix(),
        skew,
    )?;
    Ok(req)
}

/// Parte con estado: throttle, anti-replay e inserción del peer vigente.
fn admit_register(
    g: &mut State,
    req: &RegisterReq,
    ip: IpAddr,
    tx: mpsc::Sender<String>,
) -> Result<(u64, NodeId), &'static str> {
    if !g.reg_limits.allow(ip) {
        return Err("throttled");
    }
    if g.seen.contains_key(&(req.id.clone(), req.ts)) {
        return Err("replay");
    }
    let id = NodeId::parse(&req.id).map_err(|_| "id invalido")?;
    let now = now_unix();
    g.seen.insert((req.id.clone(), req.ts), ());
    g.seen
        .retain(|(_, ts), _| *ts + g.limits.ts_skew_secs >= now);
    let gen = g.next_gen;
    g.next_gen += 1;
    g.peers.insert(
        id.clone(),
        Peer {
            ip,
            port: req.port,
            tx,
            gen,
            last_seen: Instant::now(),
        },
    );
    Ok((gen, id))
}

/// Conexión de host: registro persistente → `hb` → `invite` salientes.
async fn host_connection(
    mut sock: TcpStream,
    src: SocketAddr,
    first: String,
    st: Arc<Mutex<State>>,
) -> std::io::Result<()> {
    // 1) validación pura (sin tocar el estado) — el error se contesta con el
    //    socket entero, antes de partirlo.
    let skew = { st.lock().unwrap().limits.ts_skew_secs };
    let req = match validate_register(&first, skew) {
        Ok(r) => r,
        Err(code) => {
            write_line(&mut sock, &err_line(code)).await?;
            return Ok(());
        }
    };

    // 2) canal + writer: el socket queda partido y la escritura (registered e
    //    invites) pasa por el canal en orden.
    let (rd, wr) = sock.into_split();
    let (tx, mut rx) = mpsc::channel::<String>(16);
    tokio::spawn(async move {
        let mut wr = wr;
        while let Some(line) = rx.recv().await {
            if write_line(&mut wr, &line).await.is_err() {
                break;
            }
        }
    });

    // 3) admisión con estado (throttle/anti-replay/insertar) con el tx real.
    //    El guard del mutex termina ANTES de cualquier await (el future debe
    //    ser Send para el spawn por conexión).
    let admitted = {
        let mut g = st.lock().unwrap();
        g.sweep();
        admit_register(&mut g, &req, src.ip(), tx.clone())
    };
    let (gen, id) = match admitted {
        Ok(v) => v,
        Err(code) => {
            let _ = tx.send(err_line(code)).await;
            return Ok(());
        }
    };
    let _ = tx
        .send(serde_json::json!({"t":"registered"}).to_string())
        .await;

    // 4) bucle: `hb` refresca la vida del peer (y detecta sustituciones).
    let mut rd = rd;
    loop {
        let line = match read_line_from(&mut rd).await {
            Ok(l) => l,
            Err(_) => break, // desconexión
        };
        if t_of(&line).as_deref() == Some("hb") {
            let mut g = st.lock().unwrap();
            if let Some(p) = g.peers.get_mut(&id) {
                if p.gen == gen {
                    p.last_seen = Instant::now();
                }
            }
        }
        // otras líneas se ignoran (el cliente registra al conectar).
    }

    // 5) retirada: solo si seguimos siendo el registro vigente de ese id.
    {
        let mut g = st.lock().unwrap();
        if let Some(p) = g.peers.get(&id) {
            if p.gen == gen {
                g.peers.remove(&id);
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct ResolveReq {
    #[allow(dead_code)]
    t: String,
    id: String,
}

async fn resolve_connection(
    mut sock: TcpStream,
    src: SocketAddr,
    first: String,
    st: Arc<Mutex<State>>,
) -> std::io::Result<()> {
    // Toda la decisión se toma bajo el lock y produce UNA línea de respuesta.
    let reply = {
        let mut g = st.lock().unwrap();
        g.sweep();
        if !g.res_limits.allow(src.ip()) {
            err_line("throttled")
        } else {
            let req: ResolveReq = match serde_json::from_str(&first) {
                Ok(r) => r,
                Err(_) => return Ok(()),
            };
            match NodeId::parse(&req.id) {
                Err(_) => err_line("id"),
                Ok(id) => match g.peers.get(&id) {
                    None => err_line("offline"),
                    Some(p) => {
                        let (ip, port, tx) = (p.ip, p.port, p.tx.clone());
                        let ticket = random_ticket();
                        g.tickets.insert(
                            ticket.clone(),
                            TicketEntry {
                                created: Instant::now(),
                                slots: Vec::new(),
                            },
                        );
                        let invite = serde_json::json!({
                            "t": "invite", "from": id.to_string(), "ticket": ticket
                        })
                        .to_string();
                        if tx.try_send(invite).is_ok() {
                            serde_json::json!({
                                "t": "answer",
                                "addr": format!("{ip}:{port}"),
                                "ticket": ticket
                            })
                            .to_string()
                        } else {
                            g.tickets.remove(&ticket);
                            err_line("offline")
                        }
                    }
                },
            }
        }
    };
    write_line(&mut sock, &reply).await?;
    Ok(())
}

#[derive(Deserialize)]
struct RelayReq {
    #[allow(dead_code)]
    t: String,
    ticket: String,
}

/// Admisión al pipeline: el segundo portador del ticket empareja y enlaza.
/// El PRIMERO devuelve `Ok(())` dejando su stream dentro de la entrada del
/// ticket (vive hasta el par o la caducidad del ticket).
async fn relay_admission(
    sock: TcpStream,
    _src: SocketAddr,
    first: String,
    st: Arc<Mutex<State>>,
) -> std::io::Result<()> {
    let req: RelayReq = match serde_json::from_str(&first) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    // `Option<TcpStream>`: el stream solo se «mueve» de forma observable
    // cuando lo necesitamos (patrón sin moves condicionales).
    let mut maybe_sock = Some(sock);
    let outcome: Result<Option<(TcpStream, TcpStream)>, &'static str> = {
        let mut g = st.lock().unwrap();
        g.sweep();
        if g.active_pairs >= g.limits.max_pairs {
            Err("relay_full")
        } else {
            let ttl = Duration::from_secs(g.limits.ticket_ttl_secs);
            match g.tickets.get_mut(&req.ticket) {
                Some(entry) if entry.created.elapsed() < ttl => {
                    entry
                        .slots
                        .push(maybe_sock.take().expect("sock disponible"));
                    if entry.slots.len() == 2 {
                        let entry = g.tickets.remove(&req.ticket).expect("dos slots");
                        g.active_pairs += 1;
                        let mut it = entry.slots.into_iter();
                        Ok(Some((it.next().unwrap(), it.next().unwrap())))
                    } else {
                        Ok(None) // primer portador: esperando al par
                    }
                }
                _ => Err("ticket"),
            }
        }
    };
    match outcome {
        Err(code) => {
            if let Some(mut s) = maybe_sock.take() {
                let _ = write_line(&mut s, &err_line(code)).await;
            }
            Ok(())
        }
        Ok(None) => Ok(()), // stream ya dentro de la entrada del ticket
        Ok(Some((mut a, mut b))) => {
            // Pipeline confirmado a ambos y enlace ciego.
            let piped = serde_json::json!({"t":"piped"}).to_string();
            if write_line(&mut a, &piped).await.is_err()
                || write_line(&mut b, &piped).await.is_err()
            {
                finish_pair(&st);
                return Ok(());
            }
            let st2 = st.clone();
            let max = Duration::from_secs(st.lock().unwrap().limits.pair_max_secs);
            tokio::spawn(async move {
                let _ =
                    tokio::time::timeout(max, tokio::io::copy_bidirectional(&mut a, &mut b)).await;
                finish_pair(&st2);
            });
            Ok(())
        }
    }
}

fn finish_pair(st: &Arc<Mutex<State>>) {
    let mut g = st.lock().unwrap();
    g.active_pairs = g.active_pairs.saturating_sub(1);
}

async fn handle_conn(
    mut sock: TcpStream,
    src: SocketAddr,
    st: Arc<Mutex<State>>,
) -> std::io::Result<()> {
    let first = read_line_from(&mut sock).await?;
    let header: Header = serde_json::from_str(&first)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "json"))?;
    match header.t.as_str() {
        "register" => host_connection(sock, src, first, st).await,
        "resolve" => resolve_connection(sock, src, first, st).await,
        "relay" => relay_admission(sock, src, first, st).await,
        _ => {
            write_line(&mut sock, &err_line("tipo")).await?;
            Ok(())
        }
    }
}

/// Sirve conexiones hasta que el listener muera.
pub async fn serve(listener: TcpListener, limits: Limits) -> std::io::Result<()> {
    let st = Arc::new(Mutex::new(State::new(limits)));
    loop {
        let (sock, src) = listener.accept().await?;
        let st = st.clone();
        tokio::spawn(async move {
            let _ = handle_conn(sock, src, st).await;
        });
    }
}

/// Bindea y sirve (bloquea) — la entrada del binario `anyremote-rdv`.
pub async fn run_server(bind: &str, limits: Limits) -> std::io::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    eprintln!(
        "{PROTO}: anyremote-rdv escuchando en {} (parejas relay max {})",
        listener.local_addr()?,
        limits.max_pairs
    );
    serve(listener, limits).await
}

// ============================ clientes ============================

/// Respuesta de `resolve`: dirección directa del anfitrión + ticket de relay.
#[derive(Debug, Clone)]
pub struct Answer {
    pub addr: SocketAddr,
    pub ticket: String,
}

/// Consulta única `resolve` → `answer` (o `error` con el código del servidor).
pub async fn resolve(target: SocketAddr, id: &str) -> Result<Answer, String> {
    let mut s = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(target))
        .await
        .map_err(|_| "timeout de conexion al servidor de encuentro".to_string())?
        .map_err(|e| format!("conexion: {e}"))?;
    write_line(&mut s, &resolve_line(id))
        .await
        .map_err(|e| format!("envio: {e}"))?;
    let line = tokio::time::timeout(Duration::from_secs(5), read_line_from(&mut s))
        .await
        .map_err(|_| "sin respuesta del servidor de encuentro".to_string())?
        .map_err(|e| format!("lectura: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&line).map_err(|_| "respuesta invalida".to_string())?;
    match v.get("t").and_then(|t| t.as_str()) {
        Some("answer") => {
            let addr = v
                .get("addr")
                .and_then(|a| a.as_str())
                .and_then(|a| a.parse().ok())
                .ok_or_else(|| "direccion invalida".to_string())?;
            let ticket = v
                .get("ticket")
                .and_then(|t| t.as_str())
                .ok_or_else(|| "sin ticket".to_string())?
                .to_string();
            Ok(Answer { addr, ticket })
        }
        Some("error") => Err(format!(
            "servidor: {}",
            v.get("code").and_then(|c| c.as_str()).unwrap_or("?")
        )),
        _ => Err("respuesta desconocida del servidor".to_string()),
    }
}

/// Admite el ticket en el relay y espera `piped`: devuelve la **tubería**
/// (un `TcpStream` normal) sobre la que los pares montan su TLS de extremo a
/// extremo (el relay nunca ve más que cifrado).
pub async fn establish_relay(relay: SocketAddr, ticket: &str) -> std::io::Result<TcpStream> {
    let mut s = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(relay))
        .await
        .map_err(|e| std::io::Error::other(format!("relay connect: {e}")))?
        .map_err(|e| std::io::Error::other(format!("relay connect: {e}")))?;
    write_line(
        &mut s,
        &serde_json::json!({"t":"relay","ticket":ticket}).to_string(),
    )
    .await?;
    // El par puede tardar hasta la vida del ticket (60 s) + margen.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(70);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return Err(std::io::Error::other("sin par para el ticket"));
        }
        let line = tokio::time::timeout(left, read_line_from(&mut s))
            .await
            .map_err(|_| std::io::Error::other("sin par para el ticket"))?
            .map_err(|e| std::io::Error::other(format!("relay: {e}")))?;
        match t_of(&line).as_deref() {
            Some("piped") => return Ok(s),
            Some("error") => {
                let code: String = serde_json::from_str(&line)
                    .ok()
                    .and_then(|v: serde_json::Value| {
                        v.get("code")
                            .and_then(|c| c.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "rechazado".to_string());
                return Err(std::io::Error::other(format!("relay: {code}")));
            }
            _ => {} // línea inesperada: se ignora y se sigue esperando
        }
    }
}

/// Tubería de relay ya emparejada, con el peer (el servidor de encuentro).
pub type RelayPipe = (TcpStream, SocketAddr);

/// Conexión de host persistente al servidor de encuentro: `register` + `hb`
/// (reconexión con backoff de 5 s) y, por cada `invite`, monta la tubería de
/// relay y la entrega por `pipes` al bucle del listener (que la trata como
/// una sesión TCP entrante: bootstrap → TLS → sesión, E2E sobre el relay).
pub async fn run_host_client(
    rdv: SocketAddr,
    identity: std::sync::Arc<Identity>,
    listen_port: u16,
    pipes: mpsc::Sender<RelayPipe>,
) {
    loop {
        if let Err(e) = host_client_once(rdv, &identity, listen_port, &pipes).await {
            eprintln!("rdv: sin conexion con {rdv} ({e}) — reintento en 5 s");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

async fn host_client_once(
    rdv: SocketAddr,
    identity: &Identity,
    listen_port: u16,
    pipes: &mpsc::Sender<RelayPipe>,
) -> std::io::Result<()> {
    let mut s = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(rdv))
        .await
        .map_err(|_| std::io::Error::other("timeout conectar servidor de encuentro"))?
        .map_err(|e| std::io::Error::other(format!("conectar: {e}")))?;
    write_line(&mut s, &register_line(identity, listen_port, now_unix())).await?;
    let ack = tokio::time::timeout(Duration::from_secs(5), read_line_from(&mut s))
        .await
        .map_err(|_| std::io::Error::other("sin ack del servidor de encuentro"))??;
    if t_of(&ack).as_deref() != Some("registered") {
        eprintln!("rdv: registro rechazado por {rdv}: {ack}");
        return Err(std::io::Error::other(format!("registro rechazado: {ack}")));
    }
    eprintln!(
        "rdv: registrado en {rdv} como {} (puerto {listen_port})",
        identity.node_id()
    );
    let mut next_hb = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(next_hb) => {
                write_line(&mut s, &hb_line()).await?;
                next_hb = tokio::time::Instant::now() + Duration::from_secs(30);
            }
            line = read_line_from(&mut s) => {
                let line = line?; // EOF/error → reconectar con backoff
                if t_of(&line).as_deref() == Some("invite") {
                    let v: serde_json::Value = serde_json::from_str(&line)
                        .map_err(|_| std::io::Error::other("invite invalido"))?;
                    let ticket = v.get("ticket").and_then(|t| t.as_str()).unwrap_or("");
                    if ticket.is_empty() { continue; }
                    let ticket = ticket.to_string();
                    let pipes2 = pipes.clone();
                    // El emparejamiento puede esperar hasta la vida del ticket:
                    // cada invite vive en SU tarea (no bloquea el keepalive).
                    tokio::spawn(async move {
                        if let Ok(stream) = establish_relay(rdv, &ticket).await {
                            let _ = pipes2.send((stream, rdv)).await;
                        }
                    });
                }
            }
        }
    }
}

// ============================ tests ============================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn start_server(limits: Limits) -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(serve(l, limits));
        addr
    }

    fn temp_id_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("anyremote-rdv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn registration_line_verifies_and_rejects_tampering() {
        let dir = temp_id_dir("reg");
        let id = Identity::load_or_generate(&dir).unwrap();
        let ts = now_unix();
        let line = register_line(&id, 21118, ts);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        let (i, pk, port, ts2, sig) = (
            v["id"].as_str().unwrap().to_string(),
            v["pk"].as_str().unwrap().to_string(),
            v["port"].as_u64().unwrap() as u16,
            v["ts"].as_u64().unwrap(),
            v["sig"].as_str().unwrap().to_string(),
        );
        // roundtrip íntegro
        assert!(verify_registration(&i, &pk, port, ts2, &sig, ts, 300).is_ok());
        // id ajeno → el id no deriva de la pk
        assert_eq!(
            verify_registration("AAAAAAAAAA", &pk, port, ts2, &sig, ts, 300),
            Err("el id no corresponde a la pk")
        );
        // firma manipulada
        assert_eq!(
            verify_registration(&i, &pk, 21119, ts2, &sig, ts, 300),
            Err("firma invalida")
        );
        // ts fuera de ventana (anti-replay por frescura)
        assert_eq!(
            verify_registration(&i, &pk, port, ts2, &sig, ts + 400, 300),
            Err("ts fuera de ventana")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn register_resolve_relay_flow() {
        let rdv = start_server(Limits::default()).await;
        let dir = temp_id_dir("flow");
        let id = Identity::load_or_generate(&dir).unwrap();
        let node = id.node_id().to_string();

        // host se registra
        let mut host = TcpStream::connect(rdv).await.unwrap();
        write_line(&mut host, &register_line(&id, 21118, now_unix()))
            .await
            .unwrap();
        let ack = read_line_from(&mut host).await.unwrap();
        assert_eq!(t_of(&ack).as_deref(), Some("registered"), "{ack}");

        // controlador resuelve → ip observada + puerto registrado + ticket
        let ans = resolve(rdv, &node).await.expect("resolve");
        assert_eq!(ans.addr, SocketAddr::from(([127, 0, 0, 1], 21118)));

        // el host recibe el invite con el MISMO ticket
        let inv = read_line_from(&mut host).await.unwrap();
        let iv: serde_json::Value = serde_json::from_str(&inv).unwrap();
        assert_eq!(iv["t"], "invite", "{inv}");
        assert_eq!(iv["from"], node);
        assert_eq!(iv["ticket"].as_str().unwrap(), ans.ticket);

        // ambos extremos se admiten en el relay → tubería de bytes ciega
        let (a, b) = tokio::join!(
            establish_relay(rdv, &ans.ticket),
            establish_relay(rdv, &ans.ticket)
        );
        let mut a = a.expect("relay a");
        let mut b = b.expect("relay b");
        a.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        b.write_all(b"pong").await.unwrap();
        a.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn resolve_unknown_id_is_offline() {
        let rdv = start_server(Limits::default()).await;
        let err = resolve(rdv, "AAAAAAAAAA").await.expect_err("offline");
        assert!(err.contains("offline"), "{err}");
    }

    #[tokio::test]
    async fn resolve_is_throttled_per_ip() {
        let rdv = start_server(Limits {
            resolve_per_min: 1,
            ..Limits::default()
        })
        .await;
        let err = resolve(rdv, "AAAAAAAAAA")
            .await
            .expect_err("1a ok? no: offline");
        // primera consulta consumida (offline o throttled); segunda → throttled
        let err2 = resolve(rdv, "AAAAAAAAAA").await.expect_err("2a throttled");
        let _ = err;
        assert!(err2.contains("throttled"), "{err2}");
    }

    #[tokio::test]
    async fn dead_registration_expires() {
        let rdv = start_server(Limits {
            peer_alive_secs: 0, // sin hb → caduca de inmediato
            ..Limits::default()
        })
        .await;
        let dir = temp_id_dir("dead");
        let id = Identity::load_or_generate(&dir).unwrap();
        let node = id.node_id().to_string();
        let mut host = TcpStream::connect(rdv).await.unwrap();
        write_line(&mut host, &register_line(&id, 21118, now_unix()))
            .await
            .unwrap();
        let _ = read_line_from(&mut host).await.unwrap(); // registered
        tokio::time::sleep(Duration::from_millis(20)).await;
        let err = resolve(rdv, &node).await.expect_err("caducado");
        assert!(err.contains("offline"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
