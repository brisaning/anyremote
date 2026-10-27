# Informe de rendimiento (C-3) — reducción de recursos y ancho de banda · v0.7.1

| Campo | Valor |
|---|---|
| Fecha | 2026-10-27 |
| Rama | `feature/perf-quickwins` (D-11) · tag `perf-v0.7.1` |
| Método | Auditoría del código de referencia (RustDesk) + del nuestro, con rutas; quick wins implementados y medidos |
| Gates | Rust **41 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` **4/4** |

## 1) Qué hace la referencia (RustDesk, solo patrones — clean-room D-01)

Fuente local: `C:\proyectos\fixed\rustdesk` (v1.5.1). Submódulo `libs/hbb_common` no
inicializado (QUIC/KCP citados solo por manifiesto).

| Técnica | RustDesk | Evidencia |
|---|---|---|
| Captura DXGI con fallback GDI/HDR | ✅ | `libs/scrap/src/dxgi/mod.rs:111-131,199-230` |
| **Omitir frames sin cambios** (`AccumulatedFrames==0 \|\| LastPresentTime==0`) | ✅ | `libs/scrap/src/dxgi/mod.rs:604-606` |
| Deduplicación de frame idéntico | ✅ | `libs/scrap/src/common/mod.rs:122-131` |
| Dirty regions / move rects | ❌ (solo comentario aspiracional) | `libs/scrap/src/dxgi/mod.rs:139-140` |
| Cursor fuera del frame (mensaje aparte) | ✅ | `common/mod.rs:285-288`, `protos/message.proto:312-323` |
| Códecs VP8/VP9/AV1/H.264/H.265 | ✅ | `common/mod.rs:290-308` |
| **Aceleración por hardware** (VRAM sin readback → ffmpeg RAM → software) con fallback a VP9 | ✅ | `common/codec.rs:51-58,134-168`, `video_service.rs:1067-1175,673-682` |
| Selección de códec según peer/recursos (VP8 si RAM ≤4 GB) | ✅ | `common/codec.rs:171-330` |
| **Tope/calidad de bitrate** (`Low/Balanced/Best` + custom) | ✅ | `common/codec.rs:898-960`, `video_qos.rs:435-456` |
| **Adaptación por RED** (umbral 150 ms, bitrate primero, fps con suelo, ABR) | ✅ | `src/server/video_qos.rs:1-45,731-795` |
| Transporte TCP + UDP/KCP (+QUIC en manifiesto) | ✅ | `src/kcp_stream.rs`, `Cargo.toml:90` |
| **Audio: gate de silencio** (~4-8 s → deja de codificar/enviar) | ✅ | `src/server/audio_service.rs:628-658` |
| Audio Opus 10 ms/100 pps (bitrate por defecto) | ✅ | `audio_service.rs:19-20,213` |
| *Static refresh* (re-encode a cadencia reducida en pantallas estáticas) | ✅ | `video_service/static_refresh.rs:15-68` |
| Cambio de resolución del host en sesión | ✅ | `connection.rs:4001` |

## 2) Nuestro estado (brechas detectadas)

| Área | Estado antes de C-3 | Ruta |
|---|---|---|
| Frames sin cambios | ❌ DXGI ignoraba `AccumulatedFrames`/`LastPresentTime` → **cada movimiento de ratón sobre pantalla estática generaba un frame VP9 completo** | `core/capture/src/dxgi.rs` |
| Audio silencio | ❌ Opus a ~52 kbps constantes, sin DTX ni gate (aunque el loopback en silencio total no entrega paquetes: 0 bytes) | `core/audio/src/lib.rs`, `core/daemon/src/lib.rs` |
| Tope de bitrate | ❌ sin flag; el adaptativo podía rampar a `bps*4` (1080p → 24.8 Mbps) | `core/daemon/src/video.rs:117` |
| Tuning libvpx | ⚠️ `fps` fijo 30 (con sesión a 15 → VBV mal temporizado), sin `threads`, sin `tune_content=Screen` | `core/codec/src/vpx_backend.rs:108-125` |
| Adaptación | ⚠️ solo por **tiempo de codificación** (no usa RTT/ancho de banda) | `core/daemon/src/video.rs:98-181` |
| Captura/encode en el worker tokio | ⚠️ DXGI+VP9 dentro de `tokio::spawn` (competía con latidos/input) | `core/daemon/src/video.rs:205-286` |
| Códecs | Solo VP9 (+Deflate/Raw); el wrapper `shiguredo_libvpx` **ya soporta VP8** (`Vp8Config.static_threshold`) sin exponer | `core/codec/src/lib.rs:13-17,207` |
| Transporte | TLS/TCP puro (head-of-line: un frame grande retrasa input/latidos) | `core/transport` |
| Métricas | ✅ `SessionReport{frames_sent,bytes_sent,rtt,audio_*}` sin lazo hacia la adaptación | `core/telemetry` |

## 3) Quick wins implementados en C-3

| # | Quick win | Cambio | Ahorro esperado |
|---|---|---|---|
| **QW-1** | **No codificar frames sin cambios** | `dxgi.rs`: tras `AcquireNextFrame`, `frame_unchanged(AccumulatedFrames, LastPresentTime)` → `ReleaseFrame` + `Timeout` (patrón RustDesk); el **primer frame sí se procesa** (la vista necesita la imagen inicial) | Ratón sobre pantalla estática → **0 bytes** (antes: frame completo por movimiento) |
| **QW-2** | **Audio: gate de silencio + DTX + bitrate** | `OpusCodec`: `set_bitrate(48 kbps)`, `set_dtx(true)`, `set_complexity(5)`; daemon: `silence_gate()` (RMS < 0.002 sostenido 2 s → no codifica ni envía); controlador: `audio_stats` con rms 0 si deja de llegar audio | **Medido**: micrófono en silencio → `audio_sent=100` (2 s de rampa) vs ~250 sin gate (**-60 %**); loopback en silencio total → **0 bytes** |
| **QW-3** | **Tope de ancho de banda** | `--max-bitrate <kbps>` → `DaemonConfig.video_max_bps` → `AdaptState::new(size, max_bps)` (inicial y techo acotados) + la app pasa el tope según calidad (**720p→3 Mbps · 1080p→6 Mbps · Nativa→8 Mbps**) | Evita la rampa a 24 Mbps; techo predecible en redes congestionadas |
| **QW-4** | **Tuning de libvpx** | `fps_numerator/denominator` = fps real (antes 30 fijo), `threads` = CPU (tope 4), `row_mt`, `tune_content: ContentType::Screen` | Mejor calidad/latencia a mismo bitrate; menos tiempo de encode (la adaptación deja de bajar escala prematuramente) |
| QW-2b | `audio_stats` idle | Si el anfitrión deja de enviar, el controlador emite `audio_stats` con rms 0 → el visor muestra **`audio: sin señal`** | Diagnóstico de campo |

Tests nuevos: `frame_unchanged_detects_cursor_only_and_idle_updates` (capture),
`max_bitrate_caps_initial_and_ceiling` (adaptativo), `silence_gate_skips_only_sustained_silence`
(daemon), `opus_is_tuned_for_bandwidth` (codec) + regresión total **41 suites / 0 fallos**.

## 4) Tu lista → clasificación

| Sugerencia | Clase | Estado en C-3 / nota |
|---|---|---|
| Reducir resolución | app | ✅ selector 720p/1080p/Nativa (v0.6.1) + tope por calidad (QW-3) |
| Desactivar efectos visuales del host | consejo | 📄 documentado (abajo) |
| Profundidad de color 32→16/8 | app (L) | ❌ no: VP9 ya submuestrea 4:2:0 (bajo impacto) |
| Deshabilitar audio si no se usa | app | ✅ `--audio off` por defecto + QW-2 (silencio ≈ 0) |
| Bloquear portapapeles/periféricos | app | ✅ `--clipboard off` existe (el default sigue `win`; sin impresoras/USB en anyremote) |
| UDP en vez de TCP | app (L) | ❌ pendiente (rediseño de seguridad: DTLS/Noise + negociación) — propuesta §5 |
| Aceleración por hardware | app (L) | ❌ pendiente — propuesta §5 |
| Tope máximo de tasa de bits | app (S) | ✅ **QW-3** |
| Cerrar apps en segundo plano | consejo | 📄 (baja la carga de encode) |
| Inicio automático | no aplica/consejo | 📄 |
| Clientes ligeros | consejo | 📄 usar 720p y `--audio off` si no hace falta |

**Consejos de campo para el usuario** (añadir al README): fondo de escritorio
sólido, desactivar transparencias/animaciones (Aero), cerrar navegadores y
procesos pesados en el anfitrión, y elegir 720p en lugar de Nativa si la red
va justa.

## 5) Propuestas a medio/largo plazo (con riesgos)

| Propuesta | Esfuerzo/impacto | Riesgos / decisión necesaria |
|---|---|---|
| **Aceleración por hardware** (NVENC/AMF/QSV/Media Foundation) | L / Alto | Cambia el contrato de `VideoEncoder` (entrada GPU): hoy hay readback obligatorio (`dxgi.rs:187-201`). Mantener **fallback a libvpx** obligatorio; clean-room desde APIs públicas (no copiar el fork AGPL `hwcodec`) |
| **Adaptación por red** (RTT/pérdida, no solo `encode_ms`) | M / Alto | Los latidos ya dan RTT; añadir señal de enlace a `AdaptState` con política de umbrales estables (patrón `video_qos.rs`: bitrate primero, fps con suelo) |
| **UDP (KCP) / QUIC** | L / Alto | Rediseño de seguridad (hoy el binding de canal ancla la auth sobre TLS/TCP): DTLS/Noise + renegociación; NAT/firewall (mantener TCP como fallback) |
| **Cursor separado (overlay)** | M / Alto | Ahorra re-encode por movimiento de ratón y hace visible el cursor remoto (hoy el cursor **nunca va en el frame** de DXGI) — requiere mensaje de protocolo + render en la app |
| VP8 como opción de CPU barata | M / Medio | El wrapper ya lo soporta (`Vp8Config.static_threshold`): exponer `--codec vp8` (cambia proto/negociación) |
| Captura/encode fuera del worker tokio + reutilizar buffers | M / Medio | `spawn_blocking`/hilo dedicado + buffers pre-capacidad (hoy: `vec![]` nuevo por frame en `scale_frame`/`bgra_to_i420`) |
| libyuv/SIMD en vez de `bgra_to_i420` scalar | M / Medio | Dependencia nativa de build (patrón `libs/scrap/build.rs:247`) o crate `yuv` |
| Dirty regions / codificación por zonas | L / Bajo | ❌ **ni RustDesk lo hace** (solo un comentario): pospuesto |

## 6) Medidas de este ciclo (este equipo)

| Medición | Resultado |
|---|---|
| Micrófono en silencio, sesión 5 s (`--audio on --audio-source mic`) | `audio_sent=100` (gate a los 2 s) vs ~250 sin gate → **-60 %** · 7.4 KB totales |
| Loopback en silencio total | **0 bytes** (WASAPI no entrega paquetes sin reproducción) |
| Pantalla con Chrome activo (no estática), 640x360@10 | `frames_sent=39/5.3 s` — no concluyente: la pantalla cambiaba (video/animación); **verificación de campo**: con el anfitrión estático debe verse `frames_sent ≈ 1` por sesión |
| Gates | Rust **41 suites / 0 fallos** (con los tests nuevos) · `flutter analyze` limpio · `flutter test` 4/4 |

## Limitaciones / pendientes de este informe

- QW-1 se verificó por condición unitaria + patrón de referencia; la medición
  final en pantalla estática queda como **prueba de campo** (Chrome estaba
  reproduciendo contenido en el equipo de desarrollo).
- La adaptación sigue siendo solo por tiempo de codificación (§5).
- El encode sigue en el worker tokio (§5, QW-5 pospuesto por esfuerzo M).
