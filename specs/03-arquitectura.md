# Arquitectura — anyremote v0 (Fase 4)

| Campo | Valor |
|---|---|
| Versión | 0.1.0 (borrador para revisión arquitecto+legal+QA) |
| Fecha | 2026-10-26 |
| Baseline | AnyDesk 9.8.2 · rustdesk `59f2953c` · decisiones D-01/D-02 |

## 1. Decisiones de diseño (continuación de la serie D-xx)

- **D-03 Identidad y transporte**: cada nodo genera un par **ed25519** (identidad) + ephemeral x25519 por sesión; canal **TCP + TLS 1.3 (rustls)** con el handshake firmado por la identidad. El **ID de anyremote** = Base32 truncado del hash SHA-256 de la PK ed25519 (10–14 caracteres). **Persistente** en `<config>/identity.key` (v0.8.0: mismo equipo → mismo ID en todos los arranques; `session_id` correlacionable) y **trust TOFU** en `<config>/trusted.ids` (una sesión autenticada previa → la siguiente aparece `peer_trusted=true`; el anclaje de máquina sigue siendo el pin de certificado por destino). *IDs propietarios propios* (L-01: no se replica el modelo de IDs de AnyDesk; patrón H-RD-02 id_pk+DTLS).
- **D-04 Rendezvous**: trait `Rendezvous` con implementación `Loopback` (S0–C2: dos nodos se encuentran por IP:puerto directo) y futura implementación de servidor propio. Registro = ID + PK firmada (patrón `register_pk`, throttling H-RD-02).
- **D-05 Protocolo de sesión (ARSP)**: schema protobuf propio (`core/transport/proto/`): `Handshake`, `Auth`, `PermissionRequest/Grant`, `VideoFrame`, `AudioFrame`, `InputEvent`, `ClipboardOffer/Sync`, `FileChunk/Control`, `Stats`. Generado con `prost`. Mensajes propios, diseño inspirado en el patrón (sin copia de `message.proto` — H-RD-03 es referencia de modelo de permisos).
- **D-06 Códecs**: trait `VideoEncoder/VideoDecoder` con backend inicial **VP8/VP9** (bindings vpx) y opción AV1 (aom) — patrón H-RD-05 (fábrica `Encoder`/`Decoder` + `CodecFormat` negociable). Negociación en el Handshake.
- **D-07 Permisos**: cada sesión controlada expone `PermissionSet` granular (teclado, ratón, portapapeles, audio, archivos) con prompt UI (patrón H-RD-03) y defaults denegados.
- **D-08 Límites**: constantes de rate-limit de conexiones no autenticadas (patrón H-RD-04) en `core/transport`.
- **D-09 Configuración**: catálogo de claves tipadas por ámbito (`BUILDIN`, `LOCAL`, `DISPLAY`, `SESSION`) en `core/profiles` (patrón H-RD-09); almacenamiento TOML en `%APPDATA%\anyremote` (LOCAL) y `ProgramData\anyremote` (servicio).
- **D-10 Telemetry propia**: RF-05 sin análogo (gap aceptado) → `core/telemetry` con log de sesiones JSONL + métricas de enlace (RTT, FPS, pérdida) colectadas del pipeline (extiende parcial H-RD-10).

## 2. Mapa de capas (spec §8 → workspace)

```
anyremote/
├── core/                    # workspace Rust — patrón de capas H-RD-11
│   ├── transport/           # TCP+TLS, ARSP codec, rendezvous trait, límites D-08
│   ├── identity/            # ed25519, ID derivation, trust store (D-03)
│   ├── session/             # máquina de estados de sesión, permisos D-07
│   ├── capture/             # Capturer por SO (dxgi/wayland/x11/quartz) — patrón H-RD-05
│   ├── codec/               # traits encoder/decoder, backends vpx/aom — D-06
│   ├── input/               # inyección teclado/ratón (enigo-upstream como dependencia, MIT)
│   ├── clipboard/           # portapapeles por SO — patrón H-RD-07 (implementación propia)
│   ├── filexfer/            # canal FS con hash y límites — patrón H-RD-08
│   ├── profiles/            # catálogo de claves + persistencia TOML — D-09
│   └── telemetry/           # log de sesiones + stats de enlace — D-10
└── app/                     # Flutter desktop (Windows primero, RNF-03)
    └── UI: conexión (ID/dirección), perfiles, sesión (viewer+permisos), transferencias
```

Dependencias: `session` → `transport`+`identity`; `app` → IPC local con un binario `anyremoted` (daemon) vía named pipe (patrón IPC H-RD-11). La UI nunca toca crypto/red directamente.

## 3. Modelo de datos (spec §9)

- **Session**: `id: Uuid`, `nodes: {local: NodeId, remote: NodeId}`, `state: Enum(Handshaking|AwaitingPermission|Active|Closing|Closed)`, `metrics: LinkStats{rtt_ms, fps, loss_pct}`, `started_at/ended_at`.
- **ConnectionProfile**: `alias`, `target: Enum(Id|Addr{host,port})`, `quality: {codec偏好, max_fps}`, `permissions_default: PermissionSet`.
- **AuditEvent**: `ts`, `kind: Enum(SessionStarted|PermissionGranted|Denied|FileTransfer|Disconnected…)`, `session_id`, `detail`.
- **Finding** (proceso RE, ya vivo en vault/evidence): `id H-AD/H-RD`, `source`, `evidence_ref`, `design_impact`.

## 4. Seguridad (RNF-01)

1. Autenticación **mutua**: el controlador verifica la identidad del controlado y viceversa (firma ed25519 del transcript TLS) — sin credenciales compartidas embebidas.
2. **Trust store** local de dispositivos confiables (patrón `OPTION_ENABLE_TRUSTED_DEVICES`) con revocación.
3. **Denegación por defecto**: sin sesión activa sin concesión de permisos explícita (D-07); sesión con limitación de intentos (D-08).
4. Cifrado E2E obligatorio (TLS1.3); sin secretos en el binario (auditoría en C8).
5. Sin ejecución remota de shell fuera del alcance (RF-02/04/05 únicamente).

## 5. Plan de iteraciones y hitos

Ciclos **S0→C8** del [plan ODD](02-plan-implementacion.md) sin cambios; gates idénticos. Hitos:
- **M1 (tras C2)**: sesión autenticada E2E loopback con permisos — demo.
- **M2 (tras C3)**: vista remota con teclado/ratón + benchmark RNF-02.
- **M3 (tras C5)**: portapapeles + archivos.
- **M4 (tras C7)**: perfiles + telemetría; candidato a beta interna.
- **M5 (C8)**: endurecimiento + auditoría + cierre RNF.

## 6. Plan de pruebas

- **Unit**: por crate (`cargo test`); propiedades crypto en `identity` (roundtrip ID, rechazo de firma inválida).
- **Integración e2e** (gate por ciclo): dos procesos `anyremoted` en loopback ejecutan el slice del ciclo.
- **Seguridad** (gate C2/C8): rechazo de handshake sin firma, revocación de trust, límite de intentos, ausencia de secretos (grep de patrones + auditoría de binario).
- **Rendimiento** (gate C3/C8): benchmark captura→códec→decodificación; FPS/latencia medidos vs objetivo declarado (<100 ms loopback local como objetivo inicial, ajustable).
