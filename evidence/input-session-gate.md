# Gate RF-02a — entrada en sesión (teclado/ratón remoto) · v0.5.0

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/input-session` (D-11) · tag `entrada-v0.5.0` |
| Gates | Rust **37 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` **3/3** |
| Artefactos | `Release/anyremote_app.exe` **0.5.0** + `Release/anyremoted.exe` **0.5.0** |

## Qué se entregó

1. **Wire ARSP (`core/transport`)** — nuevo campo `Key.ch` (carácter UTF-8 de un solo
   codepoint) en `arsp.proto` con la tabla de códigos documentada (0 = carácter · 1..11
   navegación · 20..22 modificadores). El **dueño del mapeo** es
   `anyremote-transport::arsp::input_event_from_wire` (proto → `InputEvent`): rechaza códigos
   desconocidos, caracteres vacíos o multibyte y recorta coordenadas al rango `u16` del modelo.
   El gate C3 `e2e_video` reutiliza esa función (se eliminó la copia local `map_input`).
2. **Daemon receptor (`listen`)** — D-07 por defecto:
   - `DaemonConfig.allow_input` (**false sin `--allow-input`**) + `input_backend`
     (`Windows` = `SendInput` en producción · `Recording` en tests, sin tocar el sistema).
   - `Established::Active(ch, granted)` arrastra la **concesión efectiva** del handshake;
     en `active_controlled` cada evento se valida: sin flag, sin el permiso
     (`keyboard` para teclas, `mouse` para puntero/botón/rueda) o con mapeo no soportado →
     `input_denied` + auditoría **`InputDenied`**; inyectado → `input_injected` + auditoría
     **`InputInjected`** (con el evento en Debug). Los errores de inyección no matan la sesión.
   - Nuevos tipos de auditoría `InputInjected`/`InputDenied` en `anyremote-telemetry`.
3. **Daemon emisor (`connect`)** — stdin: las líneas que empiezan por `{` se interpretan como
   `{"input":{key|pointer|button}}` → `parse_input_line()` valida contra el mapeo owner y
   reenvía `Payload::InputEvent`; el resto de líneas sigue siendo el cierre (`bye`).
   Buffer del canal de stdin subido a 64 (el visor escribe eventos con frecuencia).
4. **App (Flutter)**:
   - Visor remoto con **`Listener` crudo**: `pointer` absoluto en píxeles remotos (mapeo
     lineal local→frame: el `AspectRatio` replica el aspecto del frame y `RawImage` es
     `BoxFit.contain`), **botón mantenido durante el arrastre** (down…moves…up, izq/der/medio)
     y rueda (`button.wheel`, una muesca).
   - `Focus` del visor → teclado: `keyCodeFor()` (tabla espejo del wire) o carácter `ch`,
     con emparejamiento keyup/carácter en `_heldChars`; `Ctrl+C` llega como modificador + tecla.
   - Contrato de stdin: `{"input":{...}}` línea a línea al proceso `connect` (`_sendInput`),
     con traza si el pipe se cierra.
   - **Casilla "Aceptar control remoto"** (D-07, **desmarcada por defecto**): solo entonces el
     botón *Escuchar (red)* añade `--allow-input`; el registro informa del estado elegido.
   - Contador `entrada: N` en la cabecera del visor; `input_denied`/`input_error` se trazan en
     el registro del anfitrión; el registro quedó **anclado al final** (`reverse`) para que la
     última línea sea siempre visible.

## Tests nuevos

| Suite | Qué cubre |
|---|---|
| `transport::arsp::input_wire_mapping_covers_keys_pointer_buttons` | tabla wire→modelo completa (carácter, navegación, modificador, puntero abs/rel con recorte, botón, rueda) + oneof vacío + códigos/caracteres inválidos |
| `daemon_input_e2e::input_partial_grant_injects_keyboard_and_denies_pointer` | concesión `["keyboard"]`: tecla **inyectada**, puntero **denegado** (`input_denied` con `perm:mouse`), `bye` posterior cierra, auditoría con `InputInjected`+`InputDenied` |
| `daemon_input_e2e::input_denied_by_default_without_allow_input` | sin flag: todo **denegado** (`allow_input:false`), cero `InputInjected` en auditoría, la sesión sigue hasta `bye` |
| `daemon_input_e2e::input_full_grant_injects_keyboard_pointer_button_scroll` | concesión completa: **4 inyecciones** (tecla, puntero, botón, rueda) con backend `Recording` y cero denegaciones |
| `widget_test` (Flutter) | casilla desactivada por defecto y conmutable · `keyCodeFor` = tabla del wire ARSP |

## Para probar en campo

```powershell
# anfitrión (comparte pantalla y acepta control):
anyremoted listen --bind 0.0.0.0:21118 --video on --video-size native --clipboard win --allow-input
# o con la app: marca "Aceptar control remoto" y pulsa "Escuchar (red)"
# controlador: app → destino → en el visor: mover/clic/arrastre/rueda + teclado
```

- Sin `--allow-input` (o sin marcar la casilla) el anfitrión emite `input_denied`
  (registro + `audit/*.jsonl`) y **no se inyecta nada**.
- Denegación por permiso parcial: concede solo `--grant keyboard` y el puntero se deniega.

## Limitaciones conocidas

- **Sin acuse al controlador**: `input_injected`/`input_denied` los emite el anfitrión (su
  registro/auditoría); el visor solo cuenta lo que envía (`entrada: N`).
- La tabla de códigos no cubre F1..F12, Insert, PageUp/Down ni caps/num lock.
- Coordenadas en píxeles de la resolución **nativa** del anfitrión (derivada del frame de vídeo).
