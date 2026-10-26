# Análisis PE preliminar — AnyDesk.exe (Fase 2, parcial)

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Herramienta | REA 6.8.0 (MCP sesión + CLI PE nativo) |
| Artefacto | `AnyDesk.exe` — SHA256 `2a85e3bc…c07b124` (verificado vía `open_binary`, coincide con baseline) |
| Formato | PE32 (x86), kind executable |
| Estado | Parcial — proveedor deep (Ghidra) pendiente de instalación |

## Hallazgos PE

### H-AD-01 — Tabla de imports vacía (binario protegido/packed)

- **Evidencia**: `rea inspect-pe-imports AnyDesk.exe --format json --filter-output normalized_result.coverage`
- **Resultado**: `coverage: { status: "complete", examined_dlls: 0, imports: 0, delay_imports: 0 }`
- **Interpretación**: el ejecutable no expone imports estáticos (IAT vacía) — comportamiento característico de protección/packing con resolución de imports en runtime (LoadLibrary/GetProcAddress bajo stub).
- **Impacto en diseño**: la superficie funcional observable exige análisis dinámico o decompilación del stub; el mapa de capacidades vía imports no está disponible.
- **Tipo**: observación (derived).

### H-AD-02 — Inventario de recursos PE (UI de dibujo propio, versión y manifiesto)

- **Evidencia**: `rea inspect-pe-resources AnyDesk.exe --format json --filter-output normalized_result.resources`
- **Resultado** (11 recursos, 26 entradas examinadas, coverage complete):

| Tipo PE | ID | Lang | Bytes | SHA256 (12) |
|---|---|---|---|---|
| RT_ICON (3) | 1–7 | 1033 | 7054/1640/744/488/296/4264/1128 | ea057e…/f50850…/bc1cb9…/c43362…/b00824…/b971ae…/9756f7… |
| RT_GROUP_ICON (14) | 1000, 1001 | 1033 | 76, 34 | 404108…/bd48b5… |
| RT_VERSION (16) | 1 | 1033 | 588 | 44220962c784 |
| RT_MANIFEST (24) | 1 | 1033 | 1541 | 624b35f31bf1 |

- **Interpretación**: sin RT_DIALOG (5) ni RT_STRING (6) → la UI no usa diálogos/strings Win32 nativos (UI de dibujo propio); 2 grupos de iconos (distintos contextos de identificación visual); manifiesto y versión presentes.
- **Tipo**: observación (observed). **L-01**: no reproducir iconos/marcas; solo inventario.

### H-AD-03 — Proveedor deep no disponible (limitación de proceso)

- `search_strings`/`binary_overview`/decompilación requieren proveedor deep; en este host: Ghidra sin configurar, Hopper (macOS-only), IDA sin configurar, Kuna (sin PE).
- **Remediación en curso**: instalación de Ghidra 12.2 (release oficial NSA, SHA256 `2c2cd776db3c1f6a097ba418922378ac3649cadd5e844a6f5f088ae628a7be67`) + `GHIDRA_INSTALL_DIR`.
- **Tipo**: unknown (proceso), no de comportamiento del artefacto.

## Invocaciones exactas (reproducibilidad)

```
open_binary { path: "D:\\Users\\Usuario\\Downloads\\Utilidades\\escritorio-remoto\\AnyDesk.exe" }
rea inspect-pe-imports "D:\...\AnyDesk.exe" --format json --filter-output normalized_result.coverage,normalized_result.imports[0,1]
rea inspect-pe-resources "D:\...\AnyDesk.exe" --format json --filter-output normalized_result.resources
rea inspect-binary-layout "D:\...\AnyDesk.exe" --format json   # capability_unavailable (pwntools: solo ELF Linux)
```

## Pendiente (tras Ghidra)

1. `search_strings`: endpoints de red, claves de configuración, modelo de identificadores, flags CLI.
2. `list_strings`/`binary_overview`: conteos y cobertura.
3. Procedimientos del stub de protección (opcional, acotado).
4. Cierre de Fase 2: inventario H-AD-xx completo + matriz de cobertura funcional.
