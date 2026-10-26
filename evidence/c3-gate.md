# Gate ciclo C3 — RF-02a: captura, códec y entrada

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/c3-video-input` (norma D-11) |
| Suite | **29 tests, 0 fallos** (capture 3 · codec 4 · input 2 · identity 6 · session 5 · transport 8 · e2e 3) |

## Entregados

- **`core/capture`**: trait `Capturer` + `Frame` BGRA8; `TestPatternCapturer` determinista; **`DxgiCapturer`** (DXGI Desktop Duplication real: factory→adapter→output→DuplicateOutput→staging CPU-read) — **test con escritorio real: 5.39 s, frames adquiridos** ✓; `best_capturer()` con fallback.
- **`core/codec`**: traits `VideoEncoder/VideoDecoder` (supertrait Send) + `CodecFormat{Raw,Deflate}` negociable (espejo ARSP) + backend **Deflate** lossless (flate2) + `NativeDecoder`; roundtrip verificado byte a byte.
- **`core/input`**: modelo `InputEvent/Key/MouseButton` + trait `InputInjector`; `RecordingInjector` (tests/gate); backend **Windows SendInput** (VK via VkKeyScanW, ratón abs/rel, rueda; inyección solo bajo permiso D-07 — nunca ejercitada en tests automáticos).
- **ARSP**: `VideoFrame{seq,w,h,format,data}` + `InputEventMsg{Key,Pointer,Button}` en `arsp.proto`.
- **Gate e2e** `e2e_video_streaming_and_input_injection`: controlado captura→deflate→envía 30 frames por sesión TLS; controlador decodifica (integridad del frame 0 == patrón determinista) y envía 3 eventos de entrada; el controlado los registra exactos (KeyDown 'a', MoveTo 320x180, ButtonDown Left).

## RNF-02 — medición (objetivo declarado)

| Backend | avg ms/frame | fps | compresión |
|---|---|---|---|
| Deflate (n1) | 548.96 | 2 | 0.17x |
| Raw | 2.25 | 444 | 1.00x |

(1280x720, 240 frames, integridad verificada en todos — `pipeline_bench`).

**Veredicto honesto**: el pipeline y la arquitectura por capas funcionan; **Deflate no alcanza tasas de vídeo** (objetivo ≥30 fps). Gap confirmado y aceptado: **backend VP8/VP9 real (D-06) requiere libvpx en el host** — semilla para ciclo **C3-bis** antes de consolidar RF-02 en C8. Latencia de transporte/códec Raw (2.25 ms) deja margen holgado para un códec real.

## Limitaciones registradas

- DXGI exige escritorio interactivo (en RDP/servicio devuelve `Unavailable` — test lo tolera).
- Wireformat de input mapeado en el test (cableado definitivo en daemon, ciclos C7/C8).
