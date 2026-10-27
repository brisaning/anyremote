# Gate C-4 — vídeo en pantalla estática + captura fuera del worker · v0.7.2

| Campo | Valor |
|---|---|
| Fecha | 2026-10-27 |
| Rama | `fix/video-static-thread` (D-11) · tag `video-estatico-v0.7.2` |
| Origen | Reporte de campo tras v0.7.1: *«en ninguna [calidad] se ve el vídeo, solo el registro y sesión cerrada»* |
| Gates | Rust **41 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` **4/4** |

## Diagnóstico (auditorías de ambos equipos)

| Evidencia | Lectura |
|---|---|
| Controladores fallidos (3 intentos: 720p/1080p/Nativa) | **`bytes_recv=0`** → el anfitrión no envió NI UN frame de vídeo; `disconnected: connection_gone` a los ~3 s |
| Anfitrión (`E:\audiany\audit`) | sesiones con `permission_granted` + `audio: Device("mic: sin dispositivo de entrada")` (degradación correcta, ≥v0.7.0) pero **sin línea `disconnected`** → el proceso terminó sin cierre limpio (cierre manual tras no ver vídeo) |
| Reproducción local con los args EXACTOS de la app (Release) | `video_stats frames=61`, `bytes_sent≈1.1 MB` ✓ — el software funciona en este equipo |

### Causa raíz (dos efectos de la misma condición: pantalla del anfitrión estática)

1. **QW-1 + pantalla sin cambios**: DXGI solo entrega frames cuando hay cambios
   (los de solo-cursor los descarta QW-1 por diseño) → con el anfitrión quieto
   **no se produce ni un frame** hasta que algo cambie en su pantalla: el visor
   del controlador se quedaba en blanco durante toda la prueba.
2. **Bloqueo en el worker de tokio (QW-5)**: `AcquireNextFrame` espera hasta
   **500 ms** en cada tick cuando no hay cambios — ejecutándose dentro de
   `tokio::spawn`, ese bloqueo **estrangulaba el bucle de sesión** del anfitrión
   en equipos con pocos núcleos (vídeo y latidos sin salir), y encima reiniciaba
   la espera en cada intento. (El propio informe `perf-audit.md` recomendaba
   mover la captura/encode fuera del runtime: QW-5.)

## Fix (v0.7.2)

1. **`spawn_video_task` ahora es un hilo propio** (`std::thread::spawn`):
   captura → escala → encode → `try_send` con pacing por `std::thread::sleep`.
   Los bloqueos de DXGI ya **no tocan** a los workers de tokio → el bucle de
   sesión (vídeo, latidos, entrada, portapapeles) sale siempre.
2. **Reenvío en pantalla estática**: si `AcquireNextFrame` da `Timeout` y hay un
   último frame emitido, se **reemite como máximo 1 vez por segundo** (coste: 1
   codificación/s ≈ nada) → el visor muestra la pantalla del anfitrión en lugar
   de quedarse ciego. Si aún no hay primer frame, se reintenta (DXGI entrega el
   primer frame con el primer cambio o con la imagen inicial).
3. Test de captura tolera `Timeout` (pantalla estática legítima).

## Verificación (este equipo, pantalla mayormente estática)

| Medida | Resultado |
|---|---|
| Sesión 6.5 s con args de la app (720p, `--max-bitrate 3000`, audio system,mic) | **`frames_sent=13` (≈2 fps), `bytes_sent=140 222`** → el controlador RECIBE vídeo (antes de este fix, en pantalla estática: ~0) |
| Regresión | Rust **41/0** · Flutter 4/4 · sesión completa con latidos y cierre por `max_secs` |

## Para revalidar en campo (LAN)

1. Copiar `Release/` **v0.7.2** a los dos equipos.
2. Anfitrión: *Escuchar (red)* con cualquier calidad; **sin tocar su pantalla**
   → el visor del controlador debe mostrar el escritorio del anfitrión
   (refresco ≈1 s mientras esté estático) y `bytes_recv > 0`.
3. Al interactuar en el anfitrión, el flujo vuelve a los fps configurados.

## Limitaciones

- En pantalla estática el visor refresca a **1 fps** (por diseño: DXGI no
  entrega frames sin cambios; un overlay de cursor/message propio quitaría esta
  limitación — propuesta §5 de `perf-audit.md`).
- El hilo de vídeo se desacopla al terminar la sesión (`stop`): sale en ≤500 ms
  (sin join, acotado).
