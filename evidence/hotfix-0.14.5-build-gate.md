# Gate de build — hotfix v0.14.5 (flujo de aceptación C-53/C-54)

**Fecha**: 2026-11-23 · **Rama**: `hotfix/v0.14.5-accept-flow` · **Ciclo**: hotfix
sobre `develop` (C-53 en `app/`, C-54 en `core/`).

**Alcance de este archivo**: versionado único, build reproducible y verificación
del binario. **No** hay commit, push, tag ni release: los commits los hace el
gestor del proyecto.

## Versión: los cuatro sitios

| Sitio | Valor | Commiteado en |
|---|---|---|
| `VERSION` | `0.14.5` | `3fe9ae1` |
| `app/pubspec.yaml` | `version: 0.14.5+1` | `3fe9ae1` |
| `core/daemon/Cargo.toml` | `version = "0.14.5"` | `3fe9ae1` |
| `core/Cargo.lock` → `anyremote-daemon` | `0.14.5` | `3fe9ae1` (lo actualiza el build) |

El salto es **0.14.3 → 0.14.5** (no existe una 0.14.4 publicada: la 0.14.3 era
la última versionada). La barrera de `scripts/build-release.ps1` acepta el build
solo si `VERSION`, `app/pubspec.yaml` y `core/daemon/Cargo.toml` coinciden.

## Build

Toolchain del host: Rust `stable-x86_64-pc-windows-msvc` (1.99.1; el feature
`vpx` no compila con GNU) · Flutter **3.47.8** / Dart 3.13.7
(`C:\proyectos\tools\flutter\flutter`, el fijado por CI) · VS Build Tools 2022
en `C:\BuildTools` + CMake en
`C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`.

```
$ powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1
== anyremote 0.14.5 (cbed6f8) ==
    Finished `release` profile [optimized] target(s) in 6.18s
Building Windows application...                                    51.5s
✓ Built build\windows\x64\runner\Release\anyremote_app.exe
OK v0.14.5 -> C:\proyectos\fixed\anyremote\app\build\windows\x64\runner\Release
```

### Artefactos (`app/build/windows/x64/runner/Release/`)

| Artefacto | Tamaño | SHA-256 |
|---|---|---|
| `anyremote_app.exe` | 90 624 B | `959006c3a09f7adef2fce39267e93654367ec3e5bd91fea52ebe77038c217c36` |
| `anyremoted.exe` | 12 420 608 B | `cb7b90fbe02d73381aef348d3e38f33ec43db097eac0cf5e71a8e0c6427499a4` |
| `data/app.so` | 5 604 232 B | `c02999406055562206c538947713053503fbdf051881ee3f146ed2f21c37bb6e` |

`anyremoted.exe` junto a la app es **byte a byte** el de
`core/target/release/anyremoted.exe` (mismo SHA-256) → lo resuelve
`resolveDaemonPath()` (`app/lib/main.dart:363`, que devuelve
`<dir del exe en ejecución>\anyremoted.exe`).

## Verificación de versionado

| Comprobación | Resultado |
|---|---|
| `anyremote_app.exe` → `VersionInfo.FileVersion` | **`0.14.5+1`** ✅ (`ProductVersion` = `0.14.5+1`) |
| `data/app.so` contiene `0.14.5` | ✅ 3 ocurrencias: `[build] anyremote 0.14.5`, `v0.14.5`, `anyremote 0.14.5 (desktop windows-x64)` |
| `data/app.so` **no** contiene `0.14.4` | ✅ 0 ocurrencias (tampoco `0.14.3` ni `0.14.2`: no quedó ninguna versión anterior en el AOT) |
| `anyremoted.exe --help` | ✅ imprime `anyremoted 0.14.5` (banner `DAEMON_VERSION` = `CARGO_PKG_VERSION` = `0.14.5`) |
| daemon junto a la app | ✅ `Release\anyremoted.exe`, mismo hash que el de `core/target/release/` |
| `core/Cargo.lock` → `anyremote-daemon` | ✅ `0.14.5` |

### Nota: `GIT_REV` embebido

El primer build corrió con el árbol **sin commitear** (HEAD `6b5837f` + cambios
pendientes), por lo que el binario embebía `rev 6b5837f`. El contenido del
código era idéntico, pero el sello de revisión mostrado en la UI («build») no
apuntaba al commit real. Se reconstruyó con el árbol ya commitido:
`== anyremote 0.14.5 (cbed6f8) ==` → `data/app.so` embebía `rev cbed6f8` /
`build cbed6f8`.

Efecto colateral: **solo cambió `data/app.so`** (hash anterior
`2875768f52c1b0e4274a0d4039ecb72bc404c97c192fcc52f6916c5a7d5f9837` → nuevo
`c02999406055562206c538947713053503fbdf051881ee3f146ed2f21c37bb6e`);
`anyremote_app.exe` y `anyremoted.exe` mantienen sus hashes.

## Gates (invocación exacta y resultado real)

### Rust — `cd core`

```
$ cargo fmt --all -- --check
FMT_EXIT=0

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.69s
CLIPPY_EXIT=0

$ cargo test --workspace
    Finished `test` profile [unoptimized + debuginfo] target(s) in 40.92s
TEST_EXIT=0
```

**178 tests / 0 fallos** (2 ignorados: `probe_scale_cost` y `probe_encoder`,
sondas manuales fuera del ciclo). 54 bloques `test result: ok`.

- `windows_backend::windows_clipboard_read_probe` → **ok** (el portapapeles no
  bloqueó; la sonda es tolerante al estar ocupado).
- No apareció ningún `discover_error` / `10048` (puerto de descubrimiento
  21119 ocupado) en esta ejecución. Habría sido informativo, no bloqueante.

### Flutter — `cd app`

```
$ flutter analyze
Analyzing app...
No issues found! (ran in 45.6s)
ANALYZE_EXIT=0

$ flutter test
00:15 +35: All tests passed!
FLUTTER_TEST_EXIT=0
```

**35/35 tests**, incluidos los nuevos del hotfix: `C-44/C-53:
accept_implies_control (sin casilla en el diálogo)` y `C-53:
no_second_dialog_for_same_peer`.

## Estado

- ✅ Versión única coherente en los cuatro sitios.
- ✅ Build reproducible y binario verificado con la versión del ciclo.
- ✅ Gates de `core/` y `app/` en verde.
- ⏸️ **Sin tag y sin release**: pendientes del **OK explícito del usuario**. El
  tag se crearía en `develop` tras integrar el ciclo, con mensaje que resuma el
  cambio y estos gates.
