# Gate RF audio — audio en sesión (Opus + WASAPI) · v0.6.0 → correcciones de campo en **v0.6.1**

| Campo | Valor |
|---|---|
| Fecha | 2026-10-27 |
| Rama | `feature/audio-session` (D-11) · tag `audio-v0.6.0` · correcciones `fix/field-audio-quality` (v0.6.1) |
| Semilla | spike **cpal + Opus** validado ANTES de crecer el ciclo (ODD) |
| Gates | Rust **40 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` **3/3** |
| Artefactos | `Release/anyremote_app.exe` **0.6.1** + `Release/anyremoted.exe` **0.6.1** |

## Semilla validada (spike 2026-10-27)

| Comprobación | Resultado |
|---|---|
| cpal/WASAPI — entrada | `Microphone (USB Audio)` 48 kHz estéreo F32; captura de 1 s con muestras reales |
| cpal/WASAPI — salida | `Altavoces (USB Audio)` 44.1 kHz estéreo; reproducción de 1 s OK |
| Opus (`opus` 0.4 + `opusic-sys`, libopus embebido) | 1 s de tono → **6.5 KB = 52.4 kbps**; roundtrip **20-28 ms/1 s** (≈35× realtime) |
| Formatos | WASAPI **rechaza** tasas ajenas a la de mezcla → resample lineal 44.1↔48 en ambos lados |

## Qué se entregó

1. **Crate `core/audio` (`anyremote-audio`)** — `AudioMode {Off, Test, On}` (espejo de
   `VideoMode`), captura (tono sintético en `Test`, micrófono por defecto en `On`),
   `OpusCodec` (48 kHz mono, frames de 20 ms), reproducción (conteo en `Test`,
   altavoz por defecto en `On`) y conversiones (downmix/upmix/resample lineal).
   Hilos propios por dispositivo (`cpal::Stream` no es `Send`); el codec vive en el
   daemon (`opus` sí es `Send`). RAII: `Drop` detiene y une los hilos.
2. **Wire** — `AudioFrame {seq, sample_rate, channels, format, data}` + enum
   `AudioCodecEnum` (campo 23 del envelope). `audio_frame_to_wire` /
   `audio_frame_from_wire` en transport **validan** el formato fijo de sesión
   (48 kHz · mono · Opus · payload no vacío) y rechazan el resto.
3. **Daemon `listen`** — con `--audio` pide el permiso **`audio`** (D-07: la
   intersección con el controlador decide); si está concedido arranca la captura →
   `OpusCodec::encode` → `AudioFrame` por el canal. Eventos: `audio_start`
   (modo + 48000), `audio_denied`, `audio_error`.
4. **Daemon `connect`** — con `--audio` ofrece `audio` en la concesión; sin él →
   `audio_denied` y **no abre reproductor**. Con frames → validar wire → decode →
   reproducir; `audio_stats` cada 100 frames (~2 s) con frames/muestras procesadas.
5. **App** — casilla **"Compartir audio"** (desmarcada, D-07) que añade
   `--audio on` al *Escuchar (red)*; el *Conectar* lleva `--audio on` siempre
   (si el anfitrión no comparte, no hay audio). Trazas de `audio_start`,
   `audio_denied` y `audio_error` en el registro (`audio_stats` no se loguea).
6. **Telemetry** — `SessionReport.audio_sent` / `audio_recv` (+ bytes en el
   agregado) para el informe de cierre.

## Tests nuevos

| Suite | Qué cubre |
|---|---|
| `audio::tests::audio_mode_parse…` / `downmix…` / `resample…` | modo, conversiones (identidad, 882→960 exacto, upmix, recorte) |
| `audio::tests::opus_roundtrip_preserves_tone_energy` | 50 frames (1 s) codificados+decodificados: 48 000 muestras y energía conservada (ratio 0.5-1.5) |
| `audio::tests::test_capture_emits_full_pcm_frames` | la captura `Test` emite frames de 960 muestras no nulos y `Drop` detiene el hilo |
| `audio::tests::test_playback_counts_frames_and_samples` | el reproductor `Test` procesa y contabiliza 10 frames / 9600 muestras |
| `transport::arsp::audio_frame_wire_roundtrip_and_validation` | roundtrip del wire + rechazo de tasa ajena, estéreo y payload vacío |
| `daemon_audio_e2e::audio_flows_end_to_end_in_test_mode` | **flujo completo**: concesión de `audio`, `audio_start` en ambos lados, ≥10 frames enviados/recibidos, `audio_stats`, bytes contabilizados, auditoría con el permiso y `closed(bye)` |
| `daemon_audio_e2e::audio_denied_when_controller_does_not_offer` | controlador sin `--audio` → anfitrión `audio_denied`, cero frames |
| `daemon_audio_e2e::audio_denied_when_host_does_not_share` | anfitrión sin `--audio` → controlador `audio_denied` y sin reproductor |
| `widget_test` (Flutter) | ambas casillas D-07 desmarcadas por defecto y conmutables |

## Requisito de build

`opus` compila libopus con **CMake** (VS Build Tools). Si hay que recompilar
(`cargo clean`, nueva máquina), añadirlo al `PATH` antes de `cargo build/test`:

```powershell
$env:Path = "C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;$env:Path"
```

## Para probar en campo

```powershell
# anfitrión: comparte micrófono (marca "Compartir audio" en la app) o:
anyremoted listen --bind 0.0.0.0:21118 --video on --video-size native --audio on --allow-input
# controlador: la app conecta con --audio on → escucha al anfitrión
# prueba sintética sin micrófono:
anyremoted listen  --bind 127.0.0.1:21119 --audio test --video off --clipboard off
anyremoted connect 127.0.0.1:21119 --audio on   # suena un tono de 440 Hz
```

## Limitaciones conocidas

- ~~**Micrófono** como única fuente~~ → **resuelto en v0.6.1** con el **loopback
  (audio del sistema)** como fuente por defecto (ver §Diagnóstico de campo).
- Resample **lineal** 44.1↔48 (calidad suficiente en ratios cercanos; mejorable
  con un resampler de mayor orden si el campo lo pide).
- Sin VAD/eco-cancelación ni silencio adaptativo: se envía 20 ms continuos
  (52 kbps constantes aproximadamente).
- Si el dispositivo falla en mitad de la sesión, la captura/reproducción se
  detiene sin matar la sesión (`audio_error` solo al abrir).

---

## Diagnóstico de campo (2026-10-27) → correcciones v0.6.1

### Síntomas reportados por el usuario

1. "Compartir audio" activo y YouTube sonando en el anfitrión → **sin audio**
   en el controlador.
2. Conexión y control funcionan, pero **muy lentos** para ser una LAN.

### Evidencia (auditorías de ambos equipos)

| Línea | Hallazgo |
|---|---|
| Anfitrión `permission_granted` | `{"perms":["keyboard","mouse","audio"]}` → el permiso **sí** se concedió |
| Anfitrión `error` | `{"audio":"Device(\"sin dispositivo de entrada\")"}` → **el anfitrión no tiene micrófono** y la fuente era el micrófono |
| Anfitrión report | `frames_sent=531` en 116.5 s → **4.6 fps** · `bytes_sent=27.7 MB` (≈1.9 Mbps) a **resolución nativa** |
| Controlador `heartbeat_summary` | `heartbeats_ok=78, rtt_min_ms=57, rtt_last_ms=782` → **RTT de enlace 57–782 ms** (frente a 1 ms en loopback) |

### Causas

1. **Audio**: (a) la fuente era el **micrófono**, pero YouTube es *audio del
   sistema* → hacía falta **WASAPI loopback**; (b) además ese equipo no tiene
   micrófono, así que la captura fallaba al abrir.
2. **Lentitud**: vídeo a **resolución nativa** con VP9 por software → ~4.6 fps
   en el anfitrión (cuello de CPU de codificación) y ráfagas grandes de
   keyframe que inflan el RTT del enlace (57–782 ms).

### Correcciones (v0.6.1)

1. **`AudioSource` (defecto `system`)** — loopback WASAPI: cpal usa el endpoint
   de **render** como entrada y añade `AUDCLNT_STREAMFLAGS_LOOPBACK` → se
   captura todo lo que suena en los altavoces (YouTube, vídeos, llamadas) **sin
   micrófono**. `--audio-source mic` para seguir usando el micrófono.
2. **`audio_stats.rms`** — RMS medio del audio decodificado (`0.0000` =
   silencio) + indicador **`audio ✓` / `audio: sin señal`** en la cabecera del
   visor: "¿llega audio real?" se ve de un vistazo.
3. **Calidad de vídeo en la app** — selector **720p (defecto) · 1080p ·
   Nativa**: con `native` el anfitrión solo alcanzaba 4.6 fps.

### Verificación (loopback real, este equipo)

Tono de 440 Hz por los altavoces (A) → `listen --audio on --audio-source system`
(B) → `connect --audio test` (C):

- B: `audio_start {"mode":"On","source":"system","rate":48000}` (sin errores)
- C: `audio_stats {"frames":100,"samples":96000,"rms":0.1718}` → **sonido real
  capturado** (silencio daría `0.0000`)

Gates: Rust **40/0** · `flutter analyze` limpio · `flutter test` **3/3**.
