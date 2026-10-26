# Gate ciclo C5 — RF-02c: transferencia de archivos con integridad y reanudación

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/c5-filexfer` (norma D-11) |
| Semilla | H-RD-08 (`file_transfer` en conexión + `OPTION_ENABLE/MAX_FILES/ONE_WAY`) — implementación **clean-room propia** (D-01) |
| Suite | **26 suites ok, 0 fallos** — filexfer 6/6 unit · `e2e_filexfer` 1/1 · resto intacto |

## Entregados

- **`core/filexfer`**:
  - **`TransferPolicy`** (patrón OPTION_*): `enabled` (habilitación maestra), `max_files_per_batch`, `max_bytes_per_file`, `one_way` — con validación de lote en emisor Y receptor (defensa en profundidad).
  - **`FileSender`**: `offer()` (oferta con digest SHA-256 real + sanitización) y `chunks_from(offset)` — **reanudación** desde cualquier offset.
  - **`FileReceiver`**: `accept_offer` (política + nombre + id re-oferta idempotente), `write_chunk` (**offset contiguo obligatorio; offset adelantado reporta `expected`** — base de reanudación), `finish` (**verifica SHA-256 → `HashMismatch`**) y `finish_to_file` (**escritura a disco real con nombre saneado — sin path traversal**).
  - `sanitize_name`: rechaza `/ \ : .. . vacío >255` (regla anti-traversal).
- **ARSP**: `FileOffer{transfer_id,name,size,sha256}` (19), `FileChunk{transfer_id,offset,data}` (20), `FileDone{transfer_id,ok,detail}` (21) con `detail` para `resume:{offset}` y hash hex; conversiones wire con validación (digest=32 bytes, nombre saneado → `TransportError::Wire`, nueva variante).
- **Gate e2e** `e2e_filexfer_hash_integrity_and_resume`:
  1. Oferta de 100.000 B aceptada bajo política (max 128 KiB).
  2. Chunk 0 OK → **hueco deliberado** → `Offset{expected=32768}` → `FileDone{ok:false,"resume:32768"}` → emisor reenvía desde el offset confirmado.
  3. Receptor completa, **escribe archivo real a disco temp** y verifica SHA-256 = original → `FileDone{ok:true,hash}`.
  4. **Límite del receptor**: oferta de 200.000 B > 128 KiB → rechazada con `TooLarge` (detalles verificados).
  5. Nombre inválido (`../evil.bin`) rechazado **en origen** por `offer()`.

## Limitaciones registradas

- Ensamblaje en memoria con cota + escritura final a disco (streaming a archivo en parcialidades = futuro); chunk 32 KiB < cota de frame ARSP 16 MiB.
- `one_way` queda como fuente de política para el cableado del daemon (unit verifica la bandera; la compuerta direccional vive en C7/C8).
- Reanudación offset-based sin persistencia entre sesiones del daemon (persistencia de parciales = futuro).
