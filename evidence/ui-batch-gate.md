# Gate lote UI — v0.4.0 (resolución nativa + perfiles en la app)

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/ui-batch` (D-11) · tag `ui-v0.4.0` |
| Gates | daemon 7/7 (incl. `resolve_native`) · `flutter analyze` limpio · `flutter test` 1/1 |

## Entregados

1. **Visor a resolución nativa**: `--video-size native|auto|WxH` (sentinela `(0,0)`); la tarea de vídeo fija las dimensiones con el **primer frame** del capturer real (DXGI nativo) y crea el control adaptativo con el área real; en modo Test sin escritorio → 1280x720. El botón *Escuchar (red)* de la app ahora usa `--video-size native` (transmite el escritorio a su resolución real; el adaptativo regula bitrate/escala si hace falta).
   - Test `resolve_native_uses_first_frame_or_fixed_size` (explícito manda / nativa real / fallback Test).
2. **Perfiles (C6) en la UI**: `initState` carga `anyremoted profiles --list` (JSONL de aliases) → **Dropdown de perfiles** + botón *Actualizar perfiles*; seleccionar un alias rellena el destino (el daemon lo resuelve con `--profile` — la normalización de la app ya detecta alias sin ':'). Sin daemon → traza explícita.
   - Test: control presente, tap sin daemon → traza `perfiles: anyremoted.exe no encontrado`.

## Para probar
```
anyremoted profiles --add casa,192.168.6.152,21118
app → refresca perfiles → elige "casa" en el desplegable → Conectar
Escuchar (red) → el visor muestra el escritorio a resolución NATIVA del anfitrión
```
