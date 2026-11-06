# Gate C-24 — encoder: reconstrucciones coalescadas + reenvío rápido (v0.12.7)

**Fecha**: 2026-11-05 · **Rama**: `feature/encoder-warmup` · Disparador: **reporte de campo** — «conexión desde otra máquina pero no muestra la pantalla»

## Diagnóstico (logs de campo + experimento en vivo)

Las2 sesiones de ayer/noche (relay WAN, rtt ~500 ms) en esta máquina como
anfitrión (v0.12.6) tuvieron **`frames_sent:0` / `frames_dropped:0`** con la
adaptación **cascando** cada ~3 s (`red_bitrate` 3M→200k, `red_escala`,
`presion_bitrate`) — pero **con eventos `calidad adaptada` fluyendo** (el
hilo de vídeo vivía). Un experimento en vivo (controller CLI en loopback,
mismo binario) produjo **`frames_sent:3712` / 43 MB** → el pipeline y el
fix GDI de C-23 **funcionan**; el fallo solo ocurría en su combinación.

**Mecanismo (raíz)**: `MfH264Encoder::set_target_bitrate` marca
`transform = None` (reconstrucción diferida, fix v0.9.1) y **cada
reconstrucción reinicia el calentamiento del MFT** (n inputs antes del
primer slice: los primeros NALs pueden ser solo NEED_MORE_INPUT). Con:

- **cascada de adaptación** (WAN rtt≥200 ms → `set_target_bitrate` cada
  ~3 s) → reconstrucción cada ~3 s, y
- **pantalla estática** (anfitrión sin nadie; reenvío del último frame a
  1 Hz = ~3 inputs por ventana),

el MFT **nunca acumulaba inputs suficientes para emitir** → `enc.data`
vacío en TODAS las iteraciones (`try_send` jamás llamado — por eso
`frames_dropped:0`) → **cero bytes de vídeo durante toda la sesión** →
visor negro. Con pantalla cambiante (o sin cascada, como mi test en
loopback: rtt1 ms →0 reconstrucciones) el calentamiento completa y todo
fluye ✓. La red de seguridad de C-23 (frame inicial GDI) también se
quedaba sin emitir por lo mismo.

## Qué se implementó

| Fix | Detalle |
|---|---|
| **1. Coalescado de reconstrucciones** (`mf_backend.rs`) | `set_target_bitrate` actualiza `self.bitrate` SIEMPRE pero solo marca `transform = None` si desde la última reconstrucción pasaron **≥6 s** (`REBUILD_COALESCE`; `ensure()` registra `last_rebuild`). La próxima reconstrucción (por bitrate o por cambio de dims — esos siguen siendo inmediatos) aplica el **último bitrate pendiente**. La cascada converge igual (sus pasos de3 s solo actualizan el objetivo). |
| **2. Reenvío acelerado mientras calienta** (`video.rs`) | `reemit_gap(empty_run)`: con salidas de códec **vacías seguidas** se reemite el último frame cada **200 ms** en vez de1 s → el MFT recibe inputs a5 Hz y emita su primer slice aunque nadie toque el anfitrión; al emitir, vuelve a1 s. |
| **3. Diagnóstico GDI** (`video.rs`) | `cold_start_gdi`: el frame inicial de C-23 ahora loguea **una vez por sesión** en stderr → `[video] frame inicial GDI: WxH` o `… no disponible: <error>` (visible en `ui.log` como `[listener-err]`). |

## Tests (+2)

- `bitrate_rebuilds_are_coalesced` (codec): tras `ensure`, un cambio de
  bitrate **NO** reconstruye dentro de la ventana (y el pendiente se
  guarda); con la ventana vencida SÍ; y la siguiente codificación
  reconstruye con el **último** bitrate pendiente.
- `reemit_gap_accelerates_while_codec_warms_up` (daemon):1 s en frío
  normal, <300 ms con salidas vacías.

## Gates (2026-11-05)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **22/22** |

## Pendiente de validación de campo

- Repetir el escenario fallido: anfitrión **sin nadie delante** (pantalla
  estática) + visora por **relay WAN** → el visor debe mostrar la pantalla
  en unos segundos (GDI + calentamiento acelerado) y la cascada de
  adaptación debe converger **sin** dejar el vídeo en0.
- El `ui.log` del anfitrión debe mostrar `[video] frame inicial GDI: WxH`.
