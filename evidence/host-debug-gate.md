# Gate C-5 — diagnóstico de campo del anfitrión · v0.7.3

| Campo | Valor |
|---|---|
| Fecha | 2026-10-27 |
| Rama | `fix/host-diagnostics` (D-11) · tag `diag-host-v0.7.3` |
| Gates | Rust **41 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` **4/4** |

## Qué se pidió

1. **Diagnóstico**: la desconexión solo aparece con «Control remoto + Micrófono»
   (sin *Audio del sistema*); con los tres activos no ocurre.
2. **Requisito de UX**: cuando el aplicativo es **anfitrión no debe mostrar la
   pantalla que está compartiendo** — solo el registro.

## Qué se ha verificado con tus artefactos

| Artefacto | Hallazgo |
|---|---|
| `E:\audiany\host_listen.log` (tu captura por terminal) | Sesión **sana**: `daemon 0.7.2` → `audio_start source=system` → `preview` → `video_start 1280x720@15` → `clipboard_sync` → cientos de `input_injected` → `closed reason=bye`. **Sin panics.** |
| Auditoría remota reciente (7 min) | `frames_sent=107`, `bytes_sent=2.5 MB`, cierre `bye` limpio, aviso de audio **sin envoltorio** (`mic: Device(...)`) = **degradación correcta** (captura sigue con `system`) |
| Auditorías de las sesiones fallidas | error de audio **con envoltorio** (`Device("mic: Device(...)")`) = la ruta de **ERROR** (todas las fuentes fallan) → solo `[mic]` solicitado |

### Causa de la combo que falla (confirmada por tu discriminador)

- «Control remoto + Micrófono» **sin** *Audio del sistema* → el comando
  `{"perm":{input:true,audio:false,mic:true}}` reduce las fuentes a **`[mic]`**.
- Ese equipo **no tiene micrófono** → `AudioCapture::start` falla por completo
  (`spawn_audio_capture` → `None`) y a partir de ahí la sesión no llega a
  producir vídeo (0 bytes recibidos en el controlador en esos intentos).
- Con los tres activos → fuentes `[system, mic]` → el loopback abre y la sesión
  funciona (tu log:107 frames y cierre limpio).

## Cambios (v0.7.3)

1. **`panic.log` del daemon** — gancho de pánico global en `anyremoted`: cualquier
   pánico (en cualquier hilo) se añade a **`<config_dir>/panic.log`** (timestamp,
   hilo, ubicación, mensaje) además de salir por stderr. Como `panic.log` vive en
   la misma carpeta que `audit/`, se copia con el mismo flujo de diagnóstico.
2. **`ui.log` de la app** — todas las líneas del registro (con marca de tiempo y
   los `args` exactos de listen/connect) se añaden a
   **`%APPDATA%\anyremote\ui.log`** (se trunca a 1 MB). Antes el registro
   vivía solo en la ventana y se perdió al cerrar la app — por eso no hubo
   artefacto de la sesión fallida.
3. **Un solo intento de captura por sesión** (`DynamicPerms.spawn_attempted`):
   si el comando en caliente ya decidió el estado, el arranque inicial no
   reintenta → como mucho **un** `audio_error` por sesión (antes podían aparecer
   dos intentos ambiguos).
4. **El anfitrión no muestra su propia pantalla**: el evento `preview` del
   proceso *listen* se ignora en la app (solo el controlador abre visor). En
   modo anfitrión queda **únicamente el registro**, tal como pediste.

## Qué hace falta para cerrar el diagnóstico

✅ **CERRADO EN CAMPO (2026-10-27, v0.7.3)**: el usuario reprodujo la prueba y
**no se presentó el fallo — funcionó correctamente**. En esta versión el comando
en caliente decide el estado con **un solo spawn de captura por sesión** (el
segundo intento del arranque inicial — que solo ocurría cuando la combo inicial
reducía las fuentes a `[mic]` — desapareció) y la app dejó de abrir visor en el
rol anfitrión. Se mantiene la instrumentación (`ui.log` + `panic.log`) para
cualquier incidencia futura: con ellos la próxima anomalía deja artefacto
(args + código de salida + ubicación del pánico) sin pasos extra.

## Nota

- El requisito «no desconectar por falta de movimiento»: revisado — no existe
  ningún cierre por inactividad en el código (solo `bye`, `max-secs` o muerte
  del proceso); la desconexión que se veía era la sesión sin vídeo + cierre de
  la app. El fix de v0.7.2 mantiene la pantalla quieta con refresco ≈1 fps.
