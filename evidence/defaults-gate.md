# Gate C-20 — defaults de sesión: UDP preseleccionado + servidor de encuentro prestablecido (v0.12.3)

**Fecha**: 2026-11-04 · **Rama**: `feature/session-defaults` · Petición de campo del usuario («UDP es más rápido que TCP» + VPS prestablecido)

## Qué cambia (solo defaults de la app — wire y daemon intactos)

| Default | Antes | Ahora | Por qué / por qué es seguro |
|---|---|---|---|
| Toggle **«UDP (QUIC)»** | desmarcado (TCP histórico) | **preseleccionado** | QUIC es más rápido: handshake integrado con TLS 1.3 en **1 RTT** (TCP+TLS = 2) y sin bloqueo entre streams. **Par TCP-only** → fallback `quic_fallback` (timeout de `quic::connect` = **3 s**); **listener** con `--transport udp` ata TCP+UDP en el mismo puerto → la compatibilidad con clientes viejos queda intacta (probada en campo, C-15). |
| Campo **«servidor de encuentro»** | vacío | **`51.222.81.81:21120`** (VPS propio `serveremote`) | resolución WAN + relay de respaldo disponibles sin teclear nada. **No penaliza la LAN**: el orden de resolución es **broadcast LAN → rdv → caché** (broadcast primero; `--rdv` solo actúa si aquél falla) y por IP directa no hay resolución. Vaciar el campo = LAN pura (o el fichero `rdv` del daemon). |

El toggle sigue **deshabilitado durante sesión/listener** (como antes) y el
campo es editable/vaciable en cualquier momento.

## Tests (actualizados + nuevos)

- Checkbox UDP (índice 3) ahora **`isTrue`** con razón «v0.12.3: UDP (QUIC)
  preseleccionado — más rápido que TCP».
- **Nuevo**: `find.widgetWithText(TextField, '51.222.81.81:21120')` → el campo
  va prestablecido.
- `buildListenArgs`: el default de la **función** sigue `udp: false` /
  `rdv: ''` (la app los pasa explícitos con `udp: _useUdp` y
  `rdv: _rdv.text`) — aserciones existentes intactas.

## Gates (2026-11-04)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **18/18** |

## Pendiente de validación de campo

- Sesión LAN con los defaults (debe resolver por **broadcast sin tocar el
  VPS** — si el VPS está caído no debe notarse).
- Sesión WAN por ID con el rdv prestablecido.
- **Release** `udp-rdv-defaults-v0.12.3` descargado en las dos máquinas.
