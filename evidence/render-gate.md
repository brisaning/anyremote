# Gate C-19 — render alto sin nativo (v0.12.2)

**Fecha**: 2026-11-04 · **Rama**: `feature/render-high` · Origen: README § Pendientes 4 · Diseño: `specs/02` §11

## Diagnóstico (por qué)

| Problema en `app/lib/main.dart` | Coste medido/derivado |
|---|---|
| `_onVideoBytes` recopiaba el **buffer completo en CADA chunk de TCP** (`merged = old + chunk`) | un frame de 720p (≈3,7 MB) llega en ~200 chunks → **~200 `memcpy` de hasta 3,7 MB por frame** (cientos de MB/s solo de copias; cuadrático en bytes/frame) |
| `setState` de la **página entera** por frame + otro `setState` por el cursor (~20 Hz) | reconstrucción de AppBar, chips, toggles y registro a ~30–50 Hz |
| `ImmutableBuffer` / `ImageDescriptor` / `Codec` nunca disposed | a 1080p ≈ 8 MB/frame esperando al GC |

## Qué se implementó

| Pieza | Detalle |
|---|---|
| **`app/lib/video_assembler.dart`** | `VideoFrameAssembler`: buffer que **crece por duplicación y se compacta reutilizando capacidad** (amortizado — jamás recopia «por chunk»); extrae cada frame con **una sola copia** de payload; la resincronía por cabecera inválida se cuenta en `dropped` (mismo comportamiento que antes: se descarta todo). |
| `_onVideoBytes` | delega en el ensamblador (`push` → frames completos → `_renderFrame`); el parsing de cabecera ya no vive en `main.dart`. |
| **Notifiers acotando el repintado** | `_frame` (imagen actual), `_videoStatus` (cabecera del visor, **tope 2 Hz**) y `_cursor` (overlay) → un único `ListenableBuilder` (`_viewerPaint` = merge de frame+cursor) repinta **SOLO el subtree del visor**. La página entera ya no se reconstruye por frame (~30 Hz) ni por cursor (~20 Hz). `video_stats` también va por notifier. |
| `_renderFrame` | **disposal por frame** de `codec`/`desc`/`buf` tras `getNextFrame` (patrón del SDK: `instantiateImageCodecWithSize` libera el buffer al crear el codec — refcount del engine) + la imagen vieja al reemplazarse (como antes). Primer frame → un único `setState` para que `viewerVisible` pase a `true`. |
| `_stopViewer` / `dispose` | limpia ensamblador, notifiers y la imagen actual con contrato propio (antes dependía del `setState` de cada llamante). |

**No se cambió el wire ni el daemon**: el header de 16 bytes (`w,h,seq,len`)
y el payload BGRA son idénticos — solo cambia el consumidor Flutter.

## Tests

- **`app/test/video_assembler_test.dart` (8)**: frame completo en un chunk ·
  multi-chunk con cortes impares (1, 16, 17, …) · varios frames en un chunk
  en orden · header incompleto retenido · basura → resincronía + recuperación
  del frame siguiente · **capacidad acotada tras 200 frames** (regresión
  directa del coste cuadrático: la capacidad no crece con el nº de chunks) ·
  `reset()` · chunks vacíos.
- **`widget_test.dart` +1 (integración)**: frame BGRA crudo 64×64 con cabecera
  real partido en 3 chunks → `debugPushVideoBytes` (mismo camino que el
  preview) → `RawImage` visible + segundo frame con otra resolución sin
  excepciones. Los tests C-9/C-10/C-11 siguen **sin cambiar sus aserciones**
  (las costuras ahora pasan por los notifiers).

## Gates (2026-11-04)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **18/18** (9→18 con C-19) |

## Pendiente de validación de campo

- **Anotar el `fps render`** de la cabecera del visor a **720p / 1080p /
  «Nativa»** con este build: solo si NO llega a los fps del stream se planta
  el paso al widget **`Texture`** (decode fuera de Flutter: cambio L — wire de
  preview + plataforma Windows), que es el «posible paso a nativo» del README.
- Checklist global: **oír el audio** con v0.12.1+ (cadena `rubato`) y probar
  calidad «Nativa».
