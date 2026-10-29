# SPEC — Proyecto `anyremote`

## 1. Identificación del proyecto

| Campo | Valor |
|---|---|
| Nombre | anyremote |
| Tipo | Aplicativo de acceso a escritorio remoto |
| Ruta destino | `anyremote/` |
| Artefacto de referencia 1 (binario) | `D:\Users\Usuario\Downloads\Utilidades\escritorio-remoto\AnyDesk.exe` |
| Artefacto de referencia 2 (código fuente) | `rustdesk/` |
| Herramientas de soporte obligatorias | MCP **rea** (exploración del binario) y MCP **codebase-memory** (indexación del código fuente) |
| Estado | **Vivo/dinámico** — ver [README § Estado](../README.md#estado) y [specs/04 § Estado de ciclos](04-trazabilidad.md#estado-de-ciclos-2026-10-26) |

## Estado de requisitos (dinámico — actualizar con cada ciclo)

|RF|Estado|Trazabilidad|
|---|---|---|
|RF-01 Sesiones|✅ implementado (C1+C2) · **v0.8.2 listener repetidor** (cierre del invitado → la escucha se libera y acepta reconexiones; el proceso termina al detenerse) · **v0.8.6 dueño ausente** (cerrar la app → la escucha/sesión se detiene sola)|`specs/04`, `evidence/listener-repeat-gate.md`, `evidence/owner-gate.md`|
|RF-02a Pantalla/códec|✅ VP9 + visor (C3/C3-bis/M2) · **v0.8.3 visor sin desbordes**: la pantalla del anfitrión cabe completa a cualquier tamaño de ventana · **v0.8.4 modo visor** (Conexión/Registro ocultos con el visor; `Detener` en la barra; toggles en la cabecera) · **v0.8.5 cursor remoto visible** (overlay con la posición del cursor del anfitrión)|`evidence/c3*-gate`, `m2-video-gate`, `evidence/viewer-id-gate.md`, `evidence/viewer-mode-gate.md`, `evidence/cursor-gate.md`|
|RF-02a Entrada (teclado/ratón)|✅ v0.5.0 cableado en la sesión (D-07 por defecto) · **validada en campo 2026-10-27** · v0.6.2 coordenadas normalizadas · v0.7.0 toggles dinámicos · **v0.8.1 acuse de inyección + F1..F12**|`evidence/input-session-gate.md`, `evidence/menores-gate.md`|
|RF-02b Portapapeles|✅ (C4)|`c4-gate`|
|RF-02c Archivos|✅ + reanudación (C5 + v0.3.0)|`c5-gate`, `v030-quality-batch-gate`|
|RF-03 ID+auth|✅ (C2) · identidad **persistente** + trust **TOFU** (v0.8.0): mismo `NodeId` entre arranques, `peer_trusted=true` en la 2ª sesión · **v0.8.3 ID visible/copiable + CLI `anyremoted id`** (conexión por ID pendiente de resolutor)|`specs/04`, `evidence/identity-gate.md`, `evidence/viewer-id-gate.md`|
|RF-04 Perfiles|✅ (C6 + UI v0.4.0)|`c6-gate`, `ui-batch-gate`|
|RF-05 Registro|✅ (C7)|`c7-gate`|
|RF-06 Trazabilidad|✅ (fases 0–6 + evidencia versionada)|`evidence/`|
|Pendientes globales|🟡 campo v0.8.6 (dueño ausente) — cursor v0.8.5 **✅ probado** · reconexión v0.8.2 **✅ verificada** · conexión por ID (resolutor) · rendimiento medio/largo (HW/UDP/red — specs/02 §7) · resampler (único menor)|README § Pendientes|

---

## 2. Objetivo y alcance

### 2.1 Objetivo

Especificar la construcción de un aplicativo de escritorio remoto denominado **anyremote**, cuyo diseño operacional se deriva de dos fuentes:

1. **AnyDesk.exe** — analizado mediante el MCP *rea* para extraer comportamiento observable (superficie funcional, flujos, protocolos y artefactos de configuración).
2. **RustDesk (código fuente)** — indexado con el MCP *codebase-memory* para reutilizar patrones de arquitectura, protocolos y modelos de datos abiertos.

### 2.2 Dentro del alcance

- Definición funcional y no funcional de anyremote.
- Definición del proceso de ingeniería inversa sobre AnyDesk.exe usando MCP rea.
- Definición del proceso de lectura/indexación del código de RustDesk usando MCP codebase-memory.
- Plan de ejecución por fases con precondiciones, entregables y criterios de aceptación.

### 2.3 Fuera del alcance

- Modificación, parcheo o redistribución de AnyDesk.exe.

---

## 3. Glosario

- **MCP rea**: servidor MCP para exploración/análisis estático y dinámico de binarios.
- **MCP codebase-memory**: servidor MCP que indexa y permite consultar un repositorio de código fuente.
- **Ingeniería inversa de comportamiento**: análisis de interfaces observables (CLI, red, archivos, registro) sin descompilación del núcleo propietario, salvo donde la ley lo permita.
- **Superficie funcional**: conjunto observable de funciones ofrecidas por un binario.
- **Precondición dura**: acción que debe completarse antes de cualquier otra.

---

## 4. Actores y stakeholders

| Actor | Rol |
|---|---|
| Ingeniero de RE | Opera MCP rea sobre AnyDesk.exe |
| Ingeniero de código | Opera MCP codebase-memory sobre rustdesk/ |
| Arquitecto | Consolida hallazgos y define arquitectura de anyremote |
| Responsable legal | Valida cumplimiento de EULA y licencias |
| QA | Verifica criterios de aceptación |

---

## 5. Requisitos funcionales (RF)

- **RF-01**: anyremote debe permitir iniciar y aceptar sesiones de escritorio remoto entre dos nodos (controlador y controlado).
- **RF-02**: anyremote debe soportar transferencia de pantalla, entrada de teclado/ratón, portapapeles y transferencia de archivos.
- **RF-03**: anyremote debe ofrecer conexión por identificador y por dirección, con autenticación de sesión.
- **RF-04**: anyremote debe permitir configurar perfiles de conexión persistentes.
- **RF-05**: anyremote debe exponer un registro de sesiones y diagnóstico de conexión.
- **RF-06**: la definición funcional de RF-01 a RF-05 debe estar trazada a hallazgos obtenidos vía MCP rea (sobre AnyDesk) o vía MCP codebase-memory (sobre RustDesk), según matriz de trazabilidad.

---

## 6. Requisitos no funcionales (RNF)

- **RNF-01 Seguridad**: cifrado de extremo a extremo, autenticación mutua y ausencia de secretos embebidos.
- **RNF-02 Rendimiento**: latencia y FPS objetivo declarados y medidos respecto de la referencia observable.
- **RNF-03 Portabilidad**: plataforma objetivo definida en el plan (Windows como mínimo, extensible).
- **RNF-04 Mantenibilidad**: arquitectura modular alineada con los patrones identificados en RustDesk.
- **RNF-05 Auditabilidad**: toda decisión de diseño debe rastrearse a un hallazgo de RE o a un artefacto indexado.
- **RNF-06 Reproducibilidad**: los análisis de MCP rea y MCP codebase-memory deben ser repetibles y versionados.

---

## 7. Restricciones y precondiciones (duras)

- **C-01 (Precondición dura)**: El código fuente de `rustdesk/` **debe** estar indexado con el MCP **codebase-memory** antes de iniciar cualquier proceso de análisis, diseño o ingeniería inversa.
- **C-02**: AnyDesk.exe puede explorarse a través del MCP **rea**; se permite análisis herramientas herramientas para descompilar o hacer ingeniería inversa.
- **C-03**: El repositorio destino del aplicativo es la ruta `anyremote/`.
- **C-04**: Todos los artefactos de análisis (reportes de rea y consultas a codebase-memory) deben registrarse como evidencia versionada.

---

## 8. Arquitectura conceptual (alto nivel)

- **Capa de presentación**: UI de conexión, listado de perfiles, indicadores de sesión.
- **Capa de sesión**: negociación, autenticación, control de calidad de enlace.
- **Capa de captura/entrada**: captura de pantalla, codificación de video, inyección de eventos.
- **Capa de transporte**: canal de datos cifrado.
- **Capa de transferencia de archivos y portapapeles**: canales auxiliares.
- **Capa de configuración y telemetría local**: perfiles, logs, diagnóstico.

> Cada componente debe mapear a (a) un comportamiento observado en AnyDesk vía MCP rea, o (b) una estructura/patrón identificado en RustDesk vía MCP codebase-memory.

---

## 9. Modelo de datos (conceptual, sin esquema)

- **Sesión**: identificador, par de nodos, estado, métricas de calidad, timestamps.
- **Perfil de conexión**: alias, endpoint, preferencias de códec/calidad.
- **Evento de auditoría**: tipo, origen (rea o codebase-memory), referencia al artefacto.
- **Hallazgo de RE**: id, descripción, evidencia, fuente (AnyDesk/RustDesk), impacto en diseño.

---

## 10. Interfaces externas

- **Interfaz MCP rea**: consultas de superficie funcional de AnyDesk.exe (entradas/salidas, artefactos de configuración, endpoints de red observados).
- **Interfaz MCP codebase-memory**: consultas semánticas sobre `rustdesk/` (módulos, protocolos, patrones).
- **Interfaz de usuario de anyremote**: conexión, perfiles, sesión, transferencias.
- **Interfaz de red**: canal cifrado entre nodos.

---

## 11. Flujos operativos (resumen)

1. Indexación de RustDesk (precondición).
2. Exploración de AnyDesk vía MCP rea.
3. Consolidación de hallazgos y matriz de trazabilidad.
4. Diseño arquitectónico de anyremote.
5. Definición de criterios de aceptación y pruebas.
6. Ejecución de fases de implementación (fuera del alcance de este spec).
7. Verificación y cierre.

---

## 12. Consideraciones legales y éticas

- **L-01**: No extraer ni replicar marcas, logos, identificadores ni material propietario de AnyDesk.
- **L-02**: Registrar dictamen del responsable legal como artefacto previo a la fase de diseño.

---

## 13. Criterios de aceptación (del spec)

- **CA-01**: Existe constancia de indexación de `rustdesk/` con MCP codebase-memory previa al resto de actividades.
- **CA-02**: Existe reporte de MCP rea sobre AnyDesk.exe con superficie funcional y artefactos observados.
- **CA-03**: Existe matriz de trazabilidad RF ↔ hallazgo (rea o codebase-memory).
- **CA-04**: Existe dictamen legal favorable.
- **CA-05**: Arquitectura conceptual aprobada y versionada en `anyremote/`.
- **CA-06**: Plan de fases con entregables y evidencias registradas.

---

## 14. Riesgos

| ID | Riesgo | Mitigación |
|---|---|---|
| R-01 | Violación del EULA de AnyDesk | Revisión legal previa (L-01) |
| R-02 | Reutilización indebida de licencia de RustDesk | Auditoría de licencias (L-02) |
| R-03 | Sesgo de comportamiento por diferencias de versión | Fijar versión/hash del binario y del commit de RustDesk |
| R-04 | Pérdida de trazabilidad en consultas MCP | Registro versionado de evidencias (C-06) |
| R-05 | Dependencia de herramientas MCP no disponibles | Verificar disponibilidad y versiones antes de fase 1 |

---

# PLAN DE EJECUCIÓN

## Fase 0 — Preparación del entorno y gobernanza

**Objetivo**: dejar listo el entorno, las herramientas y las aprobaciones.

**Actividades**

1. Verificar disponibilidad y versiones de MCP **rea** y MCP **codebase-memory**.
2. Registrar la ruta exacta y el **hash** de AnyDesk.exe y el **commit** de `rustdesk/` como línea base inmutable.
3. Crear estructura documental de evidencias (reportes rea, consultas codebase-memory, matriz de trazabilidad).
4. Realizar versionado en git por cada modulo o fase completada

**Entregables**: baseline.json (referencia documental, sin código), inventario de herramientas.

**Criterio de salida**: dictamen legal favorable y herramientas operativas verificadas.

---

## Fase 1 — Indexación de RustDesk (PRECONDICIÓN DURA)

**Objetivo**: indexar `rustdesk/` con MCP codebase-memory **antes de cualquier otro proceso**.

**Actividades**

1. Ejecutar la indexación del repositorio `rustdesk/` mediante MCP codebase-memory.
2. Verificar cobertura de indexación (módulos, dependencias, puntos de entrada).
3. Registrar métricas de indexación (archivos, símbolos, versiones).
4. Generar un mapa inicial de módulos y patrones clave:
   - subsistema de captura de pantalla,
   - códecs y transporte,
   - autenticación y handshake,
   - transferencia de archivos y portapapeles,
   - configuración y perfiles.

**Entregables**: reporte de indexación, mapa de módulos, catálogo de patrones candidatos para anyremote.

**Criterio de salida**: indexación completa y verificada; solo entonces se habilita la Fase 2.

---

## Fase 2 — Ingeniería inversa de AnyDesk vía MCP rea

**Objetivo**: obtener la superficie funcional y el comportamiento observable de AnyDesk.exe.

**Actividades**

1. Cargar AnyDesk.exe en MCP rea con el hash registrado en Fase 0.
2. Extraer superficie funcional observable: capacidades, flujos de conexión, artefactos de configuración, endpoints de red documentados en reportes.
3. Extraer modelo de identificadores y emparejamiento (sin clonar identificadores propietarios).
4. Extraer flujos de UI de referencia (pantallas y transiciones observables).
5. Producir inventario de hallazgos numerados (H-AD-xx) con evidencia y referencia a la consulta en rea.

**Entregables**: reporte MCP rea, inventario H-AD-xx, matriz de cobertura funcional.

**Criterio de salida**: todos los hallazgos versionados y revisados.

---

## Fase 3 — Lectura dirigida del código RustDesk vía MCP codebase-memory

**Objetivo**: traducir los hallazgos de la Fase 2 a soluciones y patrones concretos disponibles en RustDesk.

**Actividades**

1. Por cada hallazgo H-AD-xx, formular consultas a codebase-memory para localizar la implementación abierta equivalente.
2. Registrar hallazgos H-RD-xx (con commit y ruta lógica del símbolo, sin copiar código en los reportes).
3. Elaborar matriz de equivalencias: *comportamiento AnyDesk* ↔ *patrón RustDesk* ↔ *componente anyremote*.
4. Identificar vacíos: comportamientos de AnyDesk sin análogo abierto en RustDesk (a resolver por diseño propio).
5. Auditar licencias por módulo reutilizado (L-02).

**Entregables**: matriz de equivalencias, inventario H-RD-xx, lista de vacíos, informe de licencias por módulo.

**Criterio de salida**: cobertura documentada y vacíos explícitamente aceptados por el arquitecto.

---

## Fase 4 — Diseño de anyremote

**Objetivo**: consolidar los hallazgos en una arquitectura y un backlog de ingeniería.

**Actividades**

1. Definir arquitectura de componentes (sección 8) con trazabilidad RF ↔ H-AD/H-RD.
2. Definir modelo de datos (sección 9) y contratos de interfaz.
3. Definir requisitos de seguridad (RNF-01) y plan de pruebas asociado.
4. Producir plan de iteraciones e hitos de `anyremote/`.
5. Revisión conjunta (Arquitecto + Legal + QA).

**Entregables**: documento de arquitectura, matriz de trazabilidad completa, backlog priorizado, plan de pruebas.

**Criterio de salida**: aprobación del arquitecto y del responsable legal.

---

## Fase 5 — Preparación de la implementación (fuera del alcance de este spec)

**Objetivo**: habilitar la construcción en `anyremote/`.

**Actividades**

1. Congelar versiones (AnyDesk baseline, commit RustDesk, MCPs).
2. Configurar repositorio `anyremote/` y política de evidencias.
3. Definir gates de calidad por iteración.

**Entregables**: baseline congelado, repositorio preparado, gates definidos.

---

## Fase 6 — Verificación y cierre del proceso de RE

**Actividades**

1. Ejecutar check-list de criterios de aceptación (CA-01 a CA-06).
2. Revisar que toda decisión de diseño tenga trazabilidad.
3. Emitir informe de cierre de la fase de ingeniería inversa y lectura de código.

**Entregables**: informe de cierre, acta de aceptación.

---

## Dependencias entre fases

- **Fase 1 es bloqueante absoluta** de Fases 2–6 (C-01).
- Fase 2 requiere dictamen legal de Fase 0.
- Fase 3 requiere inventario H-AD-xx de Fase 2.
- Fase 4 requiere matriz de equivalencias de Fase 3.
- Fase 5 requiere aprobación de Fase 4.

## Evidencias mínimas por fase

1. Hash/commit de línea base.
2. Reporte MCP rea (Fase 2).
3. Reporte de indexación y consultas codebase-memory (Fase 1 y 3).
4. Matriz de trazabilidad consolidada.
5. Dictamen legal.
6. Informe de cierre.