# Revisión de seguridad — C-57 (admisión por par) en el hotfix 0.14.5

| Campo | Valor |
|---|---|
| Fecha | 2026-11-23 |
| Revisor | `anyremote-security` (estática, sin ejecución) |
| Alcance | `admit_peer`/`AdmissionBoard` (`core/daemon/src/lib.rs`), diálogo y memoria de decisión (`app/lib/main.dart`), contrato `session_pending`/`session_decision_reused`, `trusted.ids` |
| Fuera de alcance | `capture`, `codec`, `filexfer`, preview (F-04/F-06/F-07 siguen abiertos en C-48/C-49/C-50) |
| Resultado | **0 críticos · 0 altos · 4 low · 3 informativos** → publicable |

## Controles verificados que siguen en pie

| Control | Evidencia |
|---|---|
| El TOFU `seen` **no** autoriza | `lib.rs` usa `is_user_trusted` (solo origen `User`); el heredado se carga como `Seen` |
| La clave del tablero es el `node_id` **derivado** de la clave verificada, no el declarado | `auth.rs:131-148` (`derived != claimed` → fallo) |
| `trust:true` persiste solo con decisión del anfitrión, e idempotente | `TrustStore::remember` + e2e `ask_mode_prompts_for_legacy_seen_peer` |
| Un rechazo reciente **manda** sobre el atajo de par confiado | `lookup` va antes del atajo; e2e `ask_mode_reuses_recent_rejection` |
| Una decisión suelta no se aplica a ninguna conexión futura | `Idle::Queue` solo encola `"perm"`; el resto → `stale_decision` |
| La intersección de permisos sigue siendo del anfitrión | `host_grant(requested, …)` |
| Inyectar sigue exigiendo `allow_input` **y** `input_on` **y** el grant | `lib.rs:2222` |
| Apagar el control sigue funcionando (el reenvío no reenvía `input:true`) | `app/lib/main.dart` |
| Revocar la admisión es posible (`bye` → el listener termina → el tablero muere) | `run_listen` + `_stopLocalListen` |
| La decisión no se filtra al par | la rama denegada solo registra y emite en local |
| Envenenamiento de mutex no propaga pánico al listener | `unwrap_or_else(into_inner)` |
| `PromptClaim` libera siempre la plaza | `Drop`; las salidas tempranas ocurren antes del `claim` |

## Hallazgos y qué se hizo

| id | Sev | Resumen | Decisión |
|---|---|---|---|
| **S-01** | low | «Aceptar» deja el toggle global de control encendido, y ese mismo flag es la condición del atajo que admite **sin diálogo** a cualquier par `user` | **Aceptado para 0.14.5** (el atajo es pre-existente de C-44 y tiene test propio); se documenta el alcance y se programará quitarlo (o abrirlo en «solo ver») en el siguiente ciclo |
| **S-02** | low | «Aceptar una vez» autoriza 60 s de sesiones del mismo par, no un intento | Registrado: el alcance es `--accept-decision-ttl` (60 s) y la ventana **no** es extensible por el par (se evalúa al leer, no se re-sella). Fix de raíz en **C-56** (`session_pending` con `attempt`) |
| **S-03** | low | El JSONL no distingue una sesión admitida por diálogo de una reutilizada | **Arreglado en 0.14.5**: `session_established` incluye `"admission": "host" \| "reused" \| "trusted_shortcut" \| "any"` |
| **S-04** | low | La app recuerda el rechazo 60 s y el daemon 30 s → hasta ~90 s de denegación silenciosa | **Arreglado en 0.14.5**: la app ya **no** recuerda rechazos (el daemon los cachea con su ventana correcta) |
| **S-05** | info | La rama «decisión para otro par» es un no-op hoy (listener serial) y fallaría al llegar F6 | **Obligatorio antes de F6**: el dueño del `claim` debe sondear también `board.lookup` |
| **S-06** | info | `session_decision_reused` es un evento **nuevo** (la evidencia afirmaba lo contrario) y el id «C-54» estaba duplicado (Caps Lock) | **Arreglado en 0.14.5**: la app tiene ya su `case` (antes caía en el `default` mudo) y el ciclo de admisión se renombra a **C-57** en `specs/02`, `specs/04`, código y evidencia |
| **S-07** | info | `ctl.pending` (permisos) no está acotado a la sesión que la originó | Sin escalada (la sesión siguiente sigue exigiendo admisión); a resolver al aplicar los permisos en la admisión (F6) |

## Condiciones de registro (cumplidas)

1. `session_decision_reused` **es** un evento nuevo del contrato daemon→app y la app
   lo trata (log en una línea).
2. El ciclo de admisión es **C-57**; **C-54** sigue siendo Caps Lock.
3. «Aceptar una vez» significa «aceptar durante `--accept-decision-ttl` (60 s)»,
   no «un intento».

## Huecos que quedan (con su ciclo)

1. Consentimiento degradado a ventana por clave → **C-56**.
2. Atajo `user + control ON` alcanzable por cualquier «aceptar» (S-01) → siguiente
   ciclo de la línea de admisión (C-57bis).
3. `session_pending` sin `attempt`/`session_id` → **C-56**.
4. Frecuencia de diálogos por origen sin límite → pre-existente (C-44), agravará
   C-56.
5. **No observable en esta revisión**: el comportamiento en campo (si el anfitrión
   tenía pares `user` heredados de 0.14.4, si el toggle quedó encendido entre
   comparticiones). Requiere el guion de `evidence/accept-control-gate.md` **más**
   un paso nuevo: *tras aceptar a A, conectar desde B (previamente «confiar») y
   comprobar si aparece el diálogo* (hoy no aparecerá).
