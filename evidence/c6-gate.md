# Gate ciclo C6 — RF-04: perfiles de conexión persistentes

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/c6-profiles` (D-11) · tag `ciclo-c6` |
| Semilla | H-RD-09 (`KEYS_*`/`OPTION_*` catalogados por ámbito) — implementación propia (D-01) |
| Suite | `anyremote-profiles` **6/6**, resto intacto |

## Entregados

- **Catálogo de claves** (`profiles::keys`): `CONFIG_DIR_ENV`, `PROFILES_FILE`, `MAX_ALIAS_LEN`, `DEFAULT_PORT` (patrón H-RD-09).
- **`ConnectionProfile`** con serde: alias(id) saneado (alnum/-/_ ≤64), host no vacío/sin espacios, puerto≠0, `codec ∈ {auto,vp9,deflate,raw}`, `quality 0..=5` — **validación explícita con `ProfileError::Invalid{field,reason}`**.
- **`ProfilesStore`**: `default_path()` (%APPDATA%\anyremote\profiles.toml o env `ANYREMOTE_CONFIG_DIR`), `load` (archivo ausente = vacío; TOML corrupto = `Corrupt` explícito; perfiles inválidos = error), `save` (valida + rechaza aliases duplicados), `upsert`, `remove`.
- **`resolve()` → "host:puerto"** para abrir sesión.

## Gate: `profile_applies_to_a_new_session`
Perfil persistido en disco → **nueva instancia del almacén** (sesión nueva) → carga → resuelve → `TcpStream::connect` real contra un listener vivo ✓. Más: CRUD con persistencia entre instancias, rechazos de validación (id/host/port/codec/quality), archivo corrupto → error explícito, duplicados rechazados, override de env.

## Limitaciones
- TOML de solo perfiles (settings globales = expansión futura sobre el mismo catálogo); sin UI (el dropdown de la app llega con la integración).
