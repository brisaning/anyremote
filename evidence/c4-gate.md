# Gate ciclo C4 — RF-02b: portapapeles bidireccional con contención de formatos

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/c4-clipboard` (norma D-11) |
| Semilla | H-RD-07 (`libs/clipboard` por plataforma) — implementación **clean-room propia** sin copia del cliprdr (D-01) |
| Suite | **0 fallos** — clipboard 6/6 unit · `e2e_clipboard` 1/1 · resto intacto |

## Entregados

- **`core/clipboard`**:
  - `ClipboardFormat::Text` como **único formato admitido** (contención v0) + `reject_format` en la frontera.
  - **`ClipboardSyncEngine<B>`**: `detect_local_change` (solo envía cambios reales; suprime eco de lo recibido y duplicados de lo enviado) y `apply_remote` (rechaza formatos no admitidos **sin tocar el backend**, idempotente ante duplicados) + `SyncStats` (sent/applied/rejected_format/echo_suppressed).
  - `MemoryClipboard` (tests/gate) y **`WinClipboard`** real: CF_UNICODETEXT vía `OpenClipboard/GetClipboardData/SetClipboardData/GlobalAlloc` (lectura segura; escritura solo con opt-in `ANYREMOTE_TEST_CLIPBOARD=1` para no pisar el portapapeles en tests automáticos — respetando D-07).
- **ARSP**: `ClipboardSync{revision, format, utf8}` (envelope 18) + `ClipboardFormatEnum{CLIP_TEXT}`; conversiones `clipboard_to_wire` / `clipboard_from_wire` con **validación de formato** (formato ≠ 0 → `TransportError::Auth`).
- **Gate e2e** `e2e_clipboard_bidirectional_with_format_containment`:
  1. Saludo controlado → controlador aplica (bidireccionalidad inicial).
  2. Cambio del controlador → controlado aplica; su siguiente poll NO reenvía (eco suprimido, stats verificados).
  3. Cambio del controlado → controlador aplica: ambos lados coinciden en `"respuesta del controlado"`.
  4. **Contención**: mensaje con `format=7` (no texto) rechazado en la frontera wire — backend intacto verificado; rechazo local con `format=9` también.
  5. Idempotencia: reenvío del mismo texto no re-aplica.

## Limitaciones registradas

- Solo texto en v0 (imágenes/HTML = rechazo explícito) — expansión de formatos futura con registro de política.
- Escritura al portapapeles real del sistema: prueba manual opt-in (`ANYREMOTE_TEST_CLIPBOARD=1 cargo test -p anyremote-clipboard`), no en suite automática.
- La sincronización vive en el motor + wire; el cableado a sesión viva llega con la integración daemon (C7/C8).
