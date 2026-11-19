# Gate C-41 — WAN con micrófono: validación y vídeo congelado (v0.13.13)

**Fecha**: 2026-11-19 · Rama: `feature/c41-wan-stall` · Origen: prueba de campo
WAN (relay) con el **micrófono activo** (07:33–07:35, Julie ← Brian).

## Lo que dice el log

### Micrófono: ✅ validado de extremo a extremo

| Lado | Cierre de sesión |
|---|---|
| Anfitrión | `bytes_sent=3234051` · **`audio_sent=100`** |
| Cliente | **`audio_recv=100`** |

Los contadores de C-40 son **frames** de audio (no bytes): 100 frames en ~34 s
con el gate de silencio/DTX activo. El camino `mic → wire → reproducción` llega.
El aviso `audio sin señal (rms 0)` del inicio corresponde a silencio real (nadie
hablando en ese instante).

### Conexión: relay WAN correcto

`ID resuelto → WAN (servidor de encuentro)` · `relay_fallback` · `SESIÓN ACTIVA`
con **`rtt_min = 279–399 ms`** (relay).

### Vídeo: se congeló sin avisar

| Hora | Hecho |
|---|---|
| 07:34:16 | `red_bitrate` 6,2 → **3,1 Mbps** |
| 07:34:28 | `red_bitrate` → **1,55 Mbps** |
| 07:34:32 | `red_bitrate` → **777 kbps** |
| 07:34:35 | `red_bitrate` → **388 kbps** |
| 07:34:36 | anfitrión: `sesión cerrada: connection_gone` |
| 07:34:37 → 07:35:17 | cliente: **129 frames, sin cambios** (imagen congelada, 50 s) |
| 07:35:22 | cliente: cierre (`heartbeats=34`) |

Además el anfitrión acumuló fallos de rendezvous (`os error 10065` host no
accesible, `10013` permisos, `10054` reset) en 07:33–07:44: la red hacia el VPS
estuvo inestable. La cascada de bitrate es coherente con esa congestión.

**Fallo de producto**: el visor siguió mostrando la última imagen y un
`fps render=6,3` suavizado, sin ninguna señal de que el vídeo llevaba 50 s parado.

## Cambios

| Pieza | Detalle |
|---|---|
| `main.dart` | **Watchdog de vídeo**: si pasan **>5 s sin un frame** con sesión activa, la cabecera pasa a «sin vídeo desde hace N s (¿red?)», el `fps render` se pone a **0** y se registra **una** línea (`sin frames desde hace N s — la sesión puede estar caída`). Se reinicia al volver el primer frame. |
| `main.dart` | El registro de `calidad adaptada` incluye **`· rtt Xms`** (el valor que disparó la decisión), ya presente en el evento desde C-40. |
| `stallSeconds()` | Helper puro con test: umbral 5 s, `50 s` para el caso de campo. |

## Gates

| Gate | Resultado |
|---|---|
| `cargo test --workspace` | **159 / 0** |
| `cargo clippy --workspace --all-targets` | **0 avisos** |
| `cargo fmt --all -- --check` | **0 diffs** |
| `flutter analyze` / `flutter test` | limpio · **30 / 30** |

## Recomendaciones para la próxima prueba WAN

1. **Comprobar el VPS/red del anfitrión** antes de la sesión: los `10065/10013`
   indican que el relay no era alcanzable desde Brian (firewall o caída).
2. Con relay estable, repetir 1080p: el objetivo es que el vídeo **no** baje de
   ~1 Mbps ni se congele; si la cascada vuelve a 388 kbps, mirar el `rtt` que
   ahora aparece en el registro.
3. Si el WAN estable sigue sin aguantar 1080p, el siguiente paso es un **tope
   por ruta**: arrancar el vídeo más bajo cuando la sesión va por relay
   (hoy arranca al 60 % del preset y baja a golpe de RTT).
