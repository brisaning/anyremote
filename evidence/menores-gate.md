# Gate C-7 — menores RF-02a + gates de CI · v0.8.1

| Campo | Valor |
|---|---|
| Fecha | 2026-10-27 |
| Rama | `feature/menores` (D-11) · tag `menores-v0.8.1` |
| Gates | Rust **42 suites / 0 fallos** · **clippy 0 avisos / 0 errores** (`-D warnings`) · **rustfmt 0 diffs** · `flutter analyze` limpio · `flutter test` **4/4** |

## 1) Acuse de inyección al visor (RF-02a, pendiente del README)

El visor sabía lo que *enviaba*, no lo que el anfitrión *inyectaba*.

- **Wire**: nuevo mensaje `InputAck {ok, perm, detail}` (campo 24 del envelope).
- **Anfitrión**: tras cada evento de entrada acusa el resultado — `ok=true`
  (inyectado), `ok=false` con motivo (`toggle_apagado` · `no_concedido` ·
  `unsupported` · error del inyector). Se envía SIEMPRE (≈20 B por evento: a
  60 eventos/s ≈ 1 KB/s, despreciable frente al vídeo).
- **Controlador**: emite `input_result {ok,perm,detail}` → la app:
  - cuenta las **realmente inyectadas** (`entrada: N` ahora se incrementa por
    acuse, no por envío);
  - registra el rechazo **solo cuando cambia el motivo** (sin inundar el log):
    `[daemon] entrada RECHAZADA por el anfitrión: mouse toggle_apagado`.
- Test e2e: en la sesión con toggles dinámicos se exigen **3 acuses = 2 ok + 1
  denegado** (coincide con las inyecciones y denegaciones del anfitrión).

## 2) Teclas F1..F12 (RF-02a, pendiente de specs/01)

- Códigos **30..=41** (reservados: 0 carácter · 1–11 navegación · 20–22
  modificadores) documentados en `arsp.proto`.
- `anyremote-input::Key` + `vk_for` (VK_F1..VK_F12, sin flag extendido),
  mapeo wire `key_from_code` y `keyCodeFor` de la app (espejo).
- Tests: `function_keys_map_from_wire_codes` (transport: 30→F1, 41→F12, 42 →
  no soportado) y `keyCodeFor(f1)=30 · f5=34 · f12=41 · f13=null` (Flutter).

## 3) Gates de CI (RNF pendiente: `clippy`/`fmt`)

- **Normalización rustfmt del workspace** (una sola vez: 43 archivos) y
  **clippy a 0 avisos**: `clippy --fix` para lo automático + arreglos puntuales
  (`ONCE_INIT`→`static` con uso directo de `Once::new()`, `resume_from` como
  expresión de `loop`, eliminación de código muerto `name_hint` e imports sin
  uso) y `#[allow]` justificados donde la firma es por diseño
  (`too_many_arguments` en las funciones de capa de sesión,
  `large_enum_variant` en `Established`).
- **Workflow** `.github/workflows/ci.yml` (windows-latest — el proyecto es
  Windows-only): job **Rust** con `cargo fmt --check` + `cargo clippy ... -- -D
  warnings` + `cargo test`; job **Flutter** con `flutter analyze` + `flutter
  test`. Caché de Rust (`Swatinem/rust-cache`) y de Flutter para no pagar el
  build completo en cada push.

## 4) Resampler de mayor orden — queda pendiente (único menor)

Decisión explícita: el resample **lineal** actual (44.1↔48 kHz, ratios
cercanos) se mantiene y queda documentado como el último menor pendiente. El
plan definido es integrar **`rubato`** (puro Rust) en `capture_loop` y en la
reproducción, con reestructuración del acumulador por chunks — esfuerzo M para
un ahorro que el campo aún no ha pedido (no se pide calidad de audio critique
en las pruebas).

## Regresión

Rust **42/0** · clippy **0/0** · fmt **0 diffs** · Flutter **analyze limpio +
4/4** (tras la normalización completa del formato).
