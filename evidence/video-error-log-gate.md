# Gate C-25 — `video_error` con detalle en el registro (v0.12.8)

**Fecha**: 2026-11-11 · **Rama**: `feature/video-error-log` · Origen: revisión de los logs de campo del2026-11-11 (4 sesiones WAN,3 modos + audio)

## Qué cambia (solo la app)

El daemon ya emitía `video_error` **con `detail`** (y lo grababa en la
auditoría), pero el `switch` de eventos de la app no tenía caso → el
registro mostraba solo el **tipo genérico** `[daemon] video_error` y el
detalle (`Decode("openh264: native:18 …")`) se perdía para el diagnóstico
de campo. Ahora:

```
[daemon] vídeo: Decode("openh264: Error { native: 18, … }")
```

En las pruebas de hoy aparecieron `native:2` (**frame de referencia
perdido** — pérdida en el enlace) y `native:18` (**SPS/PPS incompatible +
ref lost** — cambio de resolución en caliente con `red_escala`):
transitorios, no matan la sesión (el `seq_hdr` antepuesto a cada frame
recupera el decoder) — pero sin el detalle eran indistinguibles en el
registro.

## Tests (+1 →23)

- `C-25: video_error registra el detalle de decode` — costura nueva
  `debugHandleEvent(linea)` inyecta el evento JSON → el detalle
  (`openh264: native:18`) **aparece en el registro** y el tipo genérico
  **no**.

## Gates (2026-11-11)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **23/23** |

## Contexto de campo (sesiones del2026-11-11, v0.12.7)

Las4 sesiones por relay WAN fueron **exitosas en los3 modos** (720p×2,
1080p, Nativa→1920×1080) **y con audio** (`audio_sent:536` /
`audio_recv:448`), con control (303+329 inyecciones), `bye` limpio y la
cascada de adaptación **ya no dejando el vídeo en0** (fix C-24 validado:
60–121 frames y1,3–2,9 MB por sesión). Pendiente solo de anotar el
`fps render` para la decisión `Texture`.
