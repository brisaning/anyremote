# Gate Hito M2 — Streaming de vídeo a la UI (visor remoto)

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/m2-video` (D-11) · tag `m2-video-v0.2.0` |
| Daemon | `anyremoted 0.2.0` (flag `--video off\|on\|test`, `--video-size`, `--video-fps`, `--codec vp9\|deflate\|raw`) |

## Arquitectura entregada

- **`daemon::video`**: tarea de captura (`Auto` = DXGI con fallback a patrón / `Test`) → escalado por vecino más próximo → **preview local** (BGRA crudo) → encode (VP9 por defecto) → cola `mpsc(3)` con **backpressure por descarte** hacia el canal de sesión. Detención por `StopGuard` al morir la sesión.
- **`daemon::preview`**: servidor TCP loopback de un solo cliente; stream `u32le w|h|seq|len + BGRA`; si no hay cliente o hay congestión, descarta (la visualización nunca ahoga a la sesión).
- **Sesión**: `listen --video` envía `VideoFrame` ARSP (brazo select con patrones `conn_gone` ya endurecidos); el controlador **decodifica** (VP9 con estado), alimenta su preview y emite `video_stats`; errores de vídeo → `video_error` (no matan la sesión, doctrina v0.1.4).
- **App**: evento `preview{port}` → socket → parser de cabecera con resincronización → `ImageDescriptor.raw(BGRA)` → widget `RawImage` con fps/dimensión en vivo; visor auto-conecta al activar la sesión.

## Gate automatizado
`daemon_video_e2e::m2_video_streams_and_preview_delivers_frames` (**4.49 s**): listen `--video test` 320×200 → `video_start` ✓ → controlador `video_stats` ✓ con `bytes_recv>0` y `bytes_sent>0` en el emisor → **cliente TCP real al preview valida cabecera (320,200,320·200·4)** ✓.

## Smoke con procesos reales (release, VP9 + captura real)
```
listen: {"event":"video_start","mode":"Auto","width":640,"height":360,"fps":15,"codec":"Vp9"}
ctrl:   {"event":"video_stats","frames":61,...}         ← ~15 fps decodificados (4 s de sesión)
listen: bytes_sent=419523                               ← ≈560 kbps VP9 @640x360@15
ctrl:   heartbeats=6 rtt_min=1ms · closed(max_secs)     ← sesión sana, cierre limpio
preview (cliente TCP real): 41 frames / 2.55 s = 16.1 fps · first=(640,360,921600) ✓
```

## Gates generales
Rust **36 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` 1/1.

## Prueba manual (usuario)
```
Terminal: anyremoted listen --video on --video-size 1280x720 --video-fps 15 --clipboard win
App:      127.0.0.1:21118 → Conectar   (o Escuchar (red) en el otro equipo)
→ el panel "Visor remoto" muestra el escritorio del controlado con fps en vivo.
```

## Limitaciones declaradas
- Escalado por vecino más próximo (calidad suficiente para preview; resampling real = futuro).
- Sin audio (scope RF-02 sin audio en el spec actual de ciclos).
- Rendimiento de render Flutter a resolución nativa completa = medir en campo; bajar `--video-size`/`--video-fps` si hace falta.

## Confirmación de campo (usuario)

- **2026-10-26**: el usuario verificó **visualización de la pantalla remota** en el visor de la app (v0.2.1 + listen --video on) sobre LAN — hito M2 validado de extremo a extremo en producción real (captura → VP9 → sesión → preview → render Flutter).
