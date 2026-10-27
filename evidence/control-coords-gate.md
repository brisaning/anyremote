# Gate fix control remoto a 720p/1080p — coordenadas normalizadas · v0.6.2

| Campo | Valor |
|---|---|
| Fecha | 2026-10-27 |
| Rama | `fix/control-coords` (D-11) · tag `coords-v0.6.2` |
| Gates | Rust **40 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` **4/4** |

## Problema (reportado en campo)

Al elegir **720p o 1080p** en la app, el control remoto dejaba de funcionar
(clics que caían fuera de sitio); solo funcionaba con vídeo *nativo*.

## Causa

El visor mapeaba el clic a **píxeles del frame de vídeo**, pero el backend
win32 inyectaba esas coordenadas como **píxeles de pantalla** (normalizándolas
con `GetSystemMetrics`). Con vídeo escalado (1280x720 en pantalla 1920x1080)
las coordenadas llegaban comprimidas al cuadrante superior-izquierdo → el cursor
iba a otra parte de la pantalla y no había control.

## Solución

Coordenadas **absolutas normalizadas 0..65535** (la semántica que ya documentaba
el modelo de dominio):

- App: `normalizedPointer(local, box) = local / box * 65535` — **independiente**
  de la resolución del vídeo y de la pantalla del anfitrión.
- `core/input/windows_backend.rs`: `MoveTo{x,y}` ya llega normalizado → se
  envía directo con `MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE` (eliminada la
  normalización por `GetSystemMetrics`).
- `Pointer.absolute` documentado como 0..65535 en `arsp.proto` y en transport.

`MoveBy` (relativo) sigue en píxeles. Multi-monitor sigue refiriéndose al
monitor primario (como antes).

## Tests

| Suite | Qué cubre |
|---|---|
| Flutter `normalizedPointer…` | centro (≈32768), bordes (0 / 65535), arrastre fuera del visor (recorte), caja degenerada (0,0 sin división por cero) e **invarianza de escala** (160/320 == 640/1280 → mismo valor) |
| Rust (sin cambios) | passthrough del wire: e2e de entrada del daemon y tabla de mapeo de transport siguen verdes |

## Verificación

Cualquier calidad (720p · 1080p · Nativa) produce el mismo valor normalizado
para la misma posición relativa → el control remoto funciona igual en todas.
