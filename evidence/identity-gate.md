# Gate D-03 — identidad persistente + trust por dispositivo · v0.8.0

| Campo | Valor |
|---|---|
| Fecha | 2026-10-27 |
| Rama | `feature/identity-trust` (D-11) · tag `identidad-v0.8.0` |
| Gates | Rust **42 suites / 0 fallos** · `flutter analyze` limpio · `flutter test` **4/4** |
| Artefactos | `Release/anyremote_app.exe` **0.8.0** + `Release/anyremoted.exe` **0.8.0** |

## Problema (pendiente histórico de specs/01 y README)

Cada arranque generaba una identidad nueva → `NodeId` **efímero**, `session_id`
distinto en cada sesión y `peer_trusted` **siempre `false`** (sin trust por
dispositivo y sin base para «conectar por ID»).

## Qué se entregó

1. **Identidad persistente** — `Identity::load_or_generate(dir)` guarda la
   semilla ed25519 en **`<config>/identity.key`** (hex, 32 bytes). Mismo equipo
   → **mismo `NodeId` en todos los arranques** → `session_id` correlacionable.
   Archivo corrupto → se regenera sin bloquear el arranque.
2. **TrustStore persistente** — **`<config>/trusted.ids`** (`ID=hexpk` por
   línea). Al leer se revalida el vínculo id↔pk y las líneas inválidas se
   ignoran; `TrustStore::remember()` implementa **TOFU**: registra un par YA
   autenticado (solo añade, no pisa; mejor esfuerzo: un fallo de E/S no afecta
   a la sesión).
3. **Cableado en ambos roles** — `listen` y `connect` cargan
   `TrustStore::load(config_dir)` para la validación de auth y llaman a
   `remember` tras autenticar al par → **la segunda sesión del mismo equipo
   aparece con `peer_trusted=true`**.
4. **Visibilidad** — `session_active` incluye `peer_trusted` y la app lo
   muestra: `SESIÓN ACTIVA con XXX ✓ verificado`.

## Política de confianza (decisión documentada)

- `peer_trusted` = «esta clave se autenticó en una sesión previa con este
  equipo» (TOFU). Se graba solo **después de una autenticación válida** (una
  sesión abortada no genera confianza).
- El **anclaje de máquina** sigue siendo el pin de certificado por destino (S0/
  C2): el pin solo se guarda tras autenticar. El listener no puede pinear
  controladores (llegan de cualquier dirección) → su señal de confianza es el
  TOFU por sesión.
- La confianza se **muestra** pero hoy no condiciona la aceptación (que sigue
  anclada en auth + pin): es información para el usuario y la base para
  «conectar por ID» futuro.

## Tests

| Suite | Qué cubre |
|---|---|
| `identity_persists_and_regenerates_on_corruption` | persistencia · mismo `NodeId` entre cargas · regeneración ante archivo corrupto |
| `trust_store_roundtrip_and_tofu_remember` | recordar → recargar → confiable · sin duplicar · línea con id de A y pk de B **ignorada** · línea rota ignorada |
| `daemon_identity_e2e::identity_is_stable_and_peer_becomes_trusted` | 2 sesiones seguidas: **`session_id` idéntico** (identidad estable) + `peer_trusted` = `[false, true]` en **ambos** lados + `identity.key` y `trusted.ids` en disco |

## Notas y limitaciones

- Una identidad por equipo: los dos procesos de la misma app (escuchar y
  conectar) comparten el directorio → misma identidad (igual que un ID por
  máquina en AnyDesk). En pruebas con un directorio compartido, ambos roles
  se autentican con la misma clave (válido en loopback).
- `session_id` pasa a ser `ses-<nodeid>` (estable por equipo, no único por
  sesión): los archivos de auditoría por apertura lo siguen distinguiendo.
- Dos procesos simultáneos arrancando con la misma config pueden regenerar la
  identidad a la vez (último en escribir gana) — ventana de milisegundos en el
  arranque de la app.
