# Gate C-2 — permisos dinámicos: control / audio del sistema / micrófono · v0.7.0

| Campo | Valor |
|---|---|
| Fecha | 2026-10-27 |
| Rama | `feature/dynamic-perms` (D-11) · tag `perm-dinamicas-v0.7.0` |
| Gates | Rust **41 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` **4/4** |
| Artefactos | `Release/anyremote_app.exe` **0.7.0** + `Release/anyremoted.exe` **0.7.0** |

## Requisito del usuario

1. Añadir **compartir micrófono** (junto al audio del sistema ya existente; el
   usuario aclaró que en esta app el micrófono es más importante que el audio).
2. Que las tres opciones — **control remoto, audio, micrófono** — sean
   **dinámicas**: el anfitrión puede activarlas/desactivarlas **en cualquier
   momento durante la conexión**, sin reconectar.

## Diseño (sin cambios de protocolo)

| Pieza | Detalle |
|---|---|
| **Capacidad vs. toggle** | Los flags de arranque (`--allow-input`, `--audio on`, `--audio-source system,mic`) siguen siendo la **capacidad** (techo D-07 junto al grant del handshake). El **estado real** lo fija el anfitrión con un comando por stdin: `{"perm":{"input":bool,"audio":bool,"mic":bool}}` (idempotente, los tres campos siempre). |
| **Aplicación en caliente** | El daemon `listen` lo procesa en el select de sesión: cambia el toggle de entrada, y **(re)arranca o detiene la captura** de audio si las fuentes cambian (`audio_start` / `audio_stopped` / `permissions_updated`). |
| **Sin carrera inicial** | La UI escribe el comando al arrancar el listener; el daemon lo **drena antes del primer evento** de la sesión → el estado inicial se aplica antes de que llegue la primera entrada del controlador. |
| **Reglas efectivas** | Entrada: `toggle ∧ grant(handshake)` (el techo del controlador se respeta). Audio: fuentes activas ∧ (`grant` ∨ activado en caliente — compartir tu audio/micrófono es decisión de quien comparte). |
| **Controlador** | El reproductor queda **preparado** aunque al inicio no haya concesión (`audio_denied` informativo) → si el anfitrión activa el audio en caliente, suena. |
| **Mezcla de fuentes** | Con audio + micrófono a la vez, el hilo de captura abre los dos dispositivos, convierte cada uno a mono 48 kHz y **suma por muestras** (mínimo disponible, saturación); una fuente muda no bloquea a la otra; deriva de relojes acotada a ~80 ms (recorte del exceso). |
| **Degradación** | Si falta un dispositivo (p. ej. PC sin micrófono) se avisa (`audio_error` con la fuente) y **sigue con el resto**; solo falla si no abre ninguna. |

## Cambios

- `core/audio`: `AudioCapture::start(mode, sources: &[AudioSource])` multi-fuente,
  `mix_available()` (mezclador puro con tests), `AudioCapture::warnings()`.
- `core/daemon`: `PermSpec`/`parse_perm_line`, `DynamicPerms` (estado + apply),
  `spawn_audio_capture`, drenaje inicial + `pending_bye`, regla de entrada con
  `perms.input_on`, reproductor del controlador sin condición de grant inicial.
  `--audio-source system|mic|system,mic` (multi).
- `app`: **tres casillas dinámicas** — *Control remoto*, *Audio del sistema*,
  *Micrófono* — siempre activas; cada cambio escribe `{"perm":...}` por stdin
  (y se envía el estado inicial al lanzar el listener). Trazas
  `permissions_updated` / `audio_stopped` / `audio_denied` con detalle.

## Tests nuevos

| Suite | Qué cubre |
|---|---|
| `audio::tests::mix_available_sums_skips_empty_and_saturates` | mezclador: suma por muestras, fuente vacía no bloquea, saturación i16, sin datos → nada |
| `audio::tests::multi_source_capture_starts_in_test_mode` | captura con `&[System, Microphone]` arranca en modo `Test` sin warnings |
| `daemon_dynperm_e2e::input_toggles_at_runtime` | línea temporal inyectada→apagada→encendida: **2 `input_injected` + 1 `input_denied` + 2 `permissions_updated`** con márgenes de ~500 ms |
| `daemon_dynperm_e2e::audio_toggles_at_runtime_and_reports_source` | captura apagada y reactivada (`audio_stopped` + 2º `audio_start`), fuente `mic` reportada, `audio_recv > 0` y `audio_stats` en el controlador |
| `widget_test` (Flutter) | tres casillas desactivadas por defecto (D-07) y conmutables de forma independiente |
| Regresión | audio 3/3 (el caso "anfitrión no comparte" ahora espera reproductor preparado + 0 frames) · entrada 3/3 · **Rust total 41/0** |

## Para probar en campo

```powershell
# anfitrión (app): Escuchar (red) → durante la sesión marca/desmarca:
#   "Control remoto" / "Audio del sistema" / "Micrófono"
# → en el registro: "[daemon] permisos actualizados: control activado · audio inactivo · mic activo"
# y el controlador nota el cambio sin reconectar (entrada y audio).
# CLI equivalente:
anyremoted listen --bind 0.0.0.0:21118 --audio on --audio-source system,mic --allow-input
# (stdin) {"perm":{"input":true,"audio":false,"mic":true}}
```

## Limitaciones conocidas

- Los flags de arranque siguen siendo el techo (capacidad): sin `--allow-input`
  o sin `--audio on`, el toggle en caliente no puede excederlos.
- Mezcla con deriva de reloj ≤80 ms entre fuentes (recorte del exceso).
- El controlador no recibe acuse de los toggles (solo el anfitrión los audita).
