# Inventario de hallazgos H-AD (AnyDesk 9.8.2) — Fase 2

Convención: `observed` = dato leído del artefacto/proceso · `derived` = interpretación razonada · `unknown` = no observable con las vías disponibles.
L-01: se registran hechos y digests; no se replican marcas ni activos.

| ID | Hallazgo | Evidencia | Confianza |
|---|---|---|---|
| **H-AD-01** | **IAT completamente vacía**: 0 imports, 0 delay-imports, coverage complete → imports resueltos en runtime bajo protección | `inspect-pe-imports` → coverage `{examined_dlls:0, imports:0}` | observed |
| **H-AD-02** | **Recursos PE**: 7 iconos (RT_ICON), 2 grupos de iconos, RT_VERSION (588 B), RT_MANIFEST (1541 B); **sin RT_DIALOG ni RT_STRING** → UI de dibujo propio, sin controles Win32 nativos | `inspect-pe-resources` (11 recursos, digests por payload) | observed |
| **H-AD-02b** | **Versión y política**: `FileVersion 9.8.2`, `ProductVersion 9.8`, assembly `AnyDesk.AnyDesk.AnyDesk 9.8.2.0 x86`; `asInvoker` (sin elevación UAC); `dpiAware=true/PM`; compat Win7–Win11 | search_strings manifiesto + StringFileInfo (0x29be05a–0x29be248) | observed |
| **H-AD-03** | **Protección Themida/WinLicense**: secciones `.custom`, `.itext`, `.rdata$voltmd` (firma "Voltage"); **27 strings en todo el binario**; `kernel32`=0 → payload cifrado | volcado completo `search --pattern "."` (27/27 emitidos, evidence `ev_cf30f063…`) | derived |
| **H-AD-04** | **Identificadores de build**: `release/win_9.8.2`, SHA1 `3b03e00b75d7e661…`, MD5 `796d925bcc94b911e23f418307fd5c11` | volcado strings (0x219d21c–0x219d25c) | observed |
| **H-AD-05** | **Referencia de símbolos**: PDB `AnyDesk.pdb` GUID `f6d836cf-c4b1-44c7-bc07-0131ac1a87f1` age 1 (RSDS) — sin PDB público; REA no admite PDB seleccionado por el caller | limitations del evidence Ghidra | observed |
| **H-AD-06** | **Modelo de proceso dinámico**: proceso raíz (PID 81696, ventana "AnyDesk", HWND 527410) + **2 procesos hijos AnyDesk.exe** (105620, 111452); sin artefactos de config en `%APPDATA%`/`%ProgramData%` ni registro HKCU/HKLM al primer arranque | observación acotada (Start-Process + Get-CimInstance + netstat + reg/ls) — familia terminada, 0 residuos | observed |
| **H-AD-07** | **Red pre-EULA**: 0 conexiones TCP/UDP de la familia en estado de primer arranque (ventana de bienvenida/EULA sin aceptar) | `netstat -ano [-p udp]` por PID → vacío | observed |
| **H-AD-08** | **Vacíos de superficie** (unknown): (a) endpoints de red observables, (b) modelo de identificadores/emparejamiento, (c) artefactos de configuración en runtime tras aceptar EULA, (d) flujos internos de UI. Causa: payload Themida + REA sin captura de proceso/UI en Windows | ver reporte §1–2 | unknown |

## Impacto por RF (para trazabilidad Fase 3+)

- **RF-01 (iniciar/aceptar sesión)**: sin hallazgo H-AD usable → dependerá de H-RD (rendezvous/handshake de RustDesk) + diseño propio.
- **RF-03 (identificadores + autenticación)**: H-AD-08(b) unknown → patrón de ID y pairing desde RustDesk; identificadores propios (L-01: no clonar IDs de AnyDesk).
- **RF-02 (pantalla/entrada/archivos)**: superficie funcional declarada solo por manifiesto ("screen sharing and remote control") → diseño desde H-RD.
- **RF-04 (config)**: H-AD-02/02b/06 acotan política observable (asInvoker, sin config persistente pre-EULA) → patrón de claves desde RustDesk (`KEYS_*`/`OPTION_*`).
- **RF-05 (registro/diagnóstico)**: sin hallazgo H-AD → diseño propio (RustDesk no expone un análogo directo ya mapeado en Fase 1).
