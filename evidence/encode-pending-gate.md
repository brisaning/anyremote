# Gate C-28 — el encoder no tira frames + instrumentación de tasa (v0.13.1)

**Fecha**: 2026-11-15 · **Rama**: `feature/encode-pending` · Disparador: reporte de campo — «en el cliente solo se ve la posición final, tarda5-10 s o se actualiza al hacer clic»

## Diagnóstico (correlación de logs, sesiones v0.13.0 del14/11)

| Sesión | Camino | Host envió | Cliente decodificó | Errores |
|---|---|---|---|---|
| juank2 (LAN) | QUIC |141 frames/2,03 MB/82 s |1,09 MB |0 ✓ |
| anfitrión2 (relay) | TCP relay | **321 frames/13.723.863 B/228 s** | **13.723.863 B (idéntico)** |0 ✓ |
| juank2 (2ª) | QUIC |270 frames/3,4 MB/159 s |2,04 MB |0 ✓ |

**Descartado con evidencia**: transporte (bytes extremo a extremo idénticos,
`frames_dropped:0`), decodificación (0 errores — C-27 OK), render del
cliente (el frame final sí se pinta). **Queda**: la PRODUCCIÓN =
**~1,4–1,7 fps emitidos** con objetivo de15.

**Causa raíz (H1)**: en `mf_backend::encode`, si la entrada del MFT no
está libre se drenaba la salida **pendiente del frame anterior** y se
tiraba (`let _ = self.drain(&t)?;`). En interacción intensa (entrada a15
fps con encode rezagado) eso perdía frames sistemáticamente → el cliente
veía «congelado» hasta que un **clic cambiaba píxeles → frame nuevo → se
pintaba** (y los5-10 s = tiempo hasta el siguiente cambio de píxeles
real).

## Qué se implementa

| Pieza | Detalle |
|---|---|
| **1. FIX (H1)** — cola FIFO de salida | `pre_raw` (drenaje previo) se **conserva** y se encola; `pick_output()` emite **lo más viejo primero** (el pre-drenaje corresponde al frame ANTERIOR), encola el actual, tope **`PENDING_CAP=8`** (≈0,5 s a15 fps — con el MFT rezagado se descarta lo nuevo para no acumular latencia infinita) y **`pending.clear()` en cada reconstrucción** (salidas del MFT anterior con seq_hdr nuevo). |
| **2. Instrumentación anfitrión** | `[video] fps (Ns): captura=… entraron=… emitidos=… entregados=…` cada5 s (stderr → `ui.log` como `[listener-err]`): **captura** = frames DXGI reales · **entraron** = entraron al encode (incluye reenvíos) · **emitidos** = códec con salida · **entregados** = al canal de sesión. LaComparación `captura` vs `emitidos` en un arrastre de5 s separa H1/H4 (códec) de H2 (captura). |
| **3. Instrumentación visora** | `[ui] visor: fps render=… · N frames` cada5 s con **`_appendQuiet`** (archivo+lista **sin `setState`** — no fuerza repaints y no enmascararía un fallo de repintado). Si entre líneas `frames` no crece → el render no recibe/producir. |

## Tests (+1 →25 Flutter /51 suites Rust)

- `pick_output_keeps_order_and_caps_pending`: sin cola → actual ·
  pre-drenaje (más viejo) primero + FIFO · cola ≤8 con30 vueltas ·
  actual vacío no encola basura.
- Widget `C-28`: visor conectado +5 s reales → la entrada
  `fps render` aparece en el registro (tras cerrar el visor — en modo
  visor el Registro está oculto, C-10).

## Gates (2026-11-15)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **25/25** |

## Pendiente de validación de campo (el test de5 s)

1. En el anfitrión: arrastrar una ventana **5 s** → leer en `ui.log`
   `[video] fps (5 s): captura=… emitidos=…`:
   - `captura≈15/s` y `emitidos≈1-2/s` → **códec** (H1/H4): ¿mejoró con
     este fix? (antes emitidos≈1,5 siempre);
   - `captura≈1-2/s` → **captura/QW-1** (H2).
2. En la visora: `[ui] visor: fps render=… · N frames` — si `frames` no
   crece → el render del cliente (H5).
3. Confirmar si el síntoma «posición final tras clic» desapareció.
