# Gate C-18 — resampler de mayor orden (`rubato`, v0.12.1)

**Fecha**: 2026-11-04 · **Rama**: `feature/resampler-rubato` · Plan: `evidence/menores-gate.md` §4 + `evidence/audio-gate.md` §Limitaciones

## Qué se implementó

| Pieza | Detalle |
|---|---|
| **`core/audio/src/resample.rs`** | `SincResampler` sobre **`rubato` v5** (puro Rust): `Fft` (windowed-sinc) con ventana **Blackman-Harris2** y sub-chunks ≈256 (los parámetros por defecto = el «esfuerzo M» del plan). |
| Streaming por chunks fijos | `FixedSync::Input` con chunks de **1024 muestras (~21 ms @48 kHz)**: la entrada variable se acumula en `pending` y solo se procesa al completar un chunk; el remanente se conserva → **nada se pierde entre llamadas**. Retardo añadido ≈ chunk + media mitad de bloque FFT (ms) — irrelevante sobre red. |
| Captura (`capture_loop`) | `Feed.resamp: Option<SincResampler>` creado iif `tasa_dispositivo ≠ 48 kHz`; `None` = sin conversión (ya 48 kHz) — el drenaje por `frame_in` no cambió. |
| Reproducción (`playback_loop`) | `resamp` creado del mismo modo con la tasa del altavoz; `None` = sin conversión. |
| **Lineal retirado** | `resample_linear_i16` **eliminado de producción** (no filtra Nyquist: al bajar pliega contenido >Nyquist nuevo → alias audible; al subir crea imagen). La referencia lineal queda SOLO en los tests de `resample` para la comparación de calidad. |

## Por qué (evidencia numérica en tests)

- `downsampling_quality_beats_linear`: seno de **5 kHz** muestreado a 48 kHz
  bajado a 44.1 kHz y comparado contra la onda ideal (alineando el sinc con su
  `output_delay()`): el RMSE del lineal debe superar **100** (se ejercita de
  verdad) y el del sinc debe ser **≥4× menor** — se cumple con margen.
- `length_bounds_and_silence`: longitud ≈ entrada×ratio (±1500 muestras de
  retardo/cola), sin sobreimpulso (pico ≤+10 %) y **silencio → silencio**
  (≤2 LSB, el FFT no inventa ruido).
- `resample_sinc_scales_length` (lib): subir y bajar con la longitud esperada.

## Gates (2026-11-04)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** (crate audio: **13/13**,3 dedicados al resampler) |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **9/9** |

## Pendiente de validación de campo

- **Oír audio en sesión** con este build (checklist de `specs/04` §3): loopback
  del sistema + micrófono + toggles en caliente + gate de silencio, ahora con
  la cadena de conversión de mejor orden en captura Y reproducción (dispositivos
  a 44.1 kHz son el caso que antes aliasaba).
