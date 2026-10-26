# anyremote — Reglas del proyecto

Aplicativo de escritorio remoto desarrollado bajo **Organic Driven Development** (ODD).
Plan maestro: [specs/02-plan-implementacion.md](specs/02-plan-implementacion.md) · Spec: [specs/01-requerimientos.md](specs/01-requerimientos.md).

## Memoria del proyecto (obligatoria)

La memoria persistente vive en la bóveda Obsidian **anyremote**:

- Ruta: `D:\Users\Usuario\OneDrive\Obsidian\anyremote`
- Acceso: MCP `obsidian-vault` (endpoint HTTP del plugin en `http://localhost:8765/mcp`; Obsidian debe estar abierto)

**Convención**: registrar en la bóveda toda decisión de diseño (D-xx), hallazgo de RE (H-AD-xx / H-RD-xx), cierre de fase/ciclo y cambio de estado. Índice: `Memoria del proyecto.md`. La bóveda es la memoria entre sesiones: al iniciar una sesión, leerla para recuperar el contexto; al terminar trabajo relevante, actualizarla.

## Herramientas MCP

- **rea** (`reverse-engineer-anything`): ingeniería inversa de AnyDesk.exe. Siempre cerrar sesiones/binares con `close_binary` y exportar evidencia.
- **codebase-memory-mcp**: grafo de conocimiento sobre `rustdesk/` (proyecto externo, debe indexarse en Fase 1 — precondición dura C-01). Para código de este repo, usar también codebase-memory.
- **obsidian-vault**: memoria del proyecto (arriba).

## Reglas clave

- **Clean-room (D-01)**: no copiar código de RustDesk (AGPL-3.0); solo patrones referenciados por ruta lógica de símbolo.
- Trazabilidad obligatoria (RNF-05): cada componente/ciclo debe mapear a hallazgos H-AD/H-RD versionados en `evidence/`.
- Versionado: commit convencional por iteración; tag por ciclo cerrado (S0…C8).
- Evidencia MCP/CLI versionada en `evidence/` con la invocación exacta que la produjo.
