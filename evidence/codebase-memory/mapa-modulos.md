# Mapa de módulos — rustdesk (entregable Fase 1)

Símbolos anclados vía `search_graph` sobre el índice `rustdesk` (commit `59f2953c…`).
**Uso permitido (D-01 clean-room)**: referencia de patrón por ruta lógica de símbolo — nunca copia de código.

## 1. Subsistema de captura de pantalla — `libs/scrap/src/`

Estructura `Capturer` por plataforma:

| Plataforma | Símbolo | Archivo |
|---|---|---|
| Windows DXGI | `Capturer` (Struct) | `libs/scrap/src/dxgi/mod.rs:53` |
| Linux X11 | `Capturer` (Struct) | `libs/scrap/src/x11.rs` · `x11/capturer.rs:6` |
| Linux Wayland | `Capturer` (Struct) | `libs/scrap/src/wayland.rs` |
| Linux (enum unificador) | `Capturer` (Enum) | `libs/scrap/src/linux.rs:11` |
| macOS Quartz | `Capturer` (Struct) | `libs/scrap/src/quartz.rs:6` |
| Cámara | `CameraCapturer` (Struct) | `libs/scrap/src/camera.rs:192` |

Patrón clave: **enum unificador por SO** con capturer propio por backend → candidato para el crate `core/capture`.

## 2. Códecs y transporte — `libs/scrap/src/common/`

| Símbolo | Archivo | Rol |
|---|---|---|
| `Encoder` (Struct, 17 fan-out) | `libs/scrap/src/common/codec.rs:85` | Fábrica/selector de codificadores |
| `Decoder` (Struct, 6 fan-in) | `libs/scrap/src/common/codec.rs:103` | Decodificación |
| `CodecFormat` (Enum, 31 fan-in — hotspot) | `libs/scrap/src/common/mod.rs:301` | Formatos negociables |
| `CodecName` (Enum) | `libs/scrap/src/common/mod.rs:290` | Nombres de códec |
| `AomEncoder` / `AomDecoder` / `AomEncoderConfig` | `libs/scrap/src/common/aom.rs:51/442/44` | Backends AV1 |

Candidato para `core/codec`. La negociación de formato enlaza con el handshake (§3).

## 3. Sesión: handshake, identidad y transporte — `src/`

| Símbolo | Archivo | Rol |
|---|---|---|
| `handshake()` (Function, 9 fan-out) | `src/client.rs:5840` | Handshake del lado cliente |
| `identity_handshake()` (Function, 15 fan-out) | `src/server.rs:264` | Handshake del lado servidor (identidad) |
| `RendezvousMediator` (Struct) | `src/rendezvous_mediator.rs:260` | Mediador con servidor de encuentro |
| `get_rendezvous_server()` (Function) | `src/common.rs:758` | Resolución del rendezvous |
| `OPTION_CUSTOM_RENDEZVOUS_SERVER` | `libs/base/src/config/keys.rs:76` | Endpoint configurable |
| `test_unauthorized_limit_bounds_live_handshakes` (test) | `src/server/connection.rs:7224` | Límite de intentos no autorizados |

Patrón clave: **par client/server handshake + mediador de rendezvous** → candidatos para `core/session` + `core/transport` (ciclos S0–C2).

## 4. Transferencia de archivos y portapapeles

| Símbolo | Archivo | Rol |
|---|---|---|
| `file_transfer` / `enable_file_transfer` (Fields) | `src/server/connection.rs:365/403` | Canal FS en la conexión |
| `OPTION_ENABLE_FILE_TRANSFER` (4 fan-in) | `libs/base/src/config/keys.rs:40` | Habilitación por opción |
| `OPTION_FILE_TRANSFER_MAX_FILES` / `OPTION_ONE_WAY_FILE_TRANSFER` | `libs/base/src/config/keys.rs:101/144` | Límites y dirección |
| wf_cliprdr (Windows clipboard) | `libs/clipboard/src/windows/wf_cliprdr.c` | Integración RDP cliprdr |
| `unix/mod.rs`, `cliprdr.h` | `libs/clipboard/src/` | Portapapeles multiplataforma |

Candidatos para `core/filexfer` + `core/clipboard` (ciclos C4–C5).

## 5. Configuración y perfiles — `libs/base/src/config/`

Catálogo de claves tipadas (patrón: constantes de clave + clasificación por ámbito):

| Símbolo | Líneas | Rol |
|---|---|---|
| `KEYS_BUILDIN_SETTINGS` | 337–374 | Opciones embebidas (build) |
| `KEYS_DISPLAY_SETTINGS` | 187–217 | Perfiles de pantalla |
| `KEYS_LOCAL_SETTINGS` | 219–270 | Ajustes locales |
| `KEYS_SETTINGS` | 272–334 | Ajustes de sesión/perfil |
| `OPTION_ENABLE_KEYBOARD` etc. | 33+ | Catálogo `OPTION_*` (34 claves localizadas) |

Candidato para `core/profiles` (ciclo C6).

## Estructura global observada (get_architecture)

- **Capas**: `scrap` (core, 259 fan-in) · `client` (core, 196 in/167 out) · `platform` · `ipc` · `base` · `common` · `hbbs_http` · `server` (619 fan-out — orquestación) · `keyboard` (entry) · `clipboard`/`enigo` (internal, libs).
- **Fronteras dominantes**: `server→scrap` (259 llamadas), `client→server` (120), `server→platform` (99).
- **Puntos de entrada UI**: `flutter/lib/main.dart` (Dart), binarios vía `build.rs`/`src/`.
- **Hotspots**: `CodecFormat.to_string` (215 fan-in), `translate` Flutter (310), `Service.ok` (240).

## Vacíos iniciales (para Fase 3)

- Sin análogo claro localizado aún para: modelo de **identificadores** de AnyDesk (se extraerá en Fase 2 antes de buscar equivalente).
- El relay/`hbbs_http` es API HTTP del servidor oficial — anyremote decidirá arquitectura de descubrimiento en Fase 4.
