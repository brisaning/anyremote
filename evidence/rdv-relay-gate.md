# Gate C-17 — WAN: rendezvous + relay (D-04, v0.12.0)

**Fecha**: 2026-11-03 · **Rama**: `feature/rdv-relay` · **Diseño**: `specs/02` §10 · **D-04**: `specs/03`

## Qué se implementó

| Pieza | Implementación |
|---|---|
| **Servidor** `anyremote-rdv` | Crate nuevo `anyremote-rendezvous` (portable: tokio/serde/identity — sin deps Windows). Un **único puerto TCP** (default `:21120`) para las tres funciones del protocolo `anyremote-rdv-v1`: `register` (host persistente con `hb` cada30 s e `invite`), `resolve` (controlador) y `relay` (admisión por ticket → tubería ciega). |
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
| CI | job nuevo **`rdv-linux`**: construye `anyremote-rdv` para **Linux** y lo sube como artefacto (`anyremote-rdv-linux-x86_64`) — el binario del VPS sale de cada run de CI. |

## Guía de despliegue en el VPS (para la validación de campo)

1. **Binario**: descargar el artefacto `anyremote-rdv-linux-x86_64` del job
   `rdv-linux` del último run de CI (o `cargo build --release -p
   anyremote-rendezvous --bin anyremote-rdv` en cualquier máquina con Rust).
2. **Puertos**: abrir **TCP 21120** (control + relay en el mismo puerto).
3. **systemd** (`/etc/systemd/system/anyremote-rdv.service`):

```ini
[Unit]
Description=anyremote rendezvous + relay
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/opt/anyremote/anyremote-rdv --bind 0.0.0.0:21120
Restart=always
RestartSec=5
# El servidor NO guarda estado (los hosts re-registran cada 30 s → se
# autosana tras cada reinicio); sin usuario dedicado solo si lo prefieres:
# User=anyremote

[Install]
WantedBy=multi-user.target
```

```bash
sudo install -m755 anyremote-rdv /opt/anyremote/
sudo systemctl daemon-reload && sudo systemctl enable --now anyremote-rdv
sudo firewall-cmd --add-port=21120/tcp --permanent && sudo firewall-cmd --reload   # (o ufw allow21120/tcp)
```

4. **Clientes** (los dos equipos): en la app, «servidor de encuentro» =
   `<ip-del-vps>:21120` (o el fichero `rdv` en el directorio de configuración
   con esa línea). El anfitrión debe escuchar con `Escuchar (red)` — registra
   su endpoint y recibe los invites.
5. **WAN sin reenvío en el router** = relay (funciona ya). Con reenvío manual
   de TCP+UDP 21118 al anfitrión = conexión **directa** (el resolve devuelve la
   IP pública observada; QUIC si el UDP va reenviado).

## Pendiente de validación de campo

- Desplegar en el VPS (guiado arriba) y probar: visora en **otra red** (p. ej.
  datos móviles) → `connect <ID>` → `resolve_ok source:rdv` → sesión con
  `relay_fallback` (o directa si hay reenvío) y tráfico fluyendo por el VPS
  (vigilar el ancho de banda del plan: 720p ≈2-6 Mbps por sesión).
