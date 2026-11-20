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

## Fix de campo (2026-11-19, v0.14.1) — el diálogo no aparecía

**Síntoma (reporte del usuario):** el cliente veía la pantalla del anfitrión y
en el anfitrión **no** aparecía ningún diálogo de autorización.

**Diagnóstico (datos reales).** La auditoría del anfitrión mostraba:

```json
{"kind":"session_established","detail":{"accept":"ask","peer":"ONDH6IU4RU","peer_trusted":true,...}}
```

y `%APPDATA%\anyremote\trusted.ids` (del **2026-11-15**, escrito por el TOFU
previo a C-44, que recordaba a **cualquier** par autenticado) contenía ese ID.
`admit_peer` en `ask` auto-aceptaba `peer_id.trusted`, es decir, trataba como
autorización del usuario un registro que se había creado **sin consentimiento**.
El diálogo de la app funcionaba; el daemon nunca lo pidió.

**Fix.**

- `TrustOrigin { User, Seen }` en `anyremote-identity`: solo `User` acredita una
  decisión explícita (`--trust` o «Aceptar y confiar»).
- `trusted.ids` v2: `ID=hex:user` / `ID=hex:seen`; **sin sufijo = `seen`**
  (migración de las entradas heredadas, sin perderlas).
- `admit_peer` en `ask`/`trusted` exige `is_user_trusted`; `--accept any`
  recuerda como `seen` (nunca degrada un `user`).
- Auditoría `session_established` añade `peer_trust` (estado previo) y
  `persist` (qué se guardó).
- App: el diálogo se muestra también si el listener es externo (solo falla al
  enviar la decisión, con aviso); test de widget que inyecta `session_pending`
  y verifica el diálogo.

**Gates.** `ask_mode_prompts_for_legacy_seen_peer` (nuevo e2e: `trusted.ids` v1
→ `session_pending` → aceptar+confiar → `:user`), unitario
`legacy_entries_are_seen_and_only_user_trust_authorizes`, widget
`C-44: session_pending abre el diálogo`. Rust **169/0** · clippy 0 · fmt 0 ·
Flutter **34/34** · analyze limpio.

**Efecto para el usuario.** Las entradas anteriores a C-44 se conservan pero
**no autorizan**: la primera conexión de cada equipo conocido desde entonces
volverá a preguntar; «Aceptar y confiar» migra esa entrada a `user` y las
siguientes no preguntan.

## Integración C-44/C-45 (2026-11-20, v0.14.2) — aceptar no daba control

**Síntoma (reporte del usuario, segunda vuelta):** el diálogo ya aparecía y la
sesión se abría, pero el cliente **solo veía** la pantalla; en el registro del
anfitrión: «entrada denegada en el anfitrión: control remoto apagado».

**Causa.** C-44 autoriza la **sesión**; C-45 exige una decisión aparte para la
**entrada**. El diálogo no incluía esa decisión y el toggle «Control remoto»
seguía apagado, así que el daemon aplicaba `{"perm":{"input":false}}` y
denegaba cada evento (`input_on:false`, 608 denegaciones en la sesión del
20/11). El comportamiento era el diseñado por C-45, pero la UX era incoherente:
«Aceptar» parecía conceder control y no lo hacía.

**Fix (app).**
- El diálogo **Sesión entrante** incluye la casilla
  **«Permitir control remoto (teclado y ratón)»**, marcada por defecto y
  visible: aceptar es la decisión explícita de ambos (conexión + control);
  desmarcarla deja la sesión en «solo ver».
- Los permisos se envían **antes** de la decisión (`{"perm":...}` queda
  encolado y se aplica al abrir la sesión) → el primer clic del cliente ya
  funciona.
- Al aceptar sin control se avisa en el registro; el chip «solo ver» sigue
  apareciendo si se apaga después.
- Dedupe del aviso de denegación en el anfitrión (antes: una línea por cada
  movimiento de ratón); se rearma al activar «Control remoto».

**Gates.** Widget `C-44: session_pending abre el diálogo y permite control`
(acepta → toggle encendido) y `C-45: se puede aceptar en «solo ver»`
(desmarcar → toggle apagado); analyze limpio; Flutter **35/35**. Rust sin
cambios: **169/0**. Binario **0.14.2** verificado (`FileVersion 0.14.2+1`,
`anyremoted 0.14.2`).

### Encadenado (v0.14.3): el estado del toggle no sobrevivía entre sesiones

Con la 0.14.2, la **segunda** conexión de un par `user` se auto-aceptaba sin
diálogo (correcto) pero `DynamicPerms::new` volvía a arrancar en `false`: la
sesión se abría en «solo ver» sin que el anfitrión pudiera decidir. El toggle
de la UI seguía marcado, pero el daemon lo olvidaba al cerrar la sesión.

**Fix.** El estado de entrada pasa a ser un `Arc<AtomicBool>` en
`DaemonConfig` (`input_state`, con `input_on()`/`set_input_on()`): persiste
mientras el listener vive y lo actualizan `--input-on` y cada
`{"perm":{"input":...}}` (incluidos los encolados al abrir sesión). Además,
`ask` solo auto-acepta a un par `user` si el control está **encendido**; si
está apagado vuelve a preguntar (el diálogo es donde se concede).

**Gates.** `input_state_persists_between_sessions_and_needs_capability` (unit),
`ask_mode_auto_accepts_user_trusted_peer_when_control_is_on` (sin
`session_pending`), `ask_mode_prompts_trusted_peer_when_control_is_off` (con
`session_pending`). Rust **172/0** · clippy 0 · fmt 0 · Flutter **35/35**.
Binario **0.14.3** (`FileVersion 0.14.3+1`, `anyremoted 0.14.3`).
