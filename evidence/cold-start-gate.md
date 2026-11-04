# Gate C-23 — arranque en frío: frame inicial GDI + input en el placeholder (v0.12.6)

**Fecha**: 2026-11-04 · **Rama**: `feature/cold-start` · Disparador: **revisión de logs de campo** (pruebas del usuario, v0.12.5)

## Diagnóstico (de los logs)

Las **2 sesiones por relay** de las pruebas (13:01 y13:11) tuvieron
**`frames_sent:0` / `bytes_sent:0`** en el anfitrión y **`bytes_recv:0`** en
la visora (ambos extremos coinciden) y **0 inyecciones**, mientras que en LAN
fluyeron65/15 frames (1 MB/314 KB) + **85 inyecciones**. El código de vídeo
**no cambió** desde el test C-17 (3-nov), donde el relay sí llevó59 frames /
103 inyecciones → no era regresión.

**Mecanismo**: la captura DXGI solo emite si hay cambios (QW-1 descarta lo
sin cambios; el reenvío a1 Hz solo existe *después* del primer frame) y el
visor **solo montaba el input cuando había frame** (el placeholder
«conectado… esperando primer frame» no capturaba ratón/teclado). Pantalla
del anfitrión **estática/bloqueada** al abrir la sesión → sin primer frame →
sin input → sin cambios → **sin frames…** (deadlock). Además los errores de
captura se tragan (`Err(_) => None`) → «estático (timeout)» y «bloqueo
(access lost)» eran indistinguibles en el log.

## Qué se implementó

| Parte | Detalle |
|---|---|
| **B — frame inicial (daemon)** | **`anyremote_capture::initial_snapshot()`** (`core/capture/src/gdi.rs`): BitBlt GDI del escritorio primario → BGRA8 top-down con **alfa forzado a 0xFF** (BI_RGB lo deja en0 → el visor lo vería transparente). Primera llamada fija **Per-Monitor V2** (si no, GDI devolvería el escritorio virtualizado a escala — y en modo nativa fijaría mal la resolución; seguro porque el input usa `MOUSEEVENTF_ABSOLUTE` normalizado y DXGI ya es físico). El bucle de vídeo (`video.rs`) lo llama en `Timeout` **o error duro** del backend **mientras no haya `last_frame`** → a partir de ahí el reenvío a1 Hz mantiene viva la vista y los cambios reales retoman DXGI. En plataformas sin soporte devuelve `Unavailable` (comportamiento previo). |
| **A — input siempre (app)** | El `Listener`/`Focus` del visor envuelven **también el placeholder**: ratón/teclado se capturan sin frame → el primer clic/movimiento real cambia píxeles en el anfitrión → DXGI despierta. El overlay del cursor se mantiene solo con imagen. |
| **Bug destapado por el test** | `dispose` del visor con preview abierto: `_preview.destroy()` dispara `onDone` **síncrono** → `_stopViewer`/`_videoStatus` con `setState` sobre el elemento ya `defunct` (regresión del `setState` añadido en C-19, solo visible en debug). Fix: bandera **`_disposed`** + retorno temprano en `_stopViewer` + guardas en `onDone`/`onError`. |

## Tests

- **capture (+1 →5)**: `gdi_snapshot_works_or_reports_unavailable` — con
  escritorio → dims>0, `Bgra8`, longitud exacta y **alfa0xFF en todos los
  píxeles**; sin escritorio (CI/servicio) → `Unavailable`/`Io` sin paniquear.
- **Flutter (+1 →22)**: visor abierto SIN frames (socket local mudo) → el
  placeholder existe, tiene **`Listener` ancestro**, el tap no lanza
  excepciones, y al inyectar un frame pasa a `RawImage` sin perder input.
- Los gates anteriores siguen intactos (C-9/C-10/C-11/C-19/C-20/C-21/C-22).

## Gates (2026-11-04)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** (capture **5/5**) |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **22/22** |

## Pendiente de validación de campo

- Anfitrión con **pantalla estática** (sin tocarlo): al conectar, el visor
  debe ver la imagen **en menos de1 s** (GDI) y el ratón debe mover el cursor
  del anfitrión (rompe el deadlock).
- Con el anfitrión **bloqueado**: imagen congelada (GDI) en vez de espera
  infinita; el control no desbloquea (normal — input en el escritorio de
  usuario).
- Las sesiones relay deben llevar ahora **vídeo e input** como en C-17.
