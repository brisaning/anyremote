# Gate lote de calidad/robustez — v0.3.0

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/quality-batch` (D-11) · tag `calidad-v0.3.0` |
| Gates | Rust **36 suites / 0 fallos** · codec+`vpx` 2/0 · `flutter analyze` limpio · `flutter test` 1/1 |

## Paso 1 — Robustez

### 1a. Cierre gracioso (bye por stdin / Ctrl-C)
- Los loops activos escuchan `bye` por **stdin** (o fuente inyectable `bye_source` para tests) y **Ctrl-C**: envían `Bye` con gracia de 150 ms y cierran con `closed{reason}` auditado — el par ve un cierre ordenado en lugar de `connection_gone`.
- App: **Detener** y **Detener listen** ahora escriben `bye` por stdin con fallback a kill (3 s).

### 1b. Política `one_way` cableada (patrón OPTION_ONE_WAY_FILE_TRANSFER)
- Flag `--one-way-files` (CLI y `DaemonConfig.one_way_files`): **solo controlador → controlado**. El controlador **rechaza ofertas entrantes** con `FileDone{ok:false,"one_way…"}` + `file_rejected` (evento nuevo) — no mata la sesión.
- `TransferPolicy.one_way` deriva de la config en ambos roles (`file_policy`).

### 1c. Persistencia de parciales (reanudación entre sesiones — gap C5 cerrado)
- **`TransferStore`** en `core/filexfer`: `dir/{id}.part` + `{id}.meta` (size/sha/nombre); se escribe en `accept_offer` y **se appendea por chunk**; `finish` limpia.
- `FileReceiver::new_with_store` restaura el parcial → `next_offset` = bytes ya recibidos.
- **Nuevo mensaje ARSP `FileReady{transfer_id, resume_offset}`** (22): el receptor confirma SIEMPRE tras aceptar (0 = nuevo; >0 = reanudación); el emisor envía chunks **desde ese offset** (flujo de envío del daemon reescrito en consecuencia, tolerando mensajes intercalados).
- Saneamiento reforzado: nombres con **caracteres de control rechazados** (reglas anti-inyección en metadatos).
- Tests: `partials_persist_across_sessions_and_resume` (reanuda byte exacto + limpieza final), `names_with_control_chars_are_rejected`.

## Paso 2 — Calidad de vídeo (M2+)

### 2a. Escalado bilineal
- `scale_frame` pasa de vecino más próximo a **bilineal** (BGRA, clamp 0–255). Test: gradiente 4→2 produce valor interpolado (~43) que el vecino más próximo no puede dar.

### 2b. Control adaptativo de calidad
- **`AdaptState`** (EMA del tiempo de encode, α=0.2) vs presupuesto de frame (`1000/fps`), decisión cada 60 frames:
  - presión → `presion_bitrate` (×1/2 hasta suelo 200 kbps) → `presion_escala` (×0.85, piso 320×180);
  - holgura → `recupera_escala` (a tamaño inicial) → `recupera_bitrate` (×1.5, techo = área×12).
- `VideoEncoder::set_target_bitrate` (default `false`; **VP9 lo aplica vía `reconfigure`** de shiguredo); si el códec no lo soporta → degradación a escala.
- Evento **`video_adapt{reason,size,bps}`** al canal (task → loop → UI, log en la app).
- Tests: `adapt_low_bitrate_first_then_scale_under_pressure` (orden de degradación y recuperación exactos).

### 2c. Medición de render en la UI
- El visor muestra **fps de render** (EMA en vivo): `vídeo WxH · N frames · X fps render` — medición de campo a resolución nativa a solo mirar el indicador.

## Arquitectura de flujo de vídeo actualizada
Task de vídeo → canal `VideoMsg{Frame|Event}` → loop de sesión (con backpressure por descarte, patrones `conn_gone`) → `VideoFrame` ARSP → decodificación del controlador → preview TCP → visor Flutter.

## Limitaciones
- VP9 `ReconfigureParams` solo ajusta tasa (sin cambio de calidad/q-step) — suficiente para el gate.
- `Ctrl-C` en Windows sobre el daemon CLI cierra con Bye; el kill desde la app usa stdin (documentado).
