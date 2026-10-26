# PLAN DE IMPLEMENTACIÓN — `anyremote` (Organic Driven Development)

| Campo | Valor |
|---|---|
| Versión | 1.0.0 |
| Fecha | 2026-10-26 |
| Estado | Aprobado por el usuario (decisiones §0.1 registradas) |
| Spec origen | [01-requerimientos.md](01-requerimientos.md) |
| Metodología | Organic Driven Development (ODD) |

---

## 0. Fundamento

### 0.1 Interpretación de ODD y decisiones tomadas

**ODD = crecimiento incremental desde semillas validadas**: cada requisito nace como un *germen* (evidencia RE verificada → prototipo mínimo → evolución por iteraciones cortas con gates), priorizando adaptación sobre diseño upfront.

Decisiones del usuario (2026-10-26):

- **D-01**: Estrategia de licencia **clean-room desde patrones** — anyremote se diseña a partir de comportamiento observado (H-AD-xx) y patrones identificados (H-RD-xx), **sin copiar código** de RustDesk (AGPL-3.0). Libertad de licencia; auditoría L-02 simplificada.
- **D-02**: Stack **Rust core + UI Flutter** — misma forma que RustDesk: núcleo en Rust (captura, códecs, transporte, sesión) + UI Flutter multiplataforma (Windows primero, RNF-03).

### 0.2 Principios ODD

1. **Semilla validada antes que código**: ningún ciclo arranca sin sus hallazgos aprobados (equivalente a C-01/RNF-05).
2. **Crecimiento en brotes verticales**: cada ciclo entrega un slice funcional extremo a extremo (UI → sesión → red), no capas horizontales muertas.
3. **Adaptación sobre upfront-design**: la arquitectura fija el tronco y los contratos; los ciclos la corrigen con evidencia, versionada.
4. **Gate por ciclo**: no se cierra sin tests, demo del slice, trazabilidad actualizada y commit/tag. Poda lo que no sobrevive al gate.
5. **Reproducibilidad total** (RNF-06): cada análisis MCP/CLI se guarda como evidencia versionada con el hash/commit que lo produce.

### 0.3 Línea base (verificada 2026-10-26)

| Artefacto | Valor |
|---|---|
| `AnyDesk.exe` | `D:\Users\Usuario\Downloads\Utilidades\escritorio-remoto\AnyDesk.exe` — 8.553.912 bytes, SHA256 `2a85e3bc09a2c8b6707cd56ec464be42705a5849f1a8a5b38c66b56b2c07b124` |
| `rustdesk/` | `C:\proyectos\fixed\rustdesk` — commit `59f2953c94b8373f124dc38d4fd0635098f7c5cd` (2026-10-26), upstream `rustdesk/rustdesk`, licencia AGPL-3.0 |
| MCP **rea** | CLI `rea 6.8.0` operativa; servidor MCP stdio (`rea mcp doctor`: healthy, 229 tools) |
| MCP **codebase-memory** | `codebase-memory-mcp` (binario 0.12.1, 17 tools) — instalado globalmente vía npm y conectado en OpenCode |
| Repo destino | `C:\proyectos\fixed\anyremote` (git inicializado, rama `main`) |

---

## 1. Estructura del repositorio

```
anyremote/
├── specs/                    # 01-requerimientos.md, 02-plan-implementacion.md, 03-*.md
├── evidence/
│   ├── baseline.json         # hash AnyDesk.exe, commit rustdesk, versiones MCP (Fase 0)
│   ├── rea/                  # reportes MCP rea, inventario H-AD-xx
│   ├── codebase-memory/      # reportes de indexación, H-RD-xx
│   └── legal/                # dictamen L-01/L-02
├── core/                     # workspace Rust (crates: session, transport, capture, input, clipboard, filexfer, identity, profiles, telemetry)
├── app/                      # UI Flutter (escritorio Windows primero)
├── tests/                    # integración end-to-end por ciclo
└── docs/                     # decisiones de diseño ↔ hallazgos
```

---

## 2. Fase de siembra — fases del spec (0–6)

| Fase | Contenido clave | Entregable / Gate |
|---|---|---|
| **0. Preparación** | MCP rea y codebase-memory operativos; `baseline.json`; estructura `evidence/`; git init con versionado por fase | `baseline.json`, inventario de herramientas |
| **1. Indexación RustDesk** *(precondición dura C-01)* | Indexar `rustdesk/` con codebase-memory; verificar cobertura; mapa de módulos: captura, códecs/transporte, handshake/autenticación, archivos/portapapeles, configuración | Reporte de indexación + mapa de módulos → **habilita Fase 2** |
| **2. RE de AnyDesk** | CLI/skill REA sobre `AnyDesk.exe` (hash registrado): superficie funcional, flujos de conexión, artefactos de config, endpoints observados, modelo de identificadores, flujos de UI | Inventario **H-AD-xx** con evidencia por consulta |
| **3. Lectura dirigida RustDesk** | Por cada H-AD-xx → consultas codebase-memory → **H-RD-xx** (ruta lógica del símbolo, sin copiar código — *clean-room*); matriz comportamiento ↔ patrón ↔ componente; vacíos; auditoría licencias L-02 | Matriz de equivalencias + vacíos aceptados |
| **4. Diseño** | Arquitectura de componentes (spec §8) con trazabilidad RF ↔ H-AD/H-RD; modelo de datos (§9); contratos entre crates; plan de pruebas RNF-01/RNF-02 | `specs/03-arquitectura.md` + `specs/04-trazabilidad.md` aprobados |
| **5. Preparación de build** | Congelar baseline; toolchain Rust + Flutter; scaffold `core/` + `app/`; gates de calidad (fmt, clippy, tests) | Repo compilable "hello session" |
| **6. Cierre de RE** | Check-list CA-01…CA-06, informe de cierre | Acta de aceptación de la siembra |

---

## 3. Ciclos de crecimiento (implementación)

Orden por dependencia — crece lo que alimenta a lo demás. Tronco **S0** primero.

| Ciclo | Brote (slice mínimo) | Semilla requerida | Gate de cierre |
|---|---|---|---|
| **S0 Tronco** | Workspace Rust+Flutter que compila; canal cifrado local (loopback) con handshake simulado; estado de sesión | H-RD de transporte/handshake | Dos nodos locales intercambian bytes cifrados; test e2e verde |
| **C1 · RF-01** | Iniciar/aceptar sesión real entre dos nodos (controlador ↔ controlado) | H-AD flujo conexión + H-RD rendezvous | Sesión establecida y terminada limpiamente; métricas básicas |
| **C2 · RF-03** | Conexión por ID y por dirección + autenticación de sesión | H-AD modelo de identificadores (sin clonar IDs propietarios) + H-RD pairing | Autenticación mutua (RNF-01) probada; rechazo de credenciales inválidas |
| **C3 · RF-02a** | Transferencia de pantalla + inyección teclado/ratón | H-AD rendimiento observable + H-RD captura/códecs | FPS/latencia medidos vs referencia (RNF-02); entrada funcional |
| **C4 · RF-02b** | Portapapeles bidireccional | H-RD clipboard | Sincronización verificada con contención de formatos |
| **C5 · RF-02c** | Transferencia de archivos | H-RD filexfer | Transferencia íntegra (hash), reanudación básica |
| **C6 · RF-04** | Perfiles de conexión persistentes | H-AD artefactos de config + H-RD config | CRUD perfiles + aplicación en nueva sesión |
| **C7 · RF-05** | Registro de sesiones + diagnóstico de conexión | H-AD artefactos de log observados | Evento de auditoría (spec §9) por sesión; diagnóstico accionable |
| **C8 Consolidación** | Endurecimiento transversal: RNF-01 completo, RNF-02 benchmark, auditoría de secretos, poda | Todos | Check-list RNF + trazabilidad RF↔código completa |

### 3.1 Micro-loop ODD por ciclo

```
SEMILLA (hallazgo aprobado) → BROTE (slice mínimo funcional)
→ CRECIMIENTO (iteraciones cortas: refinar + tests) → GATE (criterios + trazabilidad + commit/tag)
→ PODA (eliminar código/repr. sobrante antes del siguiente ciclo)
```

---

## 4. Trazabilidad y evidencia (RNF-05, C-04)

- `specs/03-trazabilidad.md` (tras Fase 4): cada RF ↔ H-AD-xx / H-RD-xx ↔ componente (`core/crate`) ↔ test de gate.
- Todo reporte de REA y toda consulta codebase-memory se versiona en `evidence/` con la invocación exacta que lo produjo.
- Git: commit convencional por iteración + **tag por ciclo cerrado** (Fase 0, actividad 4).

---

## 5. Riesgos operativos

| Riesgo | Mitigación |
|---|---|
| codebase-memory sin operativo (R-05) | Resuelto en Fase 0: instalado vía npm y conectado en OpenCode; verificado con `opencode mcp list` |
| Sesgo por versión (R-03) | Fijados: hash AnyDesk + commit RustDesk en `baseline.json` |
| Deriva hacia copia de código AGPL (D-01) | Gate de cada ciclo incluye revisión *clean-room*: H-RD como referencia de patrón, nunca como fuente de copia |
| Rendimiento bajo la referencia (RNF-02) | Medición desde C3 con la métrica observable extraída en Fase 2 |

---

## 6. Siguientes pasos

1. Ejecutar **Fase 0**: `baseline.json`, scaffold de `evidence/`, commit.
2. **Fase 1**: indexación de `rustdesk/` con codebase-memory — gate absoluto antes de RE o diseño.
3. Encadenar Fases 2–6; luego ciclos S0→C8 (§3).
