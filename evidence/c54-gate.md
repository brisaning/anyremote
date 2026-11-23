# Gate C-54 — una decisión por par en el daemon (v0.14.5)

**Fecha**: 2026-11-23 · Rama: `hotfix/v0.14.5-accept-flow` · **Ámbito**: solo
`core/**` (+ este archivo). Sin tocar `app/`, `specs/`, `README` ni `VERSION`.

Disparador: `evidence/c52-field-gate-failed.md` (P3) y la traza de campo del
anfitrión del 2026-11-23, donde el diálogo «Sesión entrante de ONDH6IU4RU»
apareció **una vez por intento** de conexión (11:31:00, 11:33:05, 11:37:22,
11:37:38) y un intento murió con `handshake_timeout (45 s)` mientras el usuario
atendía a otro. C-53 ya mitigó esto en la capa `app/` (reenvío de la decisión
reciente); este ciclo lo corrige **en el daemon**, que es donde vive la regla de
admisión.

## Cambio

### A. Una decisión por par (`AdmissionBoard`)

- `DaemonConfig::accept_decision_ttl_secs` (`--accept-decision-ttl`, default
  **60 s**) y `AdmissionBoard` creado por `run_listen` (vive lo que la escucha).
- `AdmitPeer` (`--accept ask`), en este orden:
  1. **Decisión reciente** del par (aceptación ≤ TTL; rechazo ≤ TTL/2, mínimo
     1 s) → se aplica sin preguntar y se emite `session_decision_reused`.
  2. **Pregunta abierta** del mismo par (otra conexión esperando) → no se emite
     otro `session_pending`; se sondea el mapa cada 250 ms hasta la decisión o
     el `--accept-timeout` (fail-closed).
  3. Si no → `session_pending` como siempre; la decisión se **archiva** en el
     mapa. `trust:true` sigue persistiendo el par en `trusted.ids`
     (`TrustStore::remember`, idempotente: una sola entrada por par).
- Una decisión para **otro** par se archiva para él si tiene pregunta abierta
  (preparado para la aceptación concurrente de F6); si no, se ignora como antes.
- Rechazo reutilizado se audita como `rejected_recently` (distinto de
  `rejected_by_host`) para que en campo se sepa por qué **no** hubo diálogo.

### B. Timeouts

- Sin cambios en `handshake_timeout()` (15 s + `--accept-timeout`): un intento
  que reutiliza la decisión entra de inmediato; el que espera la pregunta de
  otro intento queda acotado por `--accept-timeout`, dentro del sobre de 45 s.
- `handshake_timeout` sigue registrándose como intento muerto (`Error` +
  `client_failed` con `detail: handshake_timeout (…)`), no como error del
  usuario.

## Gates (invocación exacta y resultado real)

Toolchain: `stable-x86_64-pc-windows-msvc` (el feature `vpx` no compila con GNU).

```
$ cd core
$ cargo fmt --all --check
fmt exit=0

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.74s
clippy exit=0

$ cargo test --workspace
    54 bloques `test result: ok`, 0 fallos (los 2 `#[ignore]` son
    `scale_probe` y `video_quality_probe`, sondas manuales fuera de este ciclo)
```

Tests que fijan el comportamiento (`core/daemon/tests/daemon_authz_e2e.rs`):

| Test | Qué fija |
|---|---|
| `ask_mode_prompts_once_per_peer` | dos conexiones simultáneas del mismo par → **un** `session_pending`; tras la decisión, **ambas** llegan a `session_active` |
| `ask_mode_reuses_recent_decision` | una conexión nueva dentro de la ventana no pregunta y se activa (evento `session_decision_reused`) |
| `ask_mode_reuses_recent_rejection` | el rechazo se recuerda (ventana corta) y no reabre el diálogo |
| `ask_mode_prompts_for_legacy_seen_peer` | «Aceptar y confiar» sigue persistiendo el par en `trusted.ids` **una sola vez** |
| unit: `admission_board_allows_one_prompt_per_peer` | un solo dueño de la pregunta por par; otro par no se bloquea; la plaza se libera al soltar |
| unit: `admission_board_expires_decisions_after_the_window` | caducidad de la aceptación, ventana de rechazo = TTL/2 (mín. 1 s) y poda del mapa |
| unit: `reused_rejection_is_audited_with_its_own_reason` | motivo de auditoría distinto al reutilizar un rechazo |

```
$ cargo test -p anyremote-daemon --test daemon_authz_e2e
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored
```

## Riesgos / desviaciones del diseño

1. **La aceptación del listener es serial** (`run_listen` atiende una conexión
   hasta cerrar la sesión). Por eso el caso «dos intentos en vuelo comparten una
   pregunta» **no se alcanza hoy** con dos `admit_peer` simultáneos: el segundo
   intento espera en la cola TCP y, al entrar, ya encuentra la decisión en el
   mapa. El test 1 lo demuestra de punta a punta (un solo diálogo, dos sesiones
   activas de forma **secuencial**). La lógica de «pregunta abierta» está
   implementada y probada a nivel de tablero para que F6 (aceptación
   concurrente) no reintroduzca el diálogo múltiple.
2. **Ventana de reutilización**: aceptar una vez autoriza al **mismo par
   autenticado** durante 60 s, no a un único intento. Es lo que pide el ciclo
   (`ask_mode_reuses_recent_decision`), pero conviene tenerlo presente: con
   `--accept-decision-ttl 0/1` el compartimento es más estrecho.
3. **Rechazo reciente manda sobre el atajo «par confiado + control ON»**: si el
   anfitrión rechazó hace <30 s, un par `user` con el control encendido no
   entra hasta que caduque esa ventana. Es deliberado (lo último que dijo el
   anfitrión manda), pero es un cambio de matiz respecto de v0.14.3.
4. **Observación de campo no corregida aquí** (fuera de alcance): el bootstrap
   del certificado que lee el **cliente** tiene 2 s de presupuesto
   (`cert_store::read_bootstrap`). Si el listener tarda más de 2 s en aceptar
   la conexión (p. ej. esperando la decisión de otro intento), ese intento falla
   con `fatal alert: DecodeError` en el TLS posterior. Con el mapa esto es menos
   probable, pero la raíz está en la serialización del listener (F6).
5. El mapa se consulta **antes** del atajo `user_trusted && input_on`: una
   decisión reutilizada (aceptar/rechazar) prevalece sobre él. Está comentado en
   el código.
6. `parse_decision_any` archiva decisiones de otros pares solo si tienen una
   pregunta abierta; una decisión suelta sin sesión pendiente sigue emitiéndose
   como `stale_decision` (sin cambios respecto de C-44).
