# Gate C-45 — Entrada remota opt-in (F-02)

| Campo | Valor |
|---|---|
| Ciclo | **C-45** (`specs/02` §12.2) |
| Hallazgo | **F-02** de `evidence/security-review-2026-11-19.md` (severidad Alta) |
| Requisito | RNF-08 (mínimo privilegio por capacidad) |
| Fecha | 2026-11-19 |
| Rama | `feature/c45-input-optin` |
| Tag | `input-optin-v0.13.16` |
| Base | `develop` = `6967620` (el plan de seguridad vive en `feature/seguridad-plan-c44-c51`) |

## Problema (semilla)

1. La app pasaba `--allow-input` en **todos** los arranques del listener y, además,
   «Compartir escritorio» forzaba `_acceptRemoteControl = true`
   (`app/lib/main.dart`, C-21).
2. El daemon inicializaba `DynamicPerms.input_on = cfg.allow_input`: la sesión
   nacía con la entrada **permitida** sin esperar a ningún toggle ni a la línea
   `{"perm":...}` de la UI.
3. Consecuencia: compartir pantalla concedía teclado/ratón (y con F-01, a
   cualquiera que alcanzara el puerto). El README prometía «opt-in».

## Diseño implementado: capacidad ≠ estado

| Concepto | Flag / vía | Default | Efecto |
|---|---|---|---|
| **Capacidad** | `--allow-input` | off | Sin ella **ninguna** orden enciende la entrada |
| **Estado inicial** | `--input-on` (requiere la capacidad) | off | Sesión en «solo ver» hasta el primer `{"perm":{"input":true}}` |
| **Estado dinámico** | `{"perm":{"input":bool}}` por stdin (UI) | — | Enciende/apaga en caliente (D-07) |

Decisión: la app **sigue armando la capacidad** (`--allow-input`) para que los
toggles dinámicos funcionen, pero **nunca** pasa `--input-on`: el estado lo fija
el anfitrión. Así el fallo seguro es «denegar» si la UI no envía nada (antes era
«permitir»). La CLI conserva el comportamiento histórico pidiendo `--input-on`.

## Cambios

**Daemon** (`core/daemon/src/lib.rs`, `src/main.rs`)
- `DaemonConfig.input_on_start` (nuevo, default `false`) + flag `--input-on`.
- `DynamicPerms::new`: `input_on = allow_input && input_on_start`.
- Evento `listening` publica el estado real: `input_capability` e `input_initial`.
- `InputDenied` distingue **capacidad** (`allow_input`) de **estado** (`input_on`)
  y añade `reason` (`toggle_apagado` / `no_concedido` / `unsupported`).

**App** (`app/lib/main.dart`)
- Se elimina el auto-encendido de C-21 al pulsar «Compartir escritorio».
- `buildListenArgs` mantiene `--allow-input` (capacidad) y **no** pasa
  `--input-on` (comentario con la razón).
- Nuevo `shareSummary(...)`: el registro dice «solo ver (…)» o «acepta …».
- El mensaje de `input_denied` explica si falta capacidad, si el toggle está
  apagado o si falta el permiso.
- Chip **«solo ver»** en la cabecera del visor cuando el listener está activo y
  el control remoto apagado.

## Gates (todos verdes)

```bash
# Rust (toolchain MSVC: `anyremote-daemon` depende de codec/vpx)
cd core && cargo test --workspace
#   → 160 passed; 0 failed
cargo clippy --workspace --all-targets -- -D warnings   # 0 avisos
cargo fmt --all -- --check                             # 0 diffs

# Flutter
cd app && flutter analyze                              # No issues found!
flutter test                                           # 32/32
```

Pruebas específicas:
- `daemon_input_e2e::input_off_by_default_with_capability_and_enabled_by_perm`
  (nueva, dirigida por eventos): `listening` con `input_capability=true` e
  `input_initial=false`; tecla con el toggle apagado → `InputDenied`
  (`reason=toggle_apagado`, `input_on=false`, `allow_input=true`); tras
  `{"perm":{"input":true}}` la siguiente tecla se inyecta; acuses
  `(2, 1 ok, 1 denegado)`; auditoría con ambos eventos; `bye` ordenado.
- `daemon_input_e2e::input_partial_grant_*` y `input_full_grant_*`: ahora fijan
  `input_on_start=true` (simulan la CLI con estado inicial ON) y siguen
  inyectando.
- `daemon_input_e2e::input_denied_by_default_without_allow_input`: intacta
  (capacidad ausente ⇒ todo denegado).
- `daemon_dynperm_e2e::input_toggles_at_runtime`: fija `input_on_start=true`
  (la UI apaga a los 0,75 s y reactiva) — sigue midiendo on→off→on.
- `widget_test`: «C-45: compartir escritorio no concede control remoto» +
  `buildListenArgs` sin `--input-on`; el test del shell ahora exige el checkbox
  **apagado** tras pulsar «Compartir escritorio» (sustituye la aserción de C-21).

## Compatibilidad y límites

- **CLI**: quien quiera entrada desde el inicio debe pasar
  `--allow-input --input-on` (documentado en README y `--help`). Sin
  `--input-on`, la sesión espera `{"perm":{"input":true}}` (el evento
  `listening` lo hace visible).
- **Wire**: sin cambios (no se tocó `arsp.proto`).
- **Pendiente de campo**: validar en dos equipos que (a) compartir sin marcar
  control queda en «solo ver» y (b) al marcar el toggle durante la sesión la
  entrada empieza a inyectarse sin reconectar.
- **No resuelve F-01**: C-44 (autorización del par) sigue pendiente; hasta
  entonces la recomendación de red confiable se mantiene.
