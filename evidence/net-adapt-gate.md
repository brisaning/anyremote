# Gate C-13 — adaptación por red (RTT) · v0.8.7

| Campo | Valor |
|---|---|
| Fecha | 2026-10-29 |
| Rama | `feature/net-adapt` (D-11) · tag `net-adapt-v0.8.7` |
| Origen | **1ª propuesta de `specs/02` §7** (backlog aprobado: red → hardware → UDP/QUIC) |
| Gates | Rust **47 suites / 0 fallos** · **clippy 0 avisos / 0 errores** · **fmt 0 diffs** · `flutter analyze` limpio · `flutter test` **9/9** |

## Problema

El adaptador de calidad solo miraba **tiempo de encode** (CPU): una red
saturada (Wi‑Fi de campo: RTT **57–782 ms** vs LAN **<10 ms**) inflaba la
cola de envío sin que el codificador lo notara. Los latidos ya medían RTT,
pero **en el controlador** — y quien debe actuar es **el anfitrión** (dueño
del encoder).

## Diseño

1. **Pista de red `NetHint {rtt_ms}` (campo 26)**: el controlador la envía
   **1/s**, justo al computar el RTT de los latidos (`rtt_ms` real o
   `net_rtt_override` en tests — en loopback el RTT es ~0 y la política no se
   podría exercitar). Manejo de errores idéntico al eco de latidos
   (`conn_gone` → cierre limpio).
2. **Anfitrión**: la recibe en la sesión → `report.record_heartbeat()` (los
   informes del host pasan de `heartbeats=0 · rtt_min=None` a **reales**) y
   la publica por un canal **`watch`** al hilo de vídeo (último valor, sin
   encolar; sin vídeo la pista se descarta sin ruido).
3. **Política `AdaptState::observe_net(rtt)`** (`evidence/net-adapt-gate.md`):

   | Condición | Acción |
   |---|---|
   | RTT ≥ **200 ms** en **3 observaciones** (1/s) | baja bitrate **50 %** (`red_bitrate`) o escala **85 %** si ya está en el suelo (`red_escala`) + **cooldown 20 s** |
   | RTT ≤ **100 ms** en **10 observaciones** y cooldown agotado | recupera escala/bitrate **+50 %** con tope QW-3 (`red_recupera_*`) |
   | Zona neutral 100–200 ms | no incrementa; **no borra** la evidencia de alta (solo el RTT sano la limpia) |
   | `net_congested` = true | **bloquea las recuperaciones de CPU** (`observe`) — sin esto las dos políticas se pelearían → oscilación. La presión de encode sigue permitida (bajar siempre es válido) |

   Se mantiene la escalera ya probada (bitrate → escala); el fps no se toca.
4. **Fix en la misma escalera**: restaurar escala **respetaba mal el tope**
   `--max-bitrate` (el bitrate «saltaba» el tope en `recupera_escala`) →
   `.min(max_bps)` en ambas recuperaciones de escala (regresión propia).

## Tests

| Suite | Qué cubre |
|---|---|
| `net_adaptation_confirms_then_lowers_and_recovers_after_cooldown` | 1–2 muestras altas → nada; 3ª → `red_bitrate` (−50 %); zona neutral no borra evidencia; sano sí limpia; **cooldown** bloquea15 sanas; tras cooldown → `red_recupera_bitrate` y `net_congested=false` |
| `net_congestion_at_floor_scales_down_and_blocks_cpu_recovery` | en el suelo → `red_escala`; **CPU bloqueada** (60 `observe` con holgura → todas `None`); la recuperación de red (`red_recupera_escala`) desbloquea y la CPU vuelve, **respetando el tope** |
| `scale_recovery_respects_the_bitrate_cap` | regresión del fix QW-3 (recuperar escala no pasa del tope) |
| e2e `controller_rtt_hint_makes_the_host_adapt_bitrate` | cableado completo: `net_rtt_override=500` → el host emite `video_adapt` con motivo **`red_bitrate`** y su informe queda con **`heartbeats ≥ 3` y `rtt_min` presente** (antes: 0/None) |

## Regresión

Rust **47/0** (46 + `daemon_net_e2e`) · clippy **0/0** · fmt **0** · Flutter
**analyze limpio + 9/9** — sin cambios de app (el evento `video_adapt` ya se
muestra: *«calidad adaptada: red_bitrate → …»*).

## Limitaciones

- Umbrales fijos (200/100 ms, 3/10/20) documentados como decisión inicial:
  sin entrenamiento por enlace todavía (una WAN de ida y vuelta simétrica
  podría necesitar umbrales distintos — candidato a `--net-*` si el campo lo
  pide).
- Se usa **RTT** como señal única: en TCP la pérdida aparece como
  inflación de RTT/retransmisiones (cubierta indirectamente); no hay medición
  de pérdida explícita.
- La señal viaja 1/s con los latidos: la reacción mínima es ~3 s (3
  confirmaciones) — deliberado anti-oscilación.
