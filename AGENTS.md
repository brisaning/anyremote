# anyremote — Reglas del proyecto

Aplicativo de escritorio remoto desarrollado bajo **Organic Driven Development** (ODD).
Plan maestro: [specs/02-plan-implementacion.md](specs/02-plan-implementacion.md) · Spec: [specs/01-requerimientos.md](specs/01-requerimientos.md).

> **Estado del proyecto (dinámico)**: [README § Estado](README.md#estado) — versión vigente, gates, requisitos y pendientes. Al cerrar cualquier fase/ciclo/lote: actualizar README + `specs/02` (tabla de ejecución) + `specs/04` (ciclos/trazabilidad) + bóveda, y commitear.

## Memoria del proyecto (obligatoria)

La memoria persistente vive en la bóveda Obsidian **anyremote**:

- Ruta: `D:\Users\Usuario\OneDrive\Obsidian\anyremote`
- Acceso: MCP `obsidian-vault` (endpoint HTTP del plugin en `http://localhost:8765/mcp`; Obsidian debe estar abierto)

**Convención**: registrar en la bóveda toda decisión de diseño (D-xx), hallazgo de RE (H-AD-xx / H-RD-xx), cierre de fase/ciclo y cambio de estado. Índice: `Memoria del proyecto.md`. La bóveda es la memoria entre sesiones: al iniciar una sesión, leerla para recuperar el contexto; al terminar trabajo relevante, actualizarla.

## Herramientas MCP

- **rea** (`reverse-engineer-anything`): ingeniería inversa de AnyDesk.exe. Siempre cerrar sesiones/binares con `close_binary` y exportar evidencia.
- **codebase-memory-mcp**: grafo de conocimiento sobre `rustdesk/` (proyecto externo, debe indexarse en Fase 1 — precondición dura C-01). Para código de este repo, usar también codebase-memory.
- **obsidian-vault**: memoria del proyecto (arriba).

## Flujo de trabajo git (norma D-11, 2026-10-26)

- **`main`**: solo vía **PR/MR** (nunca commits directos).
- **`develop`**: rama de integración. Se actualiza desde ramas de funcionalidad **con rebase**.
- Cada funcionalidad o ciclo ODD ⇒ rama `feature/<ciclo>-<descripción>` creada desde `develop`:
  commits convencionales pequeños + **push de la rama** en cada avance.
- Cierre de rama: `git rebase develop` → push (force si rebaseó) → integración en `develop` (merge/FF o MR) → push de `develop` → **tag del ciclo en `develop`**.
- Los tags viven en `develop` (y se propagan al integrar a `main` vía PR).

## Reglas clave

- **Clean-room (D-01)**: no copiar código de RustDesk (AGPL-3.0); solo patrones referenciados por ruta lógica de símbolo.
- Trazabilidad obligatoria (RNF-05): cada componente/ciclo debe mapear a hallazgos H-AD/H-RD versionados en `evidence/`.
- Versionado: commit convencional por iteración; tag por ciclo cerrado (S0…C8).
- Evidencia MCP/CLI versionada en `evidence/` con la invocación exacta que la produjo.

## Agentes del proyecto (D-16, 2026-11-21)

El proyecto define agentes propios en `.opencode/agents/`. El
**`anyremote-orchestrator`** es el dueño del ciclo ODD: convierte una semilla en
ciclo, delega cada pieza y verifica los gates. Los demás ejecutan una pieza
acotada y **no pueden escribir fuera de su ámbito** (los permisos del agente lo
impiden).

| Agente | Rol | Modelo | Ámbito |
|---|---|---|---|
| `anyremote-orchestrator` | Dueño del ciclo ODD (spec → comisión → gate → evidencia → tag) | `minimax-m3.1-flash` | specs/README/evidence; `core`/`app` con confirmación; **push denegado** |
| `anyremote-field-analyst` | Logs de campo → semilla (causa raíz, no arreglo) | `deepseek-v4.1-flash` | solo lectura |
| `anyremote-spec` | Ciclo con fases, gates falsables y trazabilidad | `minimax-m3.1-flash` | `specs/`, `evidence/`, `README` |
| `anyremote-rust` | Implementación en `core/` + tests | `deepseek-v4.1-flash` | `core/**` |
| `anyremote-flutter` | Implementación en `app/` + widget tests | `deepseek-v4.1-flash` | `app/**` |
| `anyremote-test` | e2e de fronteras, huecos de cobertura, matriz de regresión | `minimax-m3.1-flash` | `*/tests/**` |
| `anyremote-security` | Auditoría de fronteras (auth, permisos, cuotas, logs) | `deepseek-v4.1-flash` | solo lectura |
| `anyremote-cleanroom` | Patrones de RustDesk/AnyDesk → decisión propia (D-01) | `minimax-m3.1-flash` | `evidence/` |
| `anyremote-release` | Versión única, build reproducible, tag (release con OK) | `minimax-m3.1-flash` | `VERSION`, pubspec, `Cargo.toml`, `scripts/` |

**El usuario es el gestor**: valida plan, modelos y resultados de campo. El
orquestador para y pregunta cuando algo depende de su OK (release, gate de
campo, cambio de política).

## Ciclo ODD: Definition of Done (D-13)

Un ciclo **no está cerrado** hasta que:

- (a) hay test del área tocada;
- (b) si cambia comportamiento observable, hay e2e o widget test;
- (c) el **binario está reconstruido y verificado** con la versión del ciclo
  (`VERSION` = `app/pubspec.yaml` = `core/daemon/Cargo.toml`; el script de build
  falla si no coinciden);
- (d) existe `evidence/<ciclo>-gate.md` con la invocación exacta de cada gate;
- (e) hay fila en `specs/02` (estado) y en `specs/04` (trazabilidad);
- (f) hay nota en la bóveda;
- (g) hay tag del ciclo en `develop`.

Sin (c) no hay cierre: un ciclo sin binario verificado es trabajo no validado.

## Gate de campo y alcance del ciclo (D-12/D-14)

- **Un ciclo = un frente**: si la semilla toca dos capacidades, se divide. No se
  mezcla seguridad con UX o vídeo en el mismo commit/tag.
- El ciclo **N+1 no empieza** hasta que el N tiene validación del usuario con un
  guion de campo corto (~2 min) pegado en `evidence/<ciclo>-field.md`.
- Todo `fix` de campo abre ciclo con evidencia, aunque sea de una línea.
- `docs` no cuenta como feature: documentar no avanza el estado de un ciclo
  salvo que sea su entregable.
- El **estado** de una capacidad (p. ej. el toggle de control) es del anfitrión
  y sobrevive al ciclo de sesión: si el daemon lo reinicia, la UI miente.
- **D-15**: el contrato app↔daemon (eventos por stdout, líneas por stdin) se
  versiona y se valida en CI (fase F5 del plan, `specs/02` §14).

## Plan vigente

`specs/02` **§14** (plan de reorganización 2026-11-21): F0 andamiaje → F1 vídeo
(C-52) → F2 UX+Caps (C-53/C-54) → F3 seguridad (C-46…C-51) → F4 teclado (C-55)
→ F5 contrato → F6 estructura → F7 release v0.15.0. Diagnóstico en
`evidence/plan-2026-11-21.md`.

## Toolchain de build (host Windows) — corregido 2026-11-21

- **Rust (workspace `core/`)**: toolchain **MSVC** (`stable-x86_64-pc-windows-msvc`,
  el default). El feature `vpx` de `anyremote-codec` (`shiguredo_libvpx`) **solo
  compila con MSVC**; con GNU falla con
  `Windows supports only MSVC ... but the target is 'x86_64-pc-windows-gnu'`.
- **CMake**: `opus` (crate `audio`) compila libopus con CMake — si hay que
  recompilar, añadir al PATH
  `C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`.
- **Flutter (`app/`)**: **VS Build Tools 2022 + VCTools** (MSVC) para
  `flutter build windows`. Versión **fijada 3.47.8** (la de CI): con un Flutter
  más nuevo, `package:flutter/material.dart` queda deprecado y el analyze falla.
- **Build completo y versionado**:
  `powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1`
  (comprueba que `VERSION`, `app/pubspec.yaml` y `core/daemon/Cargo.toml`
  coinciden, compila ambos y deja el daemon junto a la app). Los `.exe` no se
  pueden reemplazar con la app abierta: LNK1104 → pedir que la cierren, no
  matar el proceso. El script **localiza Flutter y CMake** si no están en el
  PATH (rutas conocidas: `C:\proyectos\tools\flutter\flutter\bin` y
  `C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`) y
  falla con un mensaje claro si no los encuentra.
- Memoria de estado: bóveda `anyremote` → nota `Memoria del proyecto.md`.
