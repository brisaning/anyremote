# Reporte de indexación — rustdesk (Fase 1)

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26T13:05Z |
| Herramienta | codebase-memory-mcp 0.12.1 (MCP conectado en OpenCode) |
| Proyecto índice | `rustdesk` |
| Ruta raíz | `C:/proyectos/fixed/rustdesk` |
| Commit indexado | `59f2953c94b8373f124dc38d4fd0635098f7c5cd` (ver baseline.json) |
| Modo | `full` (all+semantic), persistencia habilitada |
| Duración | ~18 s (13:05:27 → 13:05:45 UTC) |
| Estado final | `succeeded` / graph `ready` |

## Métricas

| Métrica | Valor |
|---|---|
| Nodos | 18.829 |
| Aristas | 80.146 |
| Archivos Rust | 317 |
| Archivos Dart | 158 |
| Métodos / Funciones | 5.127 / 4.422 |
| Clases / Structs / Enums | 998 / 514 / 146 |

## Cobertura y limitaciones (C-04)

- `parse_partial`: 19 archivos con rangos de error (gradle, plist, `flutter/linux/*.cc`) — indexados con recuperación tree-sitter; preferir grep para esos rangos.
- `parse_unusable`: 2 (`Dockerfile`, `flutter/build_android_deps.sh`) — irrelevantes para el mapa de subsistemas.
- `not_indexed`: 129 archivos **por diseño** (imágenes, `.git`, `.cargo`), 0 saltados por error.
- `watch`: no activo (rustdesk no es el proyecto de sesión) → re-indexar `index_repository` para refrescar tras cambios.
- doc_links: 217 menciones resueltas; 38 faltantes (docs).

## Invocaciones exactas (reproducibilidad)

```
index_repository { repo_path: "C:\\proyectos\\fixed\\rustdesk", name: "rustdesk",
                   mode: "full", persistence: true, async: true }
index_repository { repo_path: "C:\\proyectos\\fixed\\rustdesk", name: "rustdesk", status: true }
index_status     { project: "rustdesk", diagnostics: "summary", format: "json" }
get_architecture { project: "rustdesk", aspects: ["overview","structure"], format: "json" }
```

## Veredicto (criterio de salida Fase 1)

✅ Indexación completa y verificada; módulos, dependencias y puntos de entrada cubiertos.
Mapa de subsistemas: [mapa-modulos.md](mapa-modulos.md).
