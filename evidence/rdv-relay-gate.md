# Gate C-17 — WAN: rendezvous + relay (D-04, v0.12.0)

**Fecha**: 2026-11-03 · **Rama**: `feature/rdv-relay` · **Diseño**: `specs/02` §10 · **D-04**: `specs/03`

## Qué se implementó

| Pieza | Implementación |
|---|---|
| **Servidor** | **REUBICADO en el repo propio [brisaning/serveremote](https://github.com/brisaning/serveremote)** (paquete `serveremote`, binario `serveremote`, Dockerfile; público porque anyremote lo consume como **dependencia git con alias**: `anyremote-rendezvous = { package = "serveremote", git = … }`, commit fijado en el lock y **el código del daemon no cambió**). Allí viven el protocolo completo y el cliente (`RegisterSigner`: el `Identity` del cliente no cruza la frontera; `src/identity.rs` = copia verbatim de `core/identity` para verificar). Un **único puerto TCP** (default `:21120`): `register` (host persistente con `hb` cada30 s e `invite`), `resolve` y `relay` (admisión por ticket → tubería ciega). |
| Registro firmado (H-RD-02) | `sign_auth("anyremote-rdv-v1 register {id} {pk} {port} {ts}")` con la clave del propio id + **id derivado de la pk** (imposible reclamar un id ajeno) + ventana `ts` ±300 s + anti-replay `(id,ts)` + throttle por IP (H-RD-04:30 reg/min,60 res/min). El servidor combina la **IP observada** del socket con el puerto registrado → endpoint directo público. |
| Relay por ticket | `resolve` crea ticket aleatorio (24 B, `getrandom`) con60 s de vida y uso único → `invite` al host; ambos lados se admiten con `{"t":"relay","ticket"}` → `piped` → `copy_bidirectional` con tope de parejas (64) y duración (4 h). **E2E**: el TLS de la sesión se monta sobre la tubería — el relay solo transporta cifrado. |
| Cliente host | `run_host_client` (tarea persistente): register → hb30 s → por cada `invite` monta la tubería y la entrega al bucle de escucha como si fuese un `Accepted::Tcp` (bootstrap → TLS → `establish_common` — **cero duplicación** del flujo de sesión). Reconexión con backoff5 s; avisos por stderr (`rdv: registrado en …`). |
| Cliente connect (FASE A/B) | `resolve_id`: **LAN broadcast → servidor de encuentro → caché** (la caché después del rdv: una caché vieja nunca tapa una respuesta fresca). FASE A obtiene el canal: QUIC directo → **TCP directo con tope 8 s** (sin él, un SYN a un puerto no reenviado se cuelga20-40 s) → **relay** con evento `relay_fallback`; FASE B (`controller_session`) corre **una sola vez** sobre el ganador (`bye_rx` es de un solo uso). |
| Configuración | `--rdv host:puerto` (listen y connect) o fichero **`<config>/rdv`** (primera línea) — sin UI obligatoria. App: campo «servidor de encuentro» (vacío = LAN). Eventos: `rdv_bound`, `resolve_ok {source: broadcast\\|rdv\\|cache}`, `rdv_error`, `relay_fallback`. |
| Tests de la sesión | `daemon_rdv_e2e` (3): **directo vía rdv** (resolve + sesión) · **puerto registrado cerrado → `relay_fallback` → sesión por la tubería** · **fichero `rdv` + id offline → `rdv_error` + error accionable**. |

## Tests (puerto `rendezvous`:5 unit · daemon:3 e2e)

- `registration_line_verifies_and_rejects_tampering` (roundtrip · id ajeno ·
  firma manipulada · ts fuera de ventana).
- `register_resolve_relay_flow` (registro → resolve con IP observada →
  invite con el MISMO ticket → **dos `establish_relay` emparejados → bytes en
  ambas direcciones por la tubería**).
- `resolve_unknown_id_is_offline` · `resolve_is_throttled_per_ip` ·
  `dead_registration_expires` (hb vencido → offline).
- Daemon e2e: los3 arriba.

## Gates (2026-11-03)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **54 suites / 0 fallos · 144 tests** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` | limpio |
| `flutter test` | **9/9** (test de widgets actualizado:2 campos — destino + encuentro) |
| CI | el build de Linux y la **validación del Dockerfile** viven en el CI del repo **serveremote** (fmt/clippy/test + `docker build`); este repo solo lo consume como dependencia git (Cargo.lock fija el commit). |

## Guía de despliegue en el VPS (para la validación de campo)

**Vía recomendada — Docker** (el repo incluye un `Dockerfile` multi-etapa y su
CI valida el build):

```bash
git clone https://github.com/brisaning/serveremote.git
cd serveremote
docker build -t serveremote:latest .
docker run -d --name serveremote -p 21120:21120/tcp --restart unless-stopped serveremote:latest
docker logs -f serveremote
```

Mensaje de arranque esperado:
`anyremote-rdv-v1: serveremote escuchando en 0.0.0.0:21120 (parejas relay max 64)`.

- **Firewall**: solo **TCP 21120** (registro, resolución y relay comparten puerto).
- Estado en memoria (sin volúmenes): tras un reinicio los hosts se re-registran
  solos en ≤30 s.
- **Sin Docker**: `cargo build --release` en el VPS y la unidad systemd de
  ejemplo del README del repo (`serveremote --bind 0.0.0.0:21120`).

**Clientes** (los dos equipos): en la app, «servidor de encuentro» =
`<ip-del-vps>:21120` (o el fichero `rdv` en el directorio de configuración, o
`--rdv <ip>:21120` en CLI). El anfitrión debe escuchar con «Escuchar (red)» —
su registro debe verse como `rdv: registrado en …`.

**WAN sin reenvío en el router** = relay (funciona ya). Con reenvío manual de
TCP+UDP 21118 al anfitrión = conexión **directa** (el resolve devuelve la IP
pública observada; QUIC si el UDP va reenviado).

## Pendiente de validación de campo

- Desplegar en el VPS (guiado arriba) y probar: visora en **otra red** (p. ej.
  datos móviles) → `connect <ID>` → `resolve_ok source:rdv` → sesión con
  `relay_fallback` (o directa si hay reenvío) y tráfico fluyendo por el VPS
  (vigilar el ancho de banda del plan: 720p ≈2-6 Mbps por sesión).


---

## Validación de campo COMPLETA (2026-11-03 19:22–19:27, v0.12.0) — C-17 cerrado

**Setup**: servidor `serveremote` en el **VPS del usuario** (`51.222.81.81:21120`,
Docker), anfitrión y visora con `--rdv 51.222.81.81:21120`. El router NO tiene
reenvío de puertos ni UPnP → **todo debió pasar por relay** (y así fue).

| Evidencia | Sesión A (19:22, `--transport udp --rdv`) | Sesión B (19:25, `--rdv`, TCP) |
|---|---|---|
| Anfitrión | `rdv: registrado en 51.222.81.81:21120 como ONDH6IU4RU (puerto 21118)` + evento `rdv_bound` | idem tras reinicio del listener |
| Resolución | `resolve_ok` (3 s: broadcast LAN sin respuesta → **servidor de encuentro**) | `resolve_ok` |
| Directo → relay | `relay_fallback` (QUIC 3 s + TCP directo 8 s agotados → **pipeline del VPS**) | `relay_fallback` |
| **Auditoría (clave)** | `session_established` con **`peer_addr: "51.222.81.81:21120"`** y `transport:"tcp"` — **la sesión corrió por la tubería del VPS** (relay = TCP ciego, E2E: TLS entre pares) y `peer_trusted:true` (C2 sobre el relay) | ídem |
| Salud | 59 frames / 1,31 MB · **86 heartbeats rtt 454–491 ms** · `frames_dropped: 0` · **323 inyecciones** · `bye` limpio | 34 frames / 419 KB · **87 heartbeats rtt 461 ms (pico 1429 ms)** · `frames_dropped: 0` · 103 inyecciones · `bye` limpio |
| **C-13 sobre el relay** | cascada completa: `red_bitrate` 1382k→691k→345k→**200k** + `red_escala` 1280×720→**924×520** y recuperaciones — el enlace a454 ms de RTT se degradó y **la sesión NO murió** (cierre `bye`, no `connection_gone`) | misma cascada; la sesión sobrevivió un pico de **1,4 s de RTT** |

**Conclusión**: **C-17 validado en campo de punta a punta** — registro en el
VPS por internet, resolución por ID a través del rendezvous, caída al relay
cuando lo directo no llega (router sin reenvío), sesiones E2E sobre la tubería
ciegra con C2 verificado (`peer_trusted`), vídeo H.264 fluyendo con la
adaptación C-13 actuando y cierres limpios. Los tres modos de connect (`udp`,
`tcp`, con/sin `--rdv`) probados. **WAN cerrado.**
