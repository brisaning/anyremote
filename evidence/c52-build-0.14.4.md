# C-52 · Fase B — binario v0.14.4 construido y verificado (gate de campo)

| Campo | Valor |
|---|---|
| Fecha | 2026-11-20 |
| Rama | `feature/c52-video-regression` (HEAD `4e88cc5`, árbol sucio solo por el bump de versión) |
| Versión | **0.14.4** (app `0.14.4+1`) |
| Alcance | Rebuild del daemon + app para el gate de campo de la **Fase B** (D-13c) |
| Gates | ✅ todos verdes (resultado real abajo) |
| Commit / tag / release | ⏳ **NO** hechos — los hace el usuario (nada de `git commit`, `git push`, `git tag` ni `gh release`) |

---

## 1. Versión: los cuatro sitios

| Sitio | Antes | Ahora | Cómo se comprobó |
|---|---|---|---|
| `VERSION` | `0.14.3` | **`0.14.4`** | lectura del fichero |
| `app/pubspec.yaml` | `0.14.3+1` | **`0.14.4+1`** | `Select-String '^version:'` (el script lo exige) |
| `core/daemon/Cargo.toml` | `0.14.3` | **`0.14.4`** | `Select-String '^version\s*='` (el script lo exige) |
| `core/Cargo.lock` (`anyremote-daemon`) | `0.14.3` | **`0.14.4`** | actualizado por `cargo build` (única línea del diff) |

`git diff --stat` → 4 ficheros, 4 líneas, **solo números de versión**:

```
 VERSION                | 2 +-
 app/pubspec.yaml       | 2 +-
 core/Cargo.lock        | 2 +-
 core/daemon/Cargo.toml | 2 +-
```

`scripts/build-release.ps1` es la barrera: aborta si `VERSION`, `pubspec.yaml` y
`daemon/Cargo.toml` no coinciden. El build pasó esa comprobación ⇒ los tres
sitios concuerdan con `0.14.4`.

---

## 2. Build

Invocación exacta:

```powershell
powershell -ExecutionPolicy Bypass -Command "$env:Path = 'C:\proyectos\tools\flutter\flutter\bin;C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;' + $env:Path; Set-Location 'C:\proyectos\fixed\anyremote'; & '.\scripts\build-release.ps1'"
```

Salida (resumen literal):

```
== anyremote 0.14.4 (4e88cc5) ==
   Compiling anyremote-codec v0.1.0
   Compiling anyremote-transport v0.1.0
   Compiling anyremote-daemon v0.14.4
    Finished `release` profile [optimized] target(s) in 40.34s
Building Windows application...                                    75.9s
✓ Built build\windows\x64\runner\Release\anyremote_app.exe

OK v0.14.4 -> C:\proyectos\fixed\anyremote\app\build\windows\x64\runner\Release
```

**Sin bloqueos**: no hubo LNK1104 ni «being used by another process» (no había
procesos de anyremote abiertos). Los `.exe` se sustituyeron sin incidencias.

### Artefactos (raíz: `app/build/windows/x64/runner/Release/`)

| Fichero | Tamaño | SHA-256 |
|---|---|---|
| `anyremote_app.exe` | 90 624 B | `2C0CFCB3A5E59204D779481C84AC690A1839EE3B3649DF79082C5D01B05E38C4` |
| `anyremoted.exe` | 12 384 256 B | `747C1BCC0164EEE1274479D2E622374C632C278CC05A272E2E622302C098F123` |
| `data/app.so` | 5 653 384 B | `59CF7206F187D5417D141F7A1A668A9DFC790318DA284A62AEBB05499DBDEE8A` |
| `flutter_windows.dll` | 21 289 472 B | (runtime de Flutter, sin cambios) |

### Daemon junto a la app (lo que resuelve `resolveDaemonPath()`)

`app/lib/main.dart:327` → `resolveDaemonPath()` devuelve
`<dir de Platform.resolvedExecutable>\anyremoted.exe` (salvo override
`ANYREMOTE_DAEMON`). El daemon **está** en ese directorio, y es **byte a byte
idéntico** al que produjo cargo:

```
built   : 747C1BCC0164EEE1274479D2E622374C632C278CC05A272E2E622302C098F123
deployed: 747C1BCC0164EEE1274479D2E622374C632C278CC05A272E2E622302C098F123
IDENTICAL: True
```

---

## 3. Verificación de versionado del binario

| Comprobación | Comando | Resultado |
|---|---|---|
| Metadatos del `.exe` de la app | `(Get-Item anyremote_app.exe).VersionInfo.FileVersion` | **`0.14.4+1`** ✅ |
| Ídem, `ProductVersion` | idem | `0.14.4+1` ✅ |
| `data/app.so` contiene la versión | `grep -a -o '0\.14\.4' data/app.so` | **3 ocurrencias** ✅ |
| `data/app.so` **no** contiene la versión vieja | `grep -a -o '0\.14\.3' data/app.so` | **0 ocurrencias** ✅ |
| El daemon imprime su versión | `anyremoted.exe --help` | **`anyremoted 0.14.4`** ✅ |
| El daemon está junto a la app | `Release/anyremoted.exe` | ✅ (hash idéntico, ver §2) |

Las 3 ocurrencias de `0.14.4` en `app.so` corresponden al define
`APP_VERSION=0.14.4` (`app/lib/main.dart:24`,
`String.fromEnvironment('APP_VERSION')`). La versión del daemon sale de
`env!("CARGO_PKG_VERSION")` (`core/daemon/src/lib.rs:106`, `DAEMON_VERSION`),
impresa por `core/daemon/src/main.rs:113`.

---

## 4. Gates ejecutados (resultado real)

### Rust (`core/`)

| Gate | Comando | Resultado |
|---|---|---|
| Formato | `cargo fmt --all -- --check` | ✅ exit 0, sin diferencias |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ exit 0, **0 avisos / 0 errores** |
| Tests | `cargo test --workspace` | ✅ exit 0 — **179 passed / 0 failed** (2 ignored) |

```
54 binarios de test (unit + e2e + doc-tests)
test result: ok. 179 passed; 0 failed; 2 ignored; ...   (suma de los 54)
CLIPPY_EXIT=0
TEST_EXIT=0
```

> Nota honesta: al enlazar el test `daemon_h264_e2e` el linker de MSVC emite
> `LNK4099: PDB 'vpxmd.pdb' was not found ... linking object as if no debug info`
> (mensajes `linker_messages` del C de `shiguredo_libvpx`, no lints de Rust).
> **No** afecta al gate: clippy con `-D warnings` da exit 0, porque `cargo
> clippy` no evalúa mensajes del enlazador. Es preexistente y no lo introduce
> este cambio.

### Flutter (`app/`) — Flutter **3.47.8** (la versión fijada en AGENTS.md)

| Gate | Comando | Resultado |
|---|---|---|
| Análisis | `flutter analyze` | ✅ exit 0 — **No issues found!** (49,1 s) |
| Tests | `flutter test` | ✅ exit 0 — **35 tests, All tests passed!** |

```
ANALYZE_EXIT=0
FLUTTER_TEST_EXIT=0
00:15 +35: All tests passed!
```

---

## 5. Entorno (host Windows)

- **Rust**: `stable-x86_64-pc-windows-msvc` (el feature `vpx` de
  `anyremote-codec` solo compila con MSVC).
- **Flutter**: `3.47.8` · Dart `3.13.7` · engine `dfd2e6cb7d319db05e3bf1503f3e2ac60f6288a2`.
- **SDK de Flutter**: `C:\proyectos\tools\flutter\flutter` — **no está en el
  PATH** de esta sesión; hay que anteponerlo
  (`$env:Path = 'C:\proyectos\tools\flutter\flutter\bin;' + $env:Path`).
- **CMake** (libopus del crate `audio`): tampoco está en el PATH;
  `C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`.

---

## 6. Qué falta (pendiente del usuario)

- ⏳ `git commit` del bump de versión + este fichero de evidencia.
- ⏳ Tag del ciclo en `develop` (no en la rama de feature) tras integrar.
- ⏳ Release published (`gh release`) — **solo con OK explícito del usuario**.
- ⏳ Gate de campo (Fase C) con el binario `0.14.4` de este árbol.

**No se ha hecho commit, push, tag ni publicación de release.**
