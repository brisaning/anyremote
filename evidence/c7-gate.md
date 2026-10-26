# Gate ciclo C7 — RF-05: registro de sesiones y diagnóstico

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/c7-telemetry` (D-11) · tag `ciclo-c7` |
| Semilla | H-RD-10 **parcial** (solo stats de audio en RustDesk) → **gap RF-05 aceptado → diseño propio** (D-10) |
| Suite | telemetry 3/3 · `e2e_audit` 1/1 · **total 27 suites, 0 fallos** |

## Entregados

- **`core/telemetry`**:
  - `AuditEvent` JSONL (spec §9: `ts_unix_ms`, `kind`, `session_id`, `detail`) con 10 tipos (`session_started/established`, `permission_*`, `heartbeat_summary`, `file_transfer`, `clipboard_sync`, `profile_applied`, `error`, `disconnected`).
  - `SessionLog`: archivo `audit-<unix_ms>.jsonl` (create_new), **flush por evento** (la traza manda), `read_all` con errores explícitos (`MalformedLine` con la línea culpable).
  - `SessionReport` de diagnóstico: RTT min/último, heartbeats, frames/bytes enviados-recibidos, archivos — `finish()` + `disconnect_event()` (cierre accionable en una línea).
- **Gate e2e** `e2e_audit_events_and_diagnostic_report` (sesión viva real): secuencia exacta `SessionStarted → SessionEstablished → HeartbeatSummary → FileTransfer → Disconnected`, session_id coherente, timestamps monótonos, RTT agregado desde `LinkStats` (C1), **transferencia real con hash** en el detalle, y cierre con `heartbeats_ok=4`, `rtt_min`, `files_transferred=1`.

## Limitaciones
- Log por apertura de archivo (rotación por sesión); sin envío a red externa (telemetría local por diseño — RNF-01).
- El cableado de eventos en el daemon vivo llega en la integración (los hooks probados aquí son los que usará `anyremoted`).
