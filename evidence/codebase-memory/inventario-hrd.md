# Inventario de hallazgos H-RD (rustdesk @ 59f2953c) — Fase 3

Convención D-01 clean-room: rutas lógicas de símbolo como **referencia de patrón**; ningún código se copia a anyremote.
Fuente: índice codebase-memory (`search_graph`, `trace_path`) — invocaciones exactas en [reporte-indexacion.md](reporte-indexacion.md).

| ID | Patrón | Símbolo ancla (ruta lógica) | Componente anyremote | Ciclo |
|---|---|---|---|---|
| **H-RD-01** | **Handshake par client/server + mediador de rendezvous**: cliente `handshake()` invoca `secure_connection`, `decode_id_pk`, `dtls_fingerprint_bound`, `get_rs_pk`; servidor `identity_handshake()` con límites; registro vía `RendezvousMediator` | `src/client.rs:5840` · `src/server.rs:264` · `src/rendezvous_mediator.rs:260` | `core/session` + `core/transport` | S0, C1 |
| **H-RD-02** | **Identidad = ID + clave pública (PK)**: codificación `id_pk`, huella DTLS vinculada a la PK; el nodo **registra su PK** en el rendezvous (`register_pk`, con throttling `SENT_REGISTER_PK`) y registra peers (`register_peer`) | `src/common.rs (decode_id_pk/decode_id_pk_dtls/dtls_fingerprint_bound)` · `rendezvous_mediator.rs:1244/1291` | `core/identity` | C2 |
| **H-RD-03** | **Permisos de control granulares**: struct `ControlPermissionsID`; mensaje `Permission` (proto); `PermissionAuthorizeType` (UI) — cada capability (teclado, portapapeles, audio…) se autoriza por sesión | `src/server/connection.rs:7074` · `libs/base/protos/message.proto:629` · `flutter/lib/common.dart:3065` | `core/session` (política) + `app/` (prompt) | C2, C3 |
| **H-RD-04** | **Límites anti-abuso de conexiones no autenticadas**: `MAX_UNAUTHORIZED_CONNS`, `MAX_UNAUTHORIZED_CONNS_PER_ADDR`, `MAX_UNAUTHORIZED_MESSAGE`, `MAX_UNTRUSTED_PEER_ID_LEN` | `src/server/connection.rs:90/95/103` · `src/common.rs:3037` | `core/transport` | C1, C2 |
| **H-RD-05** | **Pipeline de vídeo captura→códec**: `Capturer` por backend (dxgi/x11/wayland/quartz), fábrica `Encoder`/`Decoder`, backends VPX y AOM (`EncodeFrame(s)`, `DecodeFrames`), formato negociable `CodecFormat` | `libs/scrap/src/…` (ver mapa-modulos §1–2) | `core/capture` + `core/codec` | C3 |
| **H-RD-06** | **Inyección de entrada multiplataforma**: librería `enigo` (MIT) + módulo `keyboard` (modos `KeyboardMode`, layout uinput) — separada del servidor como librería reutilizable | `libs/enigo/` (MIT) · `libs/base/src/keyboard.rs` · `src/server/uinput/` | `core/input` | C3 |
| **H-RD-07** | **Portapapeles remoto**: librería `clipboard` con integración RDP `cliprdr` (Windows wf_cliprdr + unix), backend propio por SO | `libs/clipboard/src/{windows,unix}` | `core/clipboard` | C4 |
| **H-RD-08** | **Transferencia de archivos en la conexión**: campo `file_transfer` en la conexión servidor + opciones `OPTION_ENABLE_FILE_TRANSFER`, `OPTION_FILE_TRANSFER_MAX_FILES`, `OPTION_ONE_WAY_FILE_TRANSFER` | `src/server/connection.rs:365/403` · `libs/base/src/config/keys.rs:40/101/144` | `core/filexfer` | C5 |
| **H-RD-09** | **Catálogo de claves de configuración tipado y clasificado por ámbito**: `KEYS_BUILDIN_SETTINGS`, `KEYS_LOCAL_SETTINGS`, `KEYS_DISPLAY_SETTINGS`, `KEYS_SETTINGS` + constantes `OPTION_*` | `libs/base/src/config/keys.rs:187–374` | `core/profiles` | C6 |
| **H-RD-10** | **Estadísticas parciales de calidad**: `CaptureStatsReporter`, `CapturePcmStats` (audio); **sin módulo de log de sesiones equivalente** → RF-05 queda mayormente como gap (diseño propio) | `src/server/audio_service/audio_capture_encoder.rs:69` | `core/telemetry` (parcial) | C7 |
| **H-RD-11** | **Descomposición en capas** (patrón de arquitectura): libs especializadas con fan-in alto (`scrap` core 259 in), orquestación en `server` (619 out), plataforma tras trait, IPC interno — el core no conoce UI | `get_architecture` layers/boundaries (mapa-modulos) | workspace `core/` completo | S0 |

## Vacíos (sin análogo abierto)

- **RF-05**: log de sesiones + diagnóstico de conexión accionable — sin módulo equivalente en RustDesk (solo estadísticas de audio).
- **Modelo de identificadores "por dirección"** (RF-03): RustDesk resuelve por ID vía rendezvous; conexión directa por IP/dirección existe pero sin directorio — cubierto por diseño propio en C2.
- **Descubrimiento/relay propio**: `hbbs_http` es la API del servidor oficial RustDesk — anyremote definirá su arquitectura de rendezvous en Fase 4 (S0 usa loopback).
