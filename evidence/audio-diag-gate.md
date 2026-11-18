# Gate C-40 — diagnóstico de audio (micrófono) y de la adaptación (v0.13.12)

**Fecha**: 2026-11-18 · Rama: `feature/c40-audio-diag` · Origen: prueba de campo
con **micrófono activo** (12:48, Julie ← Brian) y con Juank/Juank2.

## Lo que se vio en los logs

### Vídeo (sano)

| Sesión | `encode_ms` | `escala_ms` | tasa real | auto-reset |
|---|---|---|---|---|
| 12:18 (1080p) | 8,8–9,7 | 13,9–16,2 | 1,3–2,7 Mbps | 0 |
| 12:48 (1080p + mic) | 7,9–11,0 | 12,9–15,7 | 0,6–4,3 Mbps | 0 |

El escalado paralelo (C-36) se confirma en campo: **~15 ms** (antes 32–48).
`bytes_sent` 21,4 MB y 14,7 MB por sesión (antes 2,9 MB con 265 kbps).

### Audio: no se podía demostrar

- El anfitrión anunció `audio compartido desde mic (48000 Hz)` y `mic activo`.
- El cliente mostró **`audio sin señal (rms 0): ¿está reproduciendo sonido el
  anfitrión?`** aunque la fuente era el **micrófono**, no el audio del sistema.
- El cierre de sesión **no incluía contadores de audio** (`audio_sent`/
  `audio_recv` existen en el `SessionReport` pero no se imprimían).

⇒ Con esos dos huecos, «silencio esperado» y «el micrófono no llega» son
indistinguibles. Eso es lo que se arregla.

### Adaptación por red en LAN

En las dos sesiones hubo `red_bitrate` (15 → 7,5 → 3,75 Mbps) con
`rtt_min = 3–4 ms`. Sin el RTT en el evento no se puede saber si fue congestión
real o un pico de cola (hipótesis: el cliente tarda en responder por carga de
decode/render). Ahora el evento lo lleva.

## Cambios

| Pieza | Detalle |
|---|---|
| `main.rs` | El cierre de sesión añade **`audio_sent=` y `audio_recv=`** (bytes de audio en cada sentido). |
| `video.rs` | `video_adapt` incluye **`rtt`**: el RTT que disparó la decisión de red. |
| `main.dart` | Guarda `_audioSource` de `audio_start`; con `mic` la cabecera muestra **`mic ✓` / `mic: sin señal`** y el aviso dice «el anfitrión no está captando voz» (antes hablaba de «sonido del sistema»). El color verde ya no depende de la etiqueta exacta. |

## Gates

| Gate | Resultado |
|---|---|
| `cargo test --workspace` | **159 / 0** |
| `cargo clippy --workspace --all-targets` | **0 avisos** |
| `cargo fmt --all -- --check` | **0 diffs** |
| `flutter analyze` / `flutter test` | limpio · **29 / 29** |

## Cómo validar el micrófono en la próxima sesión

1. Anfitrión con **«Micrófono»** activado; cliente conectado con `--audio on`.
2. Hablar cerca del micro del anfitrión: la cabecera debe pasar a **`mic ✓`**.
3. Al cerrar, el log del anfitrión debe mostrar **`audio_sent` > 0** y el del
   cliente **`audio_recv` > 0** (mismo orden de magnitud).
4. Si `audio_sent = 0`: el problema es la captura (dispositivo/permiso). Si
   `audio_sent > 0` y `audio_recv = 0`: transporte/permiso del cliente.

## Pendientes que siguen

- Revalidación **WAN** (~10 Mbps) con el nuevo `rtt` en el evento.
- Decisión **`fps render` / `Texture`**.
- Juank/Juank2 se probaron «fluidos» pero sus logs viven en esos equipos: para
  cerrar el caso conviene traer su `ui.log` (mismo formato con `kbps`/`bpp`).
