# Gate C-44 — Autorización del par (F-01)

| Campo | Valor |
|---|---|
| Ciclo | **C-44** (`specs/02` §12.1) |
| Hallazgo | **F-01** de `evidence/security-review-2026-11-19.md` (severidad Alta) |
| Requisito | RNF-07 (autorización del par) |
| Fecha | 2026-11-19 |
| Rama | `feature/c44-peer-authz` |
| Tag | `peer-authz-v0.14.0` |
| Base | `develop` = `42a629c` (C-45 integrado) |

## Problema (semilla)

C2 demuestra posesión de una clave ed25519 autogenerada, pero **nada autorizaba
al par**: cualquier equipo que alcanzara el puerto de escucha obtenía sesión
activa (y con `--allow-input`, control). `peer_trusted` solo informaba, y el
listener **no tenía forma de pedir consentimiento**:
`grep` de diálogos/aceptación en `app/lib` no devolvía nada. Además, el
`PermissionResponse` del par se usaba como autorización del anfitrión.

## Diseño implementado

### Política de admisión (`--accept`)

| Modo | Comportamiento | Uso |
|---|---|---|
| `trusted` (**default**) | Solo pares en `trusted.ids` o pre-autorizados con `--trust ID`. Desconocido → `peer_rejected{peer_unknown}` | CLI / servidores |
| `ask` | Pregunta al anfitrión por stdin (`{"accept":"<ID>","trust":bool}` / `{"reject":"<ID>"}`). Par ya confiado → entra sin preguntar. Sin respuesta en `--accept-timeout` (30 s) → deniega | App (diálogo) |
| `any` | Cualquier par autenticado (comportamiento previo). **Explícito** | Tests / legado |

El nombre `insecure_accept_any()` (solo tests) hace visible el uso inseguro.

### Orden del handshake

1. `recv_validate_auth` (identidad del par).
2. **Admisión** (`admit_peer`): decide el anfitrión.
3. Solo si se acepta: `send_auth` (no se revela la identidad propia a un par
   rechazado), TOFU `remember` (solo pares aceptados) y eventos
   `session_established` / `session_active`.
4. `hello`/`welcome` y concesión de permisos.

### Grant del anfitrión

`host_grant(requested, intent, perms)` = **intersección** entre lo que el
anfitrión ofreció y la intención del par; `granted=false` no concede nada y un
permiso no ofrecido (`fingerprint`) nunca se activa. La auditoría
`permission_granted` registra `"source":"host"`.

### App

`buildListenArgs` pasa `--accept ask`; al recibir `session_pending` la app abre
un diálogo **Sesión entrante** con «Rechazar / Aceptar una vez / Aceptar y
confiar» y escribe la decisión por stdin (`admissionLine`). `peer_rejected` y
`stale_decision` se registran; una decisión tardía no cierra la sesión.

## Cambios

- `core/daemon/src/lib.rs`: `AcceptPolicy`; `DaemonConfig.{accept, trusted_ids,
  accept_timeout_secs}`; `insecure_accept_any()`; `admit_peer`,
  `parse_decision`, `Admission`; `host_grant`; `establish_common` con `ctl`;
  `handle_attempt` respeta `ctl.stop` tras un rechazo; el timeout de
  establecimiento incluye la espera de `ask`; `Idle::Queue` ya no convierte una
  decisión huérfana en `bye`.
- `core/daemon/src/main.rs`: `--accept`, `--trust`, `--accept-timeout` (default
  `trusted`), ayuda y doc de cabecera.
- `app/lib/main.dart`: `--accept ask`, diálogo y `admissionLine`.
- Tests: `core/daemon/tests/daemon_authz_e2e.rs` (nuevo) y
  `insecure_accept_any()` explícito en los e2e previos que ejercitan sesiones
  con pares no confiados (16 archivos).

## Gates (todos verdes)

```bash
cd core && cargo test --workspace        # 167 passed; 0 failed
cargo clippy --workspace --all-targets -- -D warnings   # 0 avisos
cargo fmt --all -- --check               # 0 diffs

cd app && flutter analyze                # No issues found!
flutter test                             # 33/33
```

| Prueba | Qué fija |
|---|---|
| `unknown_peer_rejected_by_default` | `trusted`: par nunca visto → `peer_rejected{peer_unknown}`, sin `session_active`, sin `permission_granted`, sin inyección; el listener sigue vivo |
| `trusted_peer_gets_active` | `--trust <ID>` pre-autoriza, activa la sesión y persiste el par en `trusted.ids` |
| `ask_mode_blocks_until_host_decision` | Sin decisión no hay `session_active`; con `{"accept"}` se activa y la auditoría muestra `permission_granted{source:host}` |
| `ask_mode_rejects_on_host_decision` | `{"reject"}` cierra al cliente, audita `rejected_by_host` y el listener sigue |
| `host_grant_is_intersection_and_never_expands` (unit) | El par no puede ampliar permisos; `granted=false` no concede |
| `admission_decision_parses_only_for_the_pending_peer` (unit) | La decisión se aplica solo al ID pendiente; `{"perm":...}` y `bye` no son decisiones |
| `accept_policy_defaults_to_trusted` (unit) | El default es denegar salvo par confiado |

## Contrato y compatibilidad

- **CLI**: `anyremoted listen --accept ask` (interactivo por stdin) o
  `--trust <ID>` una vez; `--accept any` reproduce el comportamiento anterior.
  Sin `--accept`, el default es **trusted** (deniega desconocidos).
- **App**: siempre `ask` (diálogo). «Aceptar y confiar» deja el par en
  `trusted.ids` y las próximas sesiones de ese ID no vuelven a preguntar.
- **Wire**: sin cambios (no se tocó `arsp.proto`).
- **Peers viejos**: compatibles; el cambio es local al anfitrión.

## Límites

- **Validación de campo pendiente**: dos equipos, (a) rechazo por defecto,
  (b) diálogo + aceptar, (c) «Aceptar y confiar» y reconexión sin diálogo.
- C-44 no cambia el portapapeles ni los archivos: **C-47/C-48** siguen abiertos
  (un par aceptado aún recibe el portapapeles del anfitrión si `--clipboard`).
- **C-46** sigue abierto: la auditoría aún escribe el carácter inyectado.
- El modo `any` queda como superficie de riesgo deliberada: visible en el
  código (`insecure_accept_any`) y en `--accept any`.
