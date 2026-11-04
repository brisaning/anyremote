# PLAN DE IMPLEMENTACIÓN — `anyremote` (Organic Driven Development)

| Campo | Valor |
|---|---|
| Versión | 1.0.0 |
| Fecha | 2026-10-26 |
| Estado | Aprobado por el usuario (decisiones §0.1 registradas) — **ejecución en curso** (ver estado dinámico abajo) |
| Spec origen | [01-requerimientos.md](01-requerimientos.md) |
| Metodología | Organic Driven Development (ODD) |

## Estado de ejecución (dinámico — actualizar al cerrar cada fase/ciclo/lote)

_Última actualización: 2026-10-27 · v0.6.1 · `main`=c5792b8 · `develop`=2e2cd77 · correcciones en `fix/field-audio-quality`_

| Fase/Ciclo/Lote | Estado | Tag / PR |
|---|---|---|
| Fase 0 Preparación | ✅ | `fase-0-completa` |
| Fase 1 Indexación rustdesk (C-01) | ✅ | `fase-1-completa` |
| Fase 2 RE AnyDesk | ✅ (Themida → vacíos aceptados) | `dceeb7f` |
| Fase 3 Equivalencias H-RD | ✅ | `c82cfd2` |
| Fase 4 Arquitectura D-01…D-11 | ✅ | `d831386` |
| Fase 5 Scaffold workspace (10 crates) | ✅ | `a448354` |
| Fase 6 Cierre RE (CA-01..06) | ✅ | `fase-6` |
| Ciclo S0 Tronco TLS+identidad | ✅ gate 9/9+demo | `ciclo-s0` |
| Ciclo C1 Sesión ARSP+permisos (RF-01) | ✅ | `ciclo-c1` |
| Ciclo C2 Auth mutua+binding (RF-03) | ✅ | `ciclo-c2` |
| Ciclo C3 Captura DXGI+vídeo e2e (RF-02a) | ✅ | `ciclo-c3` |
| Ciclo C3-bis VP9 37fps@720p (RNF-02) | ✅ | `ciclo-c3-bis` |
| Ciclo C4 Portapapeles (RF-02b) | ✅ | `ciclo-c4` |
| Ciclo C5 Archivos+hash+reanudación (RF-02c) | ✅ | `ciclo-c5` |
| Ciclo C6 Perfiles (RF-04) | ✅ | `ciclo-c6` |
| Ciclo C7 Auditoría+diagnóstico (RF-05) | ✅ | `ciclo-c7` |
| Ciclo C8 Consolidación (RNF-01/02/05) | ✅ | `ciclo-c8` |
| Integración daemon↔app v0.1.x (cross-device fixes) | ✅ validada en campo | `integracion-v0.1.3`…`v0.1.6` |
| Hito M2 Visor remoto | ✅ validado en campo | `m2-video-v0.2.0`, `integracion-v0.2.1` |
| Lote calidad/robustez v0.3.0 | ✅ validado en campo | `calidad-v0.3.0` · PR #3 |
| Lote UI v0.4.0 (nativo+perfiles) | ✅ | `ui-v0.4.0` · PR #4 |
| Entrada en sesión v0.5.0 (RF-02a) | ✅ **validada en campo** (control remoto real en LAN) | `entrada-v0.5.0` · `evidence/input-session-gate.md` |
| Audio en sesión v0.6.0 | ✅ gate verde · 🟡 pendiente campo | `audio-v0.6.0` · `evidence/audio-gate.md` |
| Ajustes de campo v0.6.1 (diagnóstico 2026-10-27) | ✅ gate verde · 🟡 pendiente LAN | `campo-v0.6.1` · `evidence/audio-gate.md` §Diagnóstico |
| Fix control 720p/1080p v0.6.2 | ✅ gate verde · 🟡 pendiente verificar en campo | `coords-v0.6.2` · `evidence/control-coords-gate.md` |
| C-2 permisos dinámicos + micrófono v0.7.0 | ✅ gate verde · 🟡 pendiente campo | `perm-dinamicas-v0.7.0` · `evidence/dynamic-perms-gate.md` |
| C-3 rendimiento (quick wins) v0.7.1 | ✅ gate verde · 🟡 pendiente verificar en campo | `perf-v0.7.1` · `evidence/perf-audit.md` |
| C-4 fix vídeo estático v0.7.2 (campo) | ✅ gate verde · 🟡 pendiente revalidar en LAN | `video-estatico-v0.7.2` · `evidence/video-static-gate.md` |
| C-5 diagnóstico del anfitrión v0.7.3 | ✅ gate verde · ✅ **validada en campo (prueba sin error)** | `diag-host-v0.7.3` · `evidence/host-debug-gate.md` |
| C-6 identidad persistente + trust v0.8.0 | ✅ gate verde · 🟡 pendiente campo | `identidad-v0.8.0` · `evidence/identity-gate.md` |
| C-7 menores + CI v0.8.1 | ✅ gate verde (acuse, F1..F12, clippy 0/fmt 0/CI) | `menores-v0.8.1` · `evidence/menores-gate.md` |
| C-8 fix de campo: listener repetidor v0.8.2 | ✅ gate verde · ✅ **validada en campo** («Funciona correctamente») | `repeat-v0.8.2` · `evidence/listener-repeat-gate.md` |
| C-9 visor completo + ID visible v0.8.3 | ✅ gate verde (con control negativo de overflow) · 🟡 pendiente campo | `visor-id-v0.8.3` · `evidence/viewer-id-gate.md` |
| C-10 modo visor (cliente) v0.8.4 | ✅ gate verde · 🟡 pendiente campo | `visor-mode-v0.8.4` · `evidence/viewer-mode-gate.md` |
| C-11 cursor remoto visible (overlay) v0.8.5 | ✅ gate verde (change-detection + overlay) · ✅ **probado en campo** | `cursor-v0.8.5` · `evidence/cursor-gate.md` |
| C-12 bug: dueño ausente v0.8.6 | ✅ gate verde · ✅ **validada en campo** (auditoría con `dueño ausente (stdin cerrado)` al cerrar la app) | `owner-exit-v0.8.6` · `evidence/owner-gate.md` |
| C-13 adaptación por red (RTT) v0.8.7 | ✅ gate verde · ✅ **validada en campo** (`red_bitrate` 3M→1.5M→750k con rtt 40–1402 ms en el informe del anfitrión) | `net-adapt-v0.8.7` · `evidence/net-adapt-gate.md` |
| C-14 aceleración por hardware: H.264/MF v0.9.0 (+ hotfix v0.9.1) | ✅ gate verde (stream validado con ffmpeg + e2e720p punta a punta) · ✅ **validada en campo** (`mf_h264` sin fallback, sesión 112 frames/2,4 MB a720p) | `hwcodec-v0.9.0`, `hwcodec-v0.9.1` · `evidence/hwcodec-gate.md` |
| C-15 transporte UDP/QUIC v0.10.0 (+ v0.10.1 auditoría `transport`) | ✅ gate verde (4 unit QUIC + 3 e2e con aserción del `transport` real en auditoría; diseño §8) · ✅ **validada en campo** 2026-11-03 (dos sesiones `--transport udp` sin `quic_fallback`, auditoría `transport:quic`, vídeo+control+C-12 sobre QUIC; compat TCP→dual antes) | `quic-transport-v0.10.0`, `quic-transport-v0.10.1` · `evidence/quic-gate.md` |
| C-16 conexión por ID (resolutor LAN) v0.11.0 (+ hotfixes v0.11.1–v0.11.2) | ✅ gate verde (8 unit + 3 e2e) · ✅ **validada en campo** (2026-11-03: `resolve_ok` broadcast 343 ms + caché 37 ms, auditoría `transport:quic` y `transport:tcp`, pin por ID, sin fallback — tras los2 hotfixes de campo: fan-out de subredes y anti-ICMP-poison del /32 WARP) | `id-resolve-v0.11.0`…`v0.11.2` · `evidence/id-connect-gate.md` |
| C-17 WAN: rendezvous + relay (D-04) v0.12.0 | ✅ gate verde (5 unit protocolo + 3 e2e daemon: resolve+directo, puerto cerrado→relay E2E, fichero `rdv`+offline) · ✅ **servidor reubicado en el repo [serveremote](https://github.com/brisaning/serveremote) con Dockerfile** (consumido por git-alias; su CI valida la imagen) · ✅ **validada en campo** (2026-11-03: VPS `51.222.81.81` Docker · registro+resolución por internet · **2 sesiones por relay** con `peer_addr` del VPS en la auditoría, C-13 adaptando a454 ms de RTT sin caer, `bye` limpio) | `rdv-relay-v0.12.0` · `evidence/rdv-relay-gate.md` · repo `serveremote` |
| C-18 resampler de mayor orden (`rubato`) v0.12.1 | ✅ gate verde (13/13 audio con3 tests de calidad; plan de `evidence/menores-gate.md` §4) · 🟡 pendiente campo (oír el audio) | `resampler-v0.12.1` · `evidence/resampler-gate.md` |
| C-19 render alto sin nativo v0.12.2 | ✅ gate verde (8 tests del ensamblador + integración de frame crudo; Rust 51/0 · clippy0 · fmt0 · Flutter 18/18; diseño §11) · 🟡 pendiente campo (`fps render` → decidir `Texture`) | `render-high-v0.12.2` · `evidence/render-gate.md` |
| C-20 defaults de sesión: UDP + rdv v0.12.3 | ✅ gate verde (Flutter 18/18 con aserciones nuevas; Rust 51/0) · petición de campo del usuario | `udp-rdv-defaults-v0.12.3` · `evidence/defaults-gate.md` |
| **Pendientes** | 🟡 revalidación campo: audio (cadena `rubato`) · «Nativa» · `fps render` a 720p/1080p/Nativa (decisión `Texture`, C-19) | [README § Pendientes](../README.md#pendientes) |

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

## 7. Propuestas futuras (requieren decisión de alcance)

Identificadas en `evidence/perf-audit.md` (§5) a partir de la auditoría de
RustDesk y de nuestro pipeline. **No se abordan hasta decisión explícita** por
su coste o riesgo; la lista queda aquí como backlog de rendimiento/capacidad:

| Propuesta | Esfuerzo | Impacto | Riesgos / notas |
|---|---|---|---|
| ~~**Aceleración por hardware** (NVENC/AMF/QSV/Media Foundation)~~ | L | Alto | ✅ **hecho en v0.9.0 (C-14)** por vía **Media Foundation**: encoder H.264 sync (salida→entrada, CBR, perfil Base, `MF_LOW_LATENCY`) con **fallback VP9 visible** + decoder **Cisco OpenH264 (BSD)**. Limpieza legal: clean-room + tercero BSD — no se copió el fork AGPL `hwcodec`. Los MFT de GPU modernos (NVIDIA/AMD/DX12) son **V2 asíncronos** → GPU directa queda como futuro (protocolo async o SDKs). Stream validado con ffmpeg. `evidence/hwcodec-gate.md` |
| ~~**Adaptación por red** (RTT/pérdida, no solo tiempo de codificación)~~ | M | Alto | ✅ **hecho en v0.8.7 (C-13)**: `NetHint` (campo 26, 1/s desde el que mide) → `watch` al hilo de vídeo → `observe_net` (200/100 ms, confirmación 3, recuperación 10, cooldown 20, `net_congested` bloquea la CPU) + fix del tope al restaurar escala. Ver `evidence/net-adapt-gate.md` |
| ~~**Transporte UDP (KCP) / QUIC**~~ | L | Alto | ✅ **hecho en v0.10.0 (C-15)**: QUIC (quinn) con **binding C2 sobre `export_keying_material`** (misma etiqueta que TCP — sin renegociación: QUIC ya es TLS 1.3), **2 streams** (control + vídeo dedicado con cola drop-new), listen-udp ata TCP+UDP y connect cae a TCP (`quic_fallback`); datagrams con mini-ARQ queda como futuro. `evidence/quic-gate.md` |
| ~~**Cursor propio (overlay no embebido)**~~ | M | Alto | ✅ **hecho en v0.8.5 (C-11)**: `CursorState` (campo 25) con sondeo a 50 ms **solo cambios** + flecha dibujada en el visor (misma métrica 0..65535 que los clics). Ver `evidence/cursor-gate.md` |
| VP8 como opción de CPU barata | M | Medio | `shiguredo_libvpx` ya lo soporta (`Vp8Config.static_threshold`) → exponer `--codec vp8` (proto + negociación) |
| Captura/encode con buffers reutilizados | M | Medio | El vídeo ya corre en hilo propio (v0.7.2); queda eliminar las allocaciones por frame en `bgra_to_i420`/`scale_frame` |
| libyuv/SIMD en vez de `bgra_to_i420` scalar | M | Medio | Dependencia nativa de build (patrón `libs/scrap/build.rs`) o crate `yuv` |
| Cambiar la resolución del host en sesión | L | Medio | Requiere restauración garantizada (incluso si el proceso muere) + permiso D-07 nuevo |
| Codificación por regiones sucias | L | Bajo | **Ni RustDesk lo implementa** (solo un comentario) → pospuesto |
| Profundidad de color reducida (16/8 bits) | L | Bajo | VP9 ya submuestrea 4:2:0 → ahorro marginal |

**Orden recomendado**: ~~solo queda **UDP/QUIC**~~ → **§7 SIN PENDIENTES** (adaptación
por red ✅ v0.8.7 · cursor propio ✅ v0.8.5 · aceleración por hardware ✅ v0.9.0 ·
**UDP/QUIC ✅ v0.10.0 (C-15)**). Quedan como futuros los renglones menores (VP8,
libyuv, regiones sucias, WAN).

---

## 8. C-15: transporte UDP/QUIC — diseño (2026-11-01)

**Disparador de campo** (v0.9.1, 2026-11-01): la sesión murió con
`connection_gone` cuando el enlace degradó (`rtt_last=1402 ms` tras bajar dos
veces el bitrate) — con **TCP** el vídeo bulk bloquea el stream único (HOL
blocking): heartbeats, control y audio esperan detrás de los retransmitidos.
Casos de libro para un transporte sin HOL por flujo.

### Decisión: QUIC (quinn 0.11) con DOS streams fiables

| Pieza | Diseño |
|---|---|
| Flag | `--transport tcp\|udp` (default `tcp` = comportamiento actual sin cambios). listen+connect. |
| **listen `udp`** | ata **TCP y UDP en el mismo puerto** (21118): compat con versiones/peers viejos (solo TCP). |
| **connect `udp`** | intenta QUIC con timeout (~3 s); si falla → evento `quic_fallback` → **TCP** (compat con listen viejo). |
| Streams | el cliente abre **2 bi-streams** en orden: **#1 control** (todo el ARSP de hoy: auth, hello, permisos, input, audio, portapapeles, archivos, heartbeats, NetHint) y **#2 vídeo** (`VideoFrame` host→viewer). Orden determinista (IDs de stream QUIC crecientes); ambos lados siempre abren/aceptan 2. |
| Vídeo | stream **dedicado fiable** (no datagrams en v1 — ver «por qué no datagrams» abajo). El envío vive en una **tarea propia con mpsc acotado (drop-new)**: el backpressure de vídeo jamás bloquea el bucle de sesión (ese es justamente el objetivo del stream dedicado); TCP queda idéntico a hoy. |
| Canal | `Channel<T>` (framing ARSP por longitud, C1) se reusa **tal cual** sobre un adaptador `StreamPair{send,recv}` que implementa `AsyncRead+AsyncWrite` (los streams de quinn ya los implementan para tokio). |
| Binding C2 | `Connection::export_keying_material(buf, BINDING_LABEL, None)` (RFC 5705 vía quinn) → **misma etiqueta `anyremote-auth-v1`** que hoy: la prueba ed25519 del peer se valida contra el binding local igual que en TCP. |
| ALPN | `anyremote-v1` en ambos lados (QUIC exige/permite ALPN; deja puerta a versiones). |
| Seguridad | El TLS de QUIC es **acuerdo de claves** (1.3 obligatorio); la autenticación sigue siendo **C2** (firma ed25519 + comparación de binding) — un intermediario que termine TLS a cada lado produce bindings distintos y C2 lo rechaza. Cliente: pin de `cert_store::load_trusted` si existe (rechazo temprano, como TCP) o **acepta-anything** si no (no hay bootstrap en claro en UDP) y tras C2 OK **persiste el cert del peer (TOFU)**. |
| Transport config | idle 30 s · ventana de stream 16 MiB (= `MAX_FRAME_BYTES`, un frame completo en vuelo) · ventana de conexión 32 MiB · hasta 8 bi-streams · sin 0-RTT (replays). Congestión: controlador por defecto de quinn. |
| App | toggle «UDP (QUIC)» en la fila principal (misma banda que los perfiles): se añade `--transport udp` a listen **y** connect; el hint de `netsh` menciona la regla UDP cuando está activo. |
| Eventos | `listening` lleva `transport`; nuevo `quic_fallback {reason}` en connect; el resto de eventos/auditoría sin cambios. |

### Por qué **no** datagrams de vídeo en v1 (decisión documentada)

Un datagrama perdido = **frame H.264 roto hasta el próximo IDR** (los
P-frame dependen del anterior; GOP ≈ 1–2 s): artefactos de segundos sin
retransmisión. Para corregirlo haría falta mini-ARQ (retransmitir con
deadline corto) o recuperación por keyframe — complejidad nueva con riesgo
de regresión. El **stream dedicado** da ya el win de QUIC frente a TCP (pérdida
de vídeo no bloquea control/audio/heartbeats) con frames en orden y sin
framing nuevo. **Futuro**: vídeo por datagrams con mini-ARQ + salto de frames
atrasados (el `mpsc` drop-new del v1 ya limita la antigüedad a ~2 frames).

### Límites de alcance (C-15)

- Sin NAT traversal/STUN/relay (LAN primero; WAN queda en §7).
- Sin renegociación de transporte en caliente (reconectar con el flag).
- `netsh` hint para UDP en la app (firewall de Windows también filtra UDP).


---

## 9. C-16: conexión por ID — resolutor LAN (2026-11-03)

**Objetivo (RF-03)**: `anyremoted connect GLV5Q76BAQ` y el campo «ID remoto o
dirección» de la app resuelven el **ID estable** a una dirección sin teclear la
IP. D-04 preveía un servidor propio (Fase 4/WAN); para LAN se implementa
**descubrimiento sin servidor**:

| Pieza | Diseño |
|---|---|
| Protocolo | UDP **broadcast** (puerto **21119**): el controlador emite `ANYREMOTE/1 WHO <id>` (hasta 3 intentos, ~2,7 s) y el anfitrión responde **unicast** `ANYREMOTE/1 HERE <id> <puerto_listen>` a la IP de origen (la respuesta viaja desde la IP correcta del anfitrión — sin ambigüedad multi-homing). Parse estricto: magia+versión+id+puerto; basura o versión desconocida → se ignora. **Fan-out (fix v0.11.1)**: la consulta va también a la **broadcast de cada subred local** (`if-addrs`) — en campo `255.255.255.255` salía por el adaptador equivocado. |
| Quién responde | Tarea `DiscoveryServer` durante `listen` (bind `0.0.0.0:21119`, se aborta al terminar la escucha; si el puerto está ocupado → aviso y la escucha sigue). Responde **solo si el ID pedido es el nuestro** + **throttling por origen** (≤1 respuesta/500 ms — patrón H-RD-02). |
| Caché | Tras una consulta exitosa se persiste `resolved/<ID>.txt` (`ip:port`). Si la broadcast no responde (firewall/subred), se usa la caché como respaldo. Sin caché y sin respuesta → error con el `netsh` exacto del puerto de descubrimiento. |
| **Seguridad** | La resolución **NO autentica** (cualquiera puede responder): la identidad la verifica **C2** en el handshake. Además `connect` por ID **exige que el par sea ese ID** (`peer_id.node_id == esperado`) ANTES de recordar TOFU/guardar pin — un impostor que redirija la dirección provoca error «no es el ID solicitado». El **pin por destino usa el ID** (`trusted/<ID>.pem`): estable entre cambios de DHCP. |
| Flag | `--discover <addr:port>` (default `255.255.255.255:21119`): listen ata `0.0.0.0:<puerto>`; connect envía a la dirección completa (en tests: `127.0.0.1:<puerto>`). |
| App | se elimina el bloqueo «por ID aún no se puede»; un ID **no es alias** (`looksLikeAlias` = sin `:`, así que el ID debe excluirla explícitamente) y se envía en mayúsculas. Hint `netsh` para UDP 21119. |
| Eventos | `resolve_ok {id, addr, source: broadcast\|cache}` en el controlador; errores con acción concreta (netsh / anfitrión escuchando). |
| Límites | LAN (broadcast no cruza subredes) — servidor propio/Rendezvous D-04 y WAN quedan en §7. El caché solo se escribe tras una respuesta viva (nunca se asume dirección). |


---

## 10. C-17: WAN — rendezvous + relay propio (D-04, 2026-11-03)

**Contexto de campo**: el router del usuario **no tiene UPnP/IGD** (recon:
SSDP sin respuesta, puertos IGD cerrados) → «abrir puertos solo» no es
posible; decisión del usuario: **servidor propio D-04 en su VPS**.

### Protocolo `anyremote-rdv` v1 (TCP único, JSON-Línea, por defecto `:21120`)

| Mensaje | Sentido | Notas |
|---|---|---|
| `{"t":"register","id","pk","port","ts","sig"}` | host → servidor | `sig = Identity::sign_auth(binding)` con `binding = "anyremote-rdv-v1 register {id} {pk} {port} {ts}"` — el **id DERIVADO del pk** hace imposible reclamar un id ajeno (H-RD-02); `ts` con ventana ±300 s y `(id,ts)` no repetible (anti-replay). `port` = puerto de escucha; el servidor combina con la **IP observada** del socket → endpoint directo público. |
| `{"t":"registered","relay"}` | servidor → host | El host mantiene la conexión con `hb` cada30 s; **`{"t":"invite","from","ticket","relay"}`** en cualquier momento. |
| `{"t":"resolve","id","ts"}` | controlador → servidor | Sin firma (directorio = dato semipúblico) + **throttling por IP** (H-RD-04). |
| `{"t":"answer","addr","relay","ticket"}` / `{"t":"error","code"}` | servidor → controlador | `addr` = ip observada + puerto registrado. El servidor crea el **ticket** (vida60 s, uso único) y envía el `invite` al host. |
| `{"t":"relay","ticket"}` → `{"t":"piped"}` | ambos → servidor | Admisión al **relay**: el servidor empareja los2 portadores del ticket y pasa a **tubería de bytes** (`copy_bidirectional`). |

### Cliente (`--rdv host:port` o fichero `config_dir/rdv`)

- **listen**: tarea persistente → `register` + `hb` (reconexión con backoff si
  el servidor cae; re-registro cada30 s = auto-sanación tras reinicios del
  servidor — el almacén es **en memoria**, v1). Un `invite` → se marca el
  stream para el bucle de escucha: dial al relay + `ticket` + **el mismo flujo
  que una sesión TCP entrante** (bootstrap → TLS accept → `establish_common`) —
  la tubería es un `TcpStream` normal, así que el TLS termina en los PARES
  (**E2E**: el relay es ciego; el relay se queda en TCP — QUIC solo en rutas
  directas).
- **connect** (tras `resolve_id`): orden **LAN primero** (broadcast → **rdv** →
  caché; la caché va DESPUÉS del rdv para que una caché vieja nunca tape una
  respuesta fresca del servidor). Intento **directo** al `addr` (QUIC/TCP según
  `--transport`, con **timeout de connect 8 s** — sin él, un SYN a un puerto no
  reenviado se colgaría20-40 s); si falla **y hay ticket** → **relay**:
  dial al servidor + `ticket` + `piped` + `tcp_controller_session` sobre la
  tubería (solo cambia de dónde sale el `TcpStream`).

### Seguridad y límites (decisión documentada)

- **Control en claro firmado** (igual que el broadcast de C-16): el registro
  está firmado con la clave del id (imposible suplantar id), la resolución solo
  da direcciones (un MITM puede negar/no equivocar → **C2 ancla la sesión**:
  la identidad se verifica igual al establecerla) y las sesiones son **E2E
  (TLS/QUIC entre pares)** — el relay transporta solo cifrado.
- Límites (D-08): tickets con60 s y uso único · throttle register/resolve por
  IP · parejas relay con idle-timeout60 s y cupo total · sin persistencia en v1.
- **Ancho de banda del relay**: el VPS vuelve y pasa todo el tráfico de la
  sesión (720p ≈2-6 Mbps) — coste a vigilar en el plan del servidor.
- Directo requiere **reenvío manual** en el router del anfitrión (TCP+UDP
  21118); sin reenvío → relay (funciona igual). UPnP queda descartado en esta
  red (recon) — se mantiene en §7 como no viable aquí.

### Reubicación (post-C-17): repo propio `serveremote`

El servidor + protocolo viven ahora en **https://github.com/brisaning/serveremote**
(paquete `serveremote`, binario `serveremote`, Dockerfile multi-etapa; repo
**público** porque anyremote lo consume como dependencia git:
`anyremote-rendezvous = { package = "serveremote", git = …, branch = "main" }`
— el Cargo.lock fija el commit y el código del daemon no cambió).
`src/identity.rs` allí es copia verbatim de `core/identity` (transcript
byte-idéntico); el `Identity` del cliente **no cruza la frontera**: el daemon
lo envuelve en `RdvSigner` (implementación del trait `RegisterSigner`).

### Despliegue (VPS)

**Docker (recomendado)**: `docker build -t serveremote .` + `docker run -d -p
21120:21120/tcp --restart unless-stopped serveremote` desde el repo
`serveremote` (guía completa en `evidence/rdv-relay-gate.md`) · firewall: solo
TCP21120 (control+relay en el mismo puerto). Los clientes: campo «servidor de
encuentro» de la app, `--rdv <ip-vps>:21120` o fichero `rdv`.


---

## 11. C-19: render alto — pipeline sin nativo (2026-11-04)

**Disparador**: backlog «Render a alta resolución → posible paso a widget
`Texture` si el fps de render limita» (README § Pendientes 4). Diagnóstico del
pipeline en `app/lib/main.dart`:

1. **`_onVideoBytes` era cuadrático**: cada chunk de TCP recopiaba el buffer
   COMPLETO (`merged = old + chunk`) → un frame de 720p (≈3,7 MB) llega en
   ~200 chunks ⇒ ~200 `memcpy` de hasta 3,7 MB **por frame**.
2. **`setState` de la página entera por frame** (~30 Hz) y **otro por el
   cursor remoto** (~20 Hz): reconstruían AppBar, chips, toggles y registro.
3. **Recursos de decode al GC**: `ImmutableBuffer`/`ImageDescriptor`/`Codec`
   sin `dispose` (a 1080p ≈ 8 MB/frame de presión de GC).

**Solución (v0.12.2)** — todavía sin widget nativo:

- **`VideoFrameAssembler`** (`app/lib/video_assembler.dart`): buffer con
  crecimiento por duplicación y compactación reutilizando capacidad
  (amortizado); **una sola copia** de payload por frame; resincronía por
  cabecera inválida contada en `dropped` (mismo comportamiento que antes).
- **Notifiers acotando el repintado**: `_frame` (imagen), `_videoStatus`
  (cabecera, tope 2 Hz) y `_cursor` (overlay) → un único `ListenableBuilder`
  repinta **solo el subtree del visor**.
- **Disposal por frame** de `codec`/`desc`/`buf` (patrón de
  `instantiateImageCodecWithSize` del SDK: el buffer se libera al crear el
  codec) + la imagen vieja al reemplazarse. Wire y daemon intactos.

**`Texture` nativo: CONDICIONADO a medición de campo.** Medir el `fps render`
de la cabecera del visor a 720p/1080p/«Nativa» con este build; solo si no
llega a los fps del stream se plantea el paso a `Texture` (decode fuera de
Flutter: wire de preview + plataforma Windows — esfuerzo L). Si el pipeline
actual llega, el backlog queda **cerrado sin nativo**.
