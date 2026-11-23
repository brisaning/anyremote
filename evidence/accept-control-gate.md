# Gate C-53 — aceptar una sesión = conceder control (v0.14.5)

**Fecha**: 2026-11-23 · Rama: `hotfix/v0.14.5-accept-flow` · Disparador:
`evidence/c52-field-gate-failed.md` (P3: el diálogo «Sesión entrante» reaparece
por cada intento QUIC → TCP → relay) y el reporte de uso: con la casilla
«Permitir control remoto» **desmarcada** la sesión quedaba en «solo ver» —
244 `input_injected` en el anfitrión que el usuario no percibía (no podía ni
seleccionar ni mover una ventana).

**Ámbito**: solo `app/**`. Sin cambios en `core/`, `specs/` ni `README`.

## Cambio

### A. Aceptar = control (se retira la casilla del diálogo)

- El diálogo deja de tener `CheckboxListTile` y queda con tres acciones:
  «Rechazar», «Aceptar una vez», «Aceptar y confiar».
- Al aceptar (ambas variantes): `_acceptRemoteControl = true` y se escribe
  **primero** `{"perm":{"input":true,"audio":…,"mic":…}}` y **después** la línea
  de decisión (`admissionLine`; `trust:true` se conserva para «Aceptar y
  confiar»). El daemon encola los `{"perm":…}` mientras espera la decisión, así
  que el permiso ya está aplicado al abrir la sesión.
- Al rechazar no se envía ningún `{"perm":…}` ni se toca el toggle.
- El aviso y el chip «solo ver» se mantienen para cuando el anfitrión **apaga**
  el control después (toggle «Control remoto»).

### B. Una decisión por par (el diálogo no reaparece)

- `Map<String, ({String choice, DateTime at})>` con ventana
  `kAdmissionDecisionWindow = 60 s`.
- `session_pending` de un par con decisión reciente → se **reenvía** la decisión
  al daemon sin mostrar diálogo. En el reenvío va **solo** la línea de decisión:
  repetir `input:true` reactivaría un control que el anfitrión hubiera apagado
  tras aceptar (los toggles ya están sincronizados por `_sendPerms`).
- Diálogo ya abierto → el evento se ignora (no se apilan diálogos).
- «Compartir escritorio» limpia el mapa: un `listen` nuevo es un contexto de
  consentimiento nuevo.

## Gates (invocación exacta y resultado real)

Ejecutados en el host de desarrollo con el SDK fijado por CI
(`C:\proyectos\tools\flutter\flutter`, **Flutter 3.47.8 · Dart 3.13.7**):

```
$ cd app
$ flutter analyze
Analyzing app...
No issues found! (ran in 6.6s)

$ flutter test
00:15 +35: All tests passed!
```

Tests de `app/test/widget_test.dart` relevantes:

| Test | Qué fija |
|---|---|
| `C-44: la app pregunta antes de aceptar y decide por stdin` | `--accept ask`; `admissionLine` para accept/trust/reject y degradación a rechazo; **`admissionWrites`**: el `{"perm":{"input":true,…}}` va antes de la decisión, `trust` conserva `trust:true` y **rechazar no envía permiso** |
| `C-44/C-53: accept_implies_control (sin casilla en el diálogo)` | `session_pending` → diálogo **sin** «Permitir control remoto» ni `CheckboxListTile`; tres acciones; «Aceptar una vez» deja el checkbox «Control remoto» de la página **marcado** |
| `C-53: no_second_dialog_for_same_peer` | dos `session_pending` del mismo par → un solo diálogo; tras decidir, un tercer evento **no** lo reabre y el registro dice «se reenvía sin volver a preguntar» |
| `C-45: compartir escritorio no concede control remoto (opt-in)` | regresión: `shareSummary` sigue en «solo ver» y `buildListenArgs` no pasa `--input-on` |
| `C-2 …` (resto de la suite) | sin regresiones: 35/35 |

## Contrato app↔daemon tocado

| Dirección | Elemento | Cambio | Cobertura |
|---|---|---|---|
| daemon → app | `session_pending` | sin cambios de esquema; nuevo **uso** (deduplicación por par a 60 s) | `no_second_dialog_for_same_peer` |
| app → daemon (stdin) | `{"perm":{"input":bool,"audio":bool,"mic":bool}}` | se envía **antes** de la decisión al aceptar | `admissionWrites` (unit) + `accept_implies_control` (widget: toggle on) |
| app → daemon (stdin) | `{"accept"/"reject":"<ID>","trust"?:true}` | sin cambios (`admissionLine` intacto) | `C-44` |

No se añaden eventos nuevos; el `switch` no gana casos, así que no hay rama
`default` nueva que cubrir.

## Límite conocido (para el daemon, no se toca aquí)

La ventana de 60 s es **por par** y no puede distinguir un reintento del mismo
intento de una sesión nueva del mismo par: el evento `session_pending` no lleva
identificador de intento. Consecuencia: durante 60 s, un «Aceptar una vez»
se reenvía a cualquier reconexión de ese par. Fix exacto (ciclo de `core/`):
incluir en `session_pending` un `attempt`/`session_id` (y que el daemon ignore
los intentos muertos), o acortar el `handshake_timeout` de 45 s. Mientras tanto
la ventana se reinicia al volver a pulsar «Compartir escritorio».

**El gate de campo sigue pendiente**: el guion es aceptar desde un cliente
mientras se abre `session_pending` por varios transportes y comprobar (1) que el
diálogo aparece una sola vez, (2) que al aceptar se puede mover y seleccionar una
ventana en el anfitrión sin tocar el toggle, y (3) que el chip «solo ver» solo
aparece si el anfitrión apaga el control.
