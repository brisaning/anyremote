# Reporte MCP rea — AnyDesk.exe (Fase 2)

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Artefacto | `AnyDesk.exe` — 8.553.912 B — SHA256 `2a85e3bc…c07b124` (verificado en `open_binary`; coincide con baseline) |
| Versión observada | 9.8.2 (`release/win_9.8.2`, recurso de versión) |
| Herramientas | REA 6.8.0 (MCP + CLI) · Ghidra **12.1.4** (proveedor deep, `GHIDRA_INSTALL_DIR`) · JDK Temurin **25.0.5** (`JAVA_HOME`) |
| Snapshot análisis | `C:\Users\wnuke\.rea\snapshots\anydesk` (proyecto Ghidra ephemeral cacheado) |
| Evidencia IDs | `ev_cf30f063…` (search_strings completo) |
| Estado | **Cerrada con vacíos explícitos** (superficie funcional protegida) |

## 1. Método y cobertura

| Operación (REA) | Estado | Cobertura |
|---|---|---|
| `open_binary` | ✅ | PE x86 verificado |
| `inspect-pe-imports` | ✅ | coverage complete: **0 imports / 0 DLLs** |
| `inspect-pe-resources` | ✅ | coverage complete: 26 entradas, 11 recursos |
| Ghidra `search_strings` (inventario completo) | ✅ | **27 strings** en todo el artefacto (coverage `defined-data-coverage-v1`) |
| `inspect-binary-layout` | ❌ | proveedor pwntools: solo ELF Linux |
| `capture_process_scenario` | ❌ | adaptador Windows PTY sin verificación de limpieza de descendientes (solo Linux/macOS) |
| `observe_native_ui` / `capture_native_ui_scenario` | ❌ | utilidades nativas solo macOS |
| `inspect_pe_imports` decompilación stub | omitida | decompilar VM-stubs Themida: desproporcionado, sin valor funcional |

## 2. Conclusión central

**AnyDesk 9.8.2 está protegido con Themida/WinLicense** (firma "Voltage" en `.rdata$voltmd`, sección `.custom`, 27 strings totales, IAT vacía). La superficie funcional observable (endpoints, claves de configuración, modelo de identificadores, flujos de UI internos) **no es extraíble estáticamente**; las vías dinámicas de REA no están disponibles en Windows (PTY capture y UI nativa son de Linux/macOS). La observación dinámica acotada autorizada confirmó: modelo de proceso (raíz + 2 hijos), estado pre-EULA sin artefactos ni conexiones.

**Consecuencia para el plan**: los RF dependientes del comportamiento interno de AnyDesk (RF-01 flujo de conexión, RF-03 identificadores/endpoints) quedan **trazados a H-RD (RustDesk) + diseño propio**, no a H-AD. La matriz de cobertura lo registra explícitamente (RNF-05).

## 3. Registro de instalación de soporte (reproducibilidad)

| Componente | Versión | Origen | Verificación |
|---|---|---|---|
| Ghidra | 12.1.4 (`ghidra_12.1.4_PUBLIC_20260921.zip`) | Release oficial NSA/GitHub | SHA256 `ddac49f903da9d5bac833e5cc79395098b9c33cfd3279be5f31bd00387d2d4db` (publicado = calculado) |
| JDK | Temurin 25.0.5+8 LTS | Adoptium API | `java -version` LTS 25 |
| Rutas | `C:\proyectos\tools\ghidra-12.1.4\ghidra_12.1.4_PUBLIC` · `C:\proyectos\tools\jdk-25\jdk-25.0.5+8` | | `GHIDRA_INSTALL_DIR` + `JAVA_HOME` en env del MCP rea (opencode.jsonc) y CLI |

Se retiraron instalaciones superadas (Ghidra 12.2 y JDK 21: fuera de la matriz verificada de REA 6.8.0 — 12.1.x + JDK 25+).

## 4. Invocaciones exactas (reproducibilidad)

```
open_binary { path: "D:\\...\\AnyDesk.exe" }
rea inspect-pe-imports  "D:\...\AnyDesk.exe" --format json --filter-output normalized_result.coverage
rea inspect-pe-resources "D:\...\AnyDesk.exe" --format json --filter-output normalized_result.resources
rea search "D:\...\AnyDesk.exe" --pattern "." --mode regex --snapshot "C:\Users\wnuke\.rea\snapshots\anydesk" --format jsonl --filter-output normalized_result
rea search "D:\...\AnyDesk.exe" --pattern "anydesk\.(com|net|org|io|support)" --mode regex ... (0 matches)
rea search "D:\...\AnyDesk.exe" --pattern "kernel32" --mode regex ... (0 matches)
capture_process_scenario { executable: "D:\...\AnyDesk.exe", timeout_ms: 20000, ... }  → capability_unavailable (win32)
observe_native_ui { pid: 81696, window_id: 527410, accessibility: true }  → capability_unavailable (macos)
# Observación dinámica directa (complementaria, Windows):
powershell Start-Process AnyDesk.exe → PID 81696, ventana "AnyDesk", HWND 527410; hijos 105620/111452
netstat -ano / -p udp | grep <pids> → 0 conexiones (estado pre-EULA)
taskkill /PID 81696 /T /F → familia terminada; 0 residuos (discos/registro verificados)
```

Ver inventario: [inventario-hallazgos.md](inventario-hallazgos.md) · Cobertura: [matriz-cobertura-fase2.md](matriz-cobertura-fase2.md)
