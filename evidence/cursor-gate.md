# Gate C-11 — cursor remoto visible (overlay) · v0.8.5

| Campo | Valor |
|---|---|
| Fecha | 2026-10-29 |
| Rama | `feature/cursor-overlay` (D-11) · tag `cursor-v0.8.5` |
| Disparador | Decisión del usuario sobre el backlog (siguiente implementación) |
| Gates | Rust **45 suites / 0 fallos** · **clippy 0 avisos / 0 errores** · **fmt 0 diffs** · `flutter analyze` limpio · `flutter test` **9/9** |

## Problema

DXGI **no** dibuja el cursor hardware en los frames capturados (patrón
documentado también en RustDesk): durante el control remoto el visor **no ve
dónde está el cursor del anfitrión** (solo se infiere por los efectos de
hover/la propia imagen local). Era el 1º del orden UX propuesto en campo
(cursor → adaptación por red, etc.).

## Diseño (clean-room D-01, patrón propio)

1. **Anfitrión**: hilo propio en cada sesión controlada que sondea el cursor
   cada **50 ms** con `GetCursorPos` + `GetCursorInfo` (`CURSOR_SHOWING` para
   visibilidad) y publica **solo cambios** por un canal **`tokio::sync::watch`**
   (último estado, sin encolar; el hilo termina solo al caerse la sesión).
   - **Métrica**: normalización **0..65535 contra el monitor primario** — la
     MISMA que usa el inyector con `MOUSEEVENTF_ABSOLUTE` y que el visor usa
     al mapear su caja (`normalizedPointer`) → **el overlay aparece donde
     caen los clics**. Fuera de rango (monitores secundarios) → recorte al
     borde, igual que el inyector.
2. **Wire**: mensaje `CursorState {nx, ny, visible}` (campo **25** del
   envelope). Envío **solo en cambios** (≈40 B); compatibilidad hacia atrás
   (campo desconocido se salta en daemons antiguos).
3. **Controlador**: brazo de recepción → evento JSONL **`cursor`** para la
   app. **Sin auditoría** (~20 Hz inundaría el `audit/`; es estado de
   display, no un evento de sesión).
4. **Visor (app)**: sobre el vídeo (`Stack` con `StackFit.expand`, que
   conserva las restricciones del layout C-9), `Positioned` en
   `nx/65535 · ancho_caja`, `ny/65535 · alto_caja` con una **flecha dibujada
   en canvas propia** (`_RemoteCursorPainter`, sin assets), punta en la
   posición exacta. Sin visor → no se aplica (evita rebuilds a 20 Hz);
   `closed`/`_stopViewer` lo limpian.

## Tests

| Suite | Qué cubre |
|---|---|
| input · `normalize_cursor_is_the_inverse_of_the_viewer_mapping` | espejo de `normalizedPointer` (centro ≈ 32767), recorte fuera de rango, degenerado |
| input · `backends_report_positions_and_sequence_advances` | `Fixed` estable y `Sequence` ciclando |
| daemon e2e · `fixed_cursor_sends_exactly_one_event` | posición **fija** → **exactamente 1** evento `cursor` en el controlador (**change-detection** real), valores y `visible` correctos |
| daemon e2e · `moving_cursor_streams_updates_in_range` | cursor en movimiento → **≥2 eventos**, todos `0..65535`, primer estado = primer valor del guion y segundo estado llega |
| app · `C-11: overlay del cursor remoto sobre el vídeo` | sin estado → sin overlay; con `(49151,16384)` → la flecha queda a **la misma proporción** sobre la caja de la imagen (±3 %) y al ocultar **desaparece** |

## Regresión

Rust **45/0** · clippy **0/0** · fmt **0 diffs** · Flutter **analyze limpio +
9/9** — los gates de C-8/C-9/C-10 (reconexión, sin-desbordes, modo visor)
pasan sin cambios: el `Stack` reemplaza al `RawImage` suelto **con las mismas
restricciones** (`StackFit.expand`).

## Limitaciones

- **Monitores secundarios**: el cursor se normaliza contra el primario (como
  el propio inyector — el control remoto ya tenía este alcance); con el
  cursor sobre el secundario el overlay queda recortado en el borde del visor.
- La **forma** de la flecha es un dibujo propio (no se transporta el shape
  real de Windows): posición, hotspot (punta) y visibilidad sí viajan.
- Sondeo a 20 Hz: movimiento instantáneo puede quedar hasta ~50 ms por
  detrás (imperceptible frente al pipeline de vídeo).
