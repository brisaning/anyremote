# Informe de licencias por módulo — Fase 3 (L-02)

**Fecha**: 2026-10-26 · **Artefacto auditado**: `rustdesk/` @ `59f2953c94b8373f124dc38d4fd0635098f7c5cd`
**Método**: lectura de `LICENCE` raíz y declaraciones `license` de `Cargo.toml` por módulo (hechos, no asesoría jurídica — el dictamen L-02 formal lo emite el responsable legal).

| Módulo | Licencia declarada | Fuente |
|---|---|---|
| Repo rustdesk (crate raíz `librustdesk`) | **AGPL-3.0** | `LICENCE` (GNU AFFERO GPL v3) |
| `libs/scrap` (captura + códecs) | **MIT** | `Cargo.toml` scrap |
| `libs/enigo` (inyección de entrada) | **MIT** | `Cargo.toml` enigo |
| `libs/base` (config, keyboard, protos) | sin campo → **hereda AGPL-3.0** | `Cargo.toml` base |
| `libs/clipboard` | sin campo → **hereda AGPL-3.0** | `Cargo.toml` clipboard |
| `libs/portable` | sin campo → **hereda AGPL-3.0** | `Cargo.toml` portable |
| `flutter/` (UI) | AGPL-3.0 (repo) | `LICENCE` |

## Análisis de impacto sobre anyremote (D-01 clean-room)

1. **anyremote no hereda AGPL-3.0** mientras observe clean-room: solo se referencian *patrones* (estructuras de módulos, modelos de datos conceptuales, protocolos públicos) sin copia de código — la matriz de equivalencias y el inventario H-RD contienen rutas de símbolo, no código.
2. **Módulos MIT** (`scrap`, `enigo`) son reutilizables como *dependencias normales* (no copiadas) sin restricción de copyleft si se consumen como crates externos; cualquier adaptación de código real dentro de anyremote debe respetar su MIT (atribución).
3. **Módulos AGPL-3.0** (`base`, `clipboard`, raíz): cualquier *copia de código* los haría derivados → cualquierremote quedaría obligado a publicar bajo AGPL. Por política D-01: **no copiar**; implementar equivalentes propios guiados por el patrón.
4. **R-02 mitigado**: este informe + D-01 cubren la auditoría de licencias por módulo; el dictamen legal formal (L-02, CA-04) sigue pendiente del responsable legal.

## Regla operativa para los ciclos S0–C8

- En cada gate: revisar diffs contra el criterio *"¿se copió texto/código de rustdesk?"* — si sí, revertir y reimplementar clean-room.
- Dependencias externas de crates públicos (p. ej. `enigo` upstream, `vpx`/`aom` bindings) declaradas en `Cargo.toml` de anyremote con su licencia correspondiente.
