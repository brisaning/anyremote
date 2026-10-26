# Gate ciclo C8 — Consolidación: RNF-01/02/05 + poda + trazabilidad

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/c8-consolidation` (D-11) · tag `ciclo-c8` |
| Suite final | **28 suites ok, 0 fallos** (workspace completo `core/`) |

## RNF-01 Seguridad — barreras verificadas (todas en suite)

| Requisito | Prueba | Ciclo |
|---|---|---|
| Cifrado E2E obligatorio (TLS 1.3, root store; sin verificadores peligrosos) | `e2e_session_loopback` + test `tls_configuration_never_uses_dangerous_verifier` | S0/C8 |
| Autenticación mutua ed25519 + channel binding | `e2e_mutual_auth_with_trust_store` | C2 |
| Rechazo de credenciales inválidas | `auth_rejects_tampered_proof_and_foreign_binding` | C2 |
| Límite de intentos | `auth_gate_limits_attempts` (3 fallos → corte) | C2 |
| Cota de recursos (frame 16 MiB) | `frame_size_limit_is_enforced` | S0 |
| Contención de formatos (portapapeles) | `e2e_clipboard` + `reject_format` | C4 |
| Anti-traversal + integridad de archivos | `e2e_filexfer` + tests `names_with_traversal_*`, `corrupted_content_fails_hash` | C5 |
| **Sin secretos hardcodeados** (barrera fail-closed) | `no_hardcoded_secrets_in_workspace_source` (escanea TODO `core/`: keywords+literales, `.pem/.key/.env` prohibidos, `dangerous verifier` prohibido) | C8 |
| Telemetría solo local (sin egress) | `events_roundtrip_as_jsonl` (archivo local) | C7 |

## RNF-02 Rendimiento — medición final (release, 1280×720, 240 frames)

| Backend | Encode | Decode | Compresión | Estado vs objetivo |
|---|---|---|---|---|
| **VP9** | **37 fps** | 54 fps | 0.005x | ✅ ≥30 fps |
| Deflate | 14 fps | 14 fps | 0.17x | fallback lossless |
| Raw | 387 fps | — | 1.00x | referencia |

Latencia de transporte/códec Raw: 2,58 ms/frame → margen para códec real.

## RNF-05 Auditabilidad
- `specs/04-trazabilidad.md` actualizado: **tabla de ciclos S0…C8 ↔ tags ↔ gates ↔ evidencia**.
- Cada ciclo con evidencia en `evidence/c*-gate.md`; hallazgos H-AD/H-RD en sus inventarios; vault actualizado por ciclo.

## Poda (grado de naturaleza muerta)
- Eliminado trait `CloneKey` de transport (sombreado por el método inherent `clone_key` de pki-types — representación redundante).
- `cargo check --workspace`: **0 warnings**.
- `flutter analyze`: **sin issues** (app, ciclo desktop).

## Limitaciones declaradas (para C8+ / integración)
- Clippy no ejecutado (gate futuro candidato); bench en Windows x64 de este host.
- `one_way` y persistencia de parciales de archivo: cableados en daemon (integración).
