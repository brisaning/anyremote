# PLAN DE IMPLEMENTACIÓN — `anyremote` (Organic Driven Development)

| Campo | Valor |
|---|---|
| Versión | 1.0.0 |
| Fecha | 2026-10-26 |
| Estado | Aprobado por el usuario (decisiones §0.1 registradas) — **ejecución en curso** (ver estado dinámico abajo) |
| Spec origen | [01-requerimientos.md](01-requerimientos.md) |
| Metodología | Organic Driven Development (ODD) |

## Estado de ejecución (dinámico — actualizar al cerrar cada fase/ciclo/lote)

_Última actualización: 2026-11-21 · **C-45 ✅ · C-44 ✅ (v0.14.3)** · **C-52 Fase A ✅** (causa raíz de la regresión de vídeo: la escalera razona en bitrate/escala y no en bpp, y no recodifica el contenido estático — `evidence/video-quality-regression.md`) · **nuevo lote de campo (§13): C-52 vídeo (Fase B pendiente), C-53 aceptar=control, C-54 Caps Lock, C-55 layout de teclado** · seguridad C-46…C-51 pendiente (§12) · semilla `evidence/field-2026-11-20.md`_

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
| C-21 «Compartir escritorio» + control remoto automático v0.12.4 | ✅ gate verde (Flutter 18/18 con aserción nueva; Rust 51/0) · petición de campo del usuario | `share-desktop-v0.12.4` · `evidence/share-desktop-gate.md` |
| C-22 rdv vacío con default efectivo + auto-detección LAN/WAN v0.12.5 | ✅ gate verde (Flutter 21/21 con3 tests nuevos; Rust 51/0) · petición de campo del usuario | `lan-wan-auto-v0.12.5` · `evidence/lan-wan-auto-gate.md` |
| C-23 arranque en frío: GDI + input en placeholder v0.12.6 | ✅ gate verde (Flutter 22/22 con test nuevo; capture 5/5 con `gdi_snapshot`; Rust 51/0) · disparador: revisión de logs de campo (2 sesiones relay sin vídeo ni input) | `cold-start-v0.12.6` · `evidence/cold-start-gate.md` |
| C-24 encoder: reconstrucciones coalescadas + reenvío rápido v0.12.7 | ✅ gate verde (Rust 51/0 con2 tests nuevos; Flutter 22/22) · reporte de campo («conexión sin pantalla», relay+estático) | `encoder-warmup-v0.12.7` · `evidence/encoder-warmup-gate.md` |
| C-25 `video_error` con detalle v0.12.8 | ✅ gate verde (Flutter 23/23 con test nuevo; Rust 51/0) · revisión de logs de campo (4 sesiones WAN exitosas en3 modos + audio) | `video-error-detail-v0.12.8` · `evidence/video-error-log-gate.md` |
| C-26 framing H.264 + decoder auto-sanado v0.12.9 | ✅ gate verde (Rust 51/0 con3 tests nuevos; Flutter 23/23) · reporte de campo (juank/juank2: `native:18` en243/261 frames = vídeo congelado) | `h264-framing-v0.12.9` · `evidence/h264-framing-gate.md` |
| C-27 usable: exceso de RTT + IDR + log tasaado v0.13.0 | ✅ gate verde (Rust 51/0 con3 tests nuevos; Flutter 24/24) · reporte de campo («ventanas por saltos»: relay11 frames/78 s +146/77 `native:18` +735 líneas de log) | `adapt-excess-v0.13.0` · `evidence/adapt-excess-gate.md` |
| C-28 encoder sin tirar frames + instrumentación v0.13.1 | ✅ gate verde (Rust 51/0 con test de cola; Flutter 25/25) · diagnóstico por correlación de logs (enviado=decodificado byte a byte, pero solo~1,5 fps emitidos) | `encode-pending-v0.13.1` · `evidence/encode-pending-gate.md` |
| C-29 adaptación: recuperación alcanzable + escalera acotada v0.13.2 | ✅ gate verde (Rust 51/0 con2 tests nuevos; Flutter 25/25) · reporte de campo (WAN:6 escalas/2 min sin recuperación →850x478@200k = texto ilegible; movimiento fluido por fps intactos) | `adapt-ladder-v0.13.2` · `evidence/adapt-ladder-gate.md` |
| C-30 conectividad: reintento de relay tras fallo directo v0.13.3 | ✅ gate verde (Rust 51/0 con test nuevo `lan_resolved_direct_failure_retries_rdv_and_uses_relay`; Flutter 25/25) · campo 17/11: Julie→Brian resolvía por broadcast pero el puerto 21118 estaba bloqueado (sin regla de firewall) y el daemon moría con `timeout (8 s)` sin fallback | `relay-rdv-retry-v0.13.3` · `evidence/relay-rdv-retry-gate.md` |
| C-31 rendimiento: escalado separable + tiempos por etapa v0.13.4 | ✅ gate verde (Rust 51/0; Flutter 25/25) · campo 17/11: Brian↔Julie por LAN directa (rtt 3–4 ms) y mismo 720p@15; Julie→Brian menos fluido porque el anfitrión Brian (escritorio 4K) produce 0,8–5,2 fps vs 5–9 fps de Julie → cuello en el bucle de vídeo del anfitrión (escala 4K→720p en CPU con `f32` por píxel) | `scale-fast-v0.13.4` · `evidence/scale-separable-gate.md` |
| C-32 legibilidad de texto: presets de bitrate v0.13.5 | ✅ gate verde (Flutter 25/25; Rust sin cambios 51/0) · campo 17/11 (Julie cliente 1080p ← Brian host 4K): texto ilegible en 720p/1080p/4K con 3/6/8 Mbps (0,22/0,19/0,064 bpp/frame); `escala_ms` y `encode_ms` descartan el escalado/códec como causa → presupuesto de bits | `text-bitrate-v0.13.5` · `evidence/text-quality-gate.md` |
| C-33 legibilidad de texto: zoom 1:1 + encoder v0.13.6 | ✅ gate verde (Rust 154/0 · clippy 0 · fmt 0 · Flutter 26/26) · campo 18/11 (Julie 1080p ← Brian 3840×2160@125 %): `bytes_sent` 2,92 MB/88 s ≈ 265 kbps con objetivo 15 Mbps y 101 frames; en laboratorio 2/6/15/25 Mbps dan el MISMO stream (el MFT software ignora el rate-control, antes escrito en `GetAttributes`); el cauce real es la resolución: el texto pasa de 16 px a **8 px** al emitir 1080p → visor **Ajustar/100 %/150 %** + log de escala, kbps/KB-frame/bpp en el daemon, perfil Main y tasa en vivo | `viewer-zoom-v0.13.6` · `evidence/viewer-zoom-gate.md` |
| C-34 barras del visor v0.13.7 | Campo (C-33 validado): «con esto se resuelve, hace falta una barra para el desplazamiento horizontal» | Las dos `Scrollbar` envuelven el **viewport** (antes la horizontal se pintaba al final del contenido, a 2.880 px, y no se veía); grosor 10 px, `thumbVisibility`, filtro por eje; test C-34 con frame 1200×800 en ventana 800×600 | `viewer-scrollbar-v0.13.7` · `evidence/viewer-scrollbar-gate.md` |
| C-35 encoder de GPU + conversión rápida v0.13.8 | El MFT de software ignoraba el bitrate (C-33) y la conversión de color costaba más que el encoder | NVENC asíncrono vía `MF_TRANSFORM_ASYNC_UNLOCK` + eventos (CBR real: 10,7/38,6/105,5 KB a 2/6/15 Mbps; 11–12 ms a 1080p); `bgra_to_nv12_into` en una pasada paralela (16,2→3,4 ms a 1080p; 52,3→9,3 ms a 4K); caída al síncrono y guarda >1080p (4K asíncrono se atasca) | `gpu-encoder-v0.13.8` · `evidence/gpu-encoder-gate.md` |
| C-36 escalado paralelo v0.13.9 | Campo 18/11 con C-35: NVENC confirmado (`encode_ms` 8,4–8,9; tasa real 1,3–5,6 Mbps; **0 auto-reset**) → `escala_ms` 32–48 ms era el cuello | `FrameScaler`: tablas cacheadas + `scratch` reutilizado + dos pasadas por bandas en hilos → **36,6→13,5 ms** (4K→1080p, 2,7×), idéntico byte a byte (test) | `scale-parallel-v0.13.9` · `evidence/scale-parallel-gate.md` |
| C-37 acentos + teclas de edición v0.13.10 | BUG pospuesto (15/11): `´`+vocal escribía `´o`; la dead key viajaba como carácter y el backend dependía del layout | App: `composeDeadAccent` (´ ` ^ ¨ ~ + vocal, mayúsculas, doble pulsación, volcado antes de teclas no-carácter); backend: no ASCII por `KEYEVENTF_UNICODE`, ASCII por VK; nuevas `Insert`/`RePág`/`AvPág`/`CapsLock`/`NumLock`/`ScrollLock` (wire 12/13/14/23/24/25) | `accents-v0.13.10` · `evidence/input-accents-gate.md` |
| C-38/C-39 acentos, navegación y 4K v0.13.11 | Campo C-37: `´ó` (el SO ya compone) y RePág/AvPág/flechas **seleccionaban** texto; 4K «atascado» en C-35 | `resolveDeadKey` descarta la dead key si el carácter ya viene compuesto; `shift=false` en 13 teclas de navegación; 4K NVENC 24/24 a 25,5 ms (era la sonda en bucle cerrado), muestras en vuelo y caída al síncrono si 4 entradas no dan salida | `input-4k-v0.13.11` · `evidence/accents-4k-gate.md` |
| C-40 diagnóstico de audio v0.13.12 | Campo 12:48 con mic: el anfitrión anunció `mic activo` pero el cliente decía «sin señal: ¿reproduce sonido el anfitrión?» y el cierre no traía contadores | Cierre con `audio_sent`/`audio_recv`; `video_adapt` con `rtt`; el visor etiqueta `mic ✓`/`mic: sin señal` según `audio_start` | `audio-diag-v0.13.12` · `evidence/audio-diag-gate.md` |
| C-41 WAN + micrófono v0.13.13 | Prueba WAN (relay) con micro: el audio llegó (`audio_sent=100` → `audio_recv=100`) pero el relay cayó (`connection_gone`, errores 10065/10013) y el visor mostró la última imagen **50 s** sin avisar | Watchdog de vídeo en el cliente (>5 s sin frames → «sin vídeo desde hace N s (¿red?)», fps a 0 y una línea de registro); `rtt` en el registro de `calidad adaptada`; helper `stallSeconds` con test | `wan-stall-v0.13.13` · `evidence/wan-mic-gate.md` |
| C-42 decisión `fps render`/`Texture` v0.13.14 | Pendiente de C-19: ¿hace falta `Texture` nativo? No había medición del coste real del visor | Contadores `recv/render/drop/coste` en el log de 5 s; banco en release: **11,0 ms/frame a 1080p** (66 ms de presupuesto, drop=0) → **no se implementa Texture**; cierre de Juank/Juank2 (fluido, validación del usuario) | `render-v0.13.14` · `evidence/viewer-render-gate.md` |
| C-43 «Conectar» deja de compartir v0.13.15 | Con el escritorio compartido, pulsar Conectar lanzaba un segundo daemon y el equipo quedaba como anfitrión y cliente a la vez | Conectar cierra el listener primero (`bye` + espera, máx. 3 s) y después arranca la sesión; con sesión activa no se abre la escucha (aviso) | `connect-share-v0.13.15` · `evidence/connect-stops-sharing-gate.md` |
| **C-44 autorización del par** (F-01 · RNF-07) **v0.14.0** (+ fix **v0.14.1**) | ✅ **gate verde** (Rust 169/0 · clippy 0 · fmt 0 · Flutter 34/34) · campo: **diálogo no aparecía con IDs heredados → corregido en v0.14.1** (`seen` vs `user`) · semilla F-01: C2 solo demuestra posesión de una clave, `peer_trusted` no decidía y el `granted` lo respondía el par → **cualquier equipo que alcanzara 21118 obtenía control** | **Política `--accept trusted\|ask\|any`** (default `trusted`; la app usa `ask` con diálogo), **admisión antes de `send_auth`**, TOFU solo de pares aceptados, `--trust ID`, timeout 30 s, y **grant del anfitrión = intersección** (el par no amplía) | `peer-authz-v0.14.0` · `evidence/peer-authz-gate.md` |
| **C-45 entrada realmente opt-in** (F-02 · RNF-08) **v0.13.16** (+ integración **v0.14.2/v0.14.3**) | ✅ **gate verde** (Rust 172/0 · Flutter 35/35) · campo 20/11: aceptar la sesión no daba control → **el diálogo incluye «Permitir control remoto»** (marcado por defecto) y el **estado del toggle persiste entre sesiones** · F-02: `buildListenArgs` armaba `--allow-input` siempre y «Compartir escritorio» forzaba el toggle; el daemon nacía con `input_on = cfg.allow_input` | **Capacidad ≠ estado**: `--input-on` (CLI, default off) separa el estado inicial; `DynamicPerms.input_on = allow_input && input_on_start`; la app no pasa `--input-on` (solo `--allow-input` para el toggle dinámico); sin auto-encendido, `shareSummary` («solo ver»), chip en la cabecera, `listening` con `input_initial` y `InputDenied` con `reason` | `input-optin-v0.13.16` · `evidence/input-optin-gate.md` |
| **C-46 auditoría sin contenido tecleado** (F-05 · RNF-10) v0.14.2 | 🔴 **planificado** · F-05: cada tecla inyectada se escribe como `KeyDown(Char('x'))` en `audit/*.jsonl`, carpeta que el README manda compartir para soporte | Diseño §12.3 · detalle `{"perm","kind"}` sin `Char`/texto + nota de migración de logs · gate `audit_never_contains_typed_text` · `evidence/audit-privacy-gate.md` |
| **C-47 compuerta de portapapeles** (F-03 · RNF-08) v0.14.3 | 🔴 **planificado** · F-03: el portapapeles del anfitrión se envía cada 500 ms y se sobrescribe con el del par sin permiso ni toggle (la app pasa `--clipboard win` fijo) | Diseño §12.4 · permiso `clipboard` (default off) + cuarto toggle en la UI · gates `clipboard_out_requires_grant`, `clipboard_in_rejected_without_grant` · `evidence/clipboard-gate-v2.md` |
| **C-48 archivos acotados y no bloqueantes** (F-04 · RNF-08/RNF-09) v0.14.4 | 🔴 **planificado** · F-04: recepción sin compuerta ni cuota (`max_files_per_batch` no se aplica), 256 MiB en RAM por archivo y `.part` persistentes; `receive_file_offer` dentro del `select!` bloquea control, `bye` y Ctrl+C | Diseño §12.5 · permiso `files`, cuota de sesión, tarea aparte con timeout y cancelación, limpieza de huérfanos · gates `file_offer_without_grant_rejected`, `stalled_transfer_does_not_block_control`, `receiver_enforces_batch_limit` · `evidence/filexfer-gate-v2.md` |
| **C-49 decodificador con cotas** (F-06 · RNF-09) v0.14.5 | 🔴 **planificado** · F-06: `Deflate` → `read_to_end` sin tope (bomba zlib) y `width*height*4` en `u32` (overflow); el formato del frame lo elige el par | Diseño §12.6 · tope de salida, `checked_mul`, rango de dimensiones · gates `deflate_bomb_is_rejected`, `oversized_dimensions_rejected` · `evidence/codec-bounds-gate.md` |
| **C-50 preview autenticado** (F-07 · RNF-09) v0.14.6 | 🔴 **planificado** · F-07: el primer cliente de `127.0.0.1:<efímero>` recibe BGRA crudo de la pantalla remota sin credencial | Diseño §12.7 · token de un solo uso en la cabecera del stream · gate `preview_requires_token` · `evidence/preview-auth-gate.md` |
| **C-51 higiene de confianza y contadores** (F-08 · RNF-07) v0.14.7 | 🔴 **planificado** · F-08: `AuthGate` inalcanzable (aborta en el primer fallo) y sin rate-limit; «✓ verificado» se pinta con `peer_trusted` (cualquier par nuevo); `Scroll` multiplica sin comprobar | Diseño §12.8 · contador real + rate-limit por origen, etiqueta «visto antes» vs allowlist, `checked_mul` · gates `auth_gate_counts_failures`, `trusted_badge_means_allowlisted` · `evidence/trust-hygiene-gate.md` |
| **C-52 regresión de calidad de vídeo** (campo 20/11 · prioridad) | 🟡 **Fase A ✅** (2026-11-21) — causa raíz sustentada en `evidence/video-quality-regression.md`: la escalera razona en **bitrate/escala** y no en **bpp**, y **no recodifica** el contenido estático al recuperar. **D1** sin suelo de bpp (tasa inicial `w*h*3` = 0,200 bpp; suelo 200 kbps = 0,003–0,006 bpp) · **D2** recuperación inalcanzable con el jitter del relay (zona neutra 100–200 ms sólo degrada `net_low`) → 2 min 35 s congelada en el suelo · **D3** al subir el bitrate no se pide IDR (el trait `VideoEncoder` no tiene método de refresco) → el texto conserva la calidad vieja. H1 y H2 **confirmadas**; H3 y H4 **descartadas** con datos · arnés determinista (5 tests, `core/daemon/src/video.rs` `mod c52_ladder`) que reproduce las 21 decisiones del colapso de campo | 🟡 **Fase B pendiente** (política bpp por tamaño + refresco + recuperación con jitter) · 🔴 **Fase C** (campo relay + script de 2 min) · gates `ladder_preserves_bpp`, `static_text_refreshes_after_recovery`, `recovery_reachable_under_jitter` · `evidence/video-quality-regression.md` |
| **C-53 aceptar = control** (UX C-44/C-45) | 🔴 **planificado** · campo 20/11: la casilla «Permitir control remoto» sobra; aceptar una sesión remota implica poder controlar | Diseño §13.2 · quitar la casilla (aceptar envía `input:true`), «solo ver» como modo explícito opcional · gates `accept_grants_control` + widget · `evidence/accept-control-gate.md` (por producir) |
| **C-54 Caps Lock no conmuta** (RF-02a) | 🔴 **planificado** · campo 20/11: mayúsculas no funciona en el anfitrión | Diseño §13.3 · reproducir en wire/backend, estado sincronizado del toggle · gates `capslock_toggles_host` · `evidence/capslock-gate.md` (por producir) |
| **C-55 teclado independiente del layout** (RF-02a) | 🔴 **planificado** · campo 20/11: la distribución del anfitrión manda; un cliente con otro layout escribe caracteres equivocados | Diseño §13.4 · investigación AnyDesk/RustDesk (clean-room D-01), diseño carácter-primero (Unicode) y atajos por scancode, fase de campo con dos layouts · `evidence/keyboard-layout-gate.md` (por producir) |
| **Pendientes** | 🔴 **campo primero: C-52 (vídeo) → C-53 (UX) → C-54 (Caps)**, luego la seguridad **C-46…C-51** y **C-55** (teclado global) · (C-44 ✅ v0.14.0, C-45 ✅ v0.13.16) · 🐛 **BUG acentos** (dead key como `Char` → `´o`; fix = precomposición + Unicode, POSPUESTO) · 🟡 repetir sesión WAN (~10 Mbps): debe haber `red_recupera_*` + escala que SUBE y texto legible · `test de campo de5 s` (captura vs emitidos) · reenvío puertos21118 · **`fps render` (decisión `Texture`)** · micrófono end-to-end · **firewall de entrada en Brian**: crear reglas 21118/21119 para LAN directa (ver C-30) | [README § Pendientes](../README.md#pendientes) |

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

---

## 12. Blindaje de fronteras de confianza — auditoría 2026-11-19 (F-01…F-08)

**Disparador.** Revisión estática de las fronteras críticas del 2026-11-19
(commit `6967620`) sobre `transport`, `identity`, `daemon`, `input`,
`filexfer`, `clipboard`, `codec` y el cableado de la app. Semilla completa y
evidencia por ruta/línea: `evidence/security-review-2026-11-19.md`. No hubo
ejecución del objetivo: los ciclos de abajo incluyen la validación local que
falta (loopback, sin red externa, con límites).

**Regla del lote.** Estos ciclos **preceden** al resto del backlog: F-01 y F-02
son altos y convierten los demás hallazgos en explotables por un tercero. El
orden ya está fijado por prioridad; no se reordena por coste.

| Orden | Ciclo | Hallazgo | RNF | Capacidad afectada |
|---|---|---|---|---|
| 1 | C-44 | F-01 · sin autorización del par | RNF-07 | admisión de sesión |
| 2 | C-45 | F-02 · entrada armada al compartir | RNF-08 | entrada remota |
| 3 | C-46 | F-05 · teclas en claro en auditoría | RNF-10 | registro |
| 4 | C-47 | F-03 · portapapeles sin compuerta | RNF-08 | portapapeles |
| 5 | C-48 | F-04 · archivos sin cuota y bloqueantes | RNF-08/09 | archivos |
| 6 | C-49 | F-06 · decodificador sin cotas | RNF-09 | vídeo (receptor) |
| 7 | C-50 | F-07 · preview local sin autenticación | RNF-09 | preview |
| 8 | C-51 | F-08 · higiene de confianza | RNF-07 | UI/registro |

**Protocolo.** C-44, C-45, C-46, C-49, C-50 y C-51 **no** tocan el wire. C-47 y
C-48 usan el `repeated string permissions` que ya existe
(`core/transport/proto/arsp.proto:210-218`): añadir `"clipboard"` y `"files"`
no cambia el esquema ni rompe peers viejos (fail-closed: sin negociación no se
concede). Ningún ciclo introduce un mensaje nuevo.

### 12.1 C-44 · Autorización del par (F-01) — ✅ cerrado v0.14.0

**Objetivo.** Que «autenticado» ≠ «autorizado»: el anfitrión decide quién entra.

**Cambios (implementados; el diseño inicial se ajustó al orden real del
handshake y al default del binario).**
- `DaemonConfig.accept: AcceptPolicy` (`trusted` | `ask` | `any`), flags
  `--accept`, `--trust ID[,ID]` y `--accept-timeout` (30 s). **Default
  `trusted`** en el daemon/CLI; la app pasa `ask`; `any` solo explícito
  (`insecure_accept_any()` en tests).
- `admit_peer` se ejecuta tras `recv_validate_auth` y **antes de `send_auth`**
  (no se revela la identidad propia a un par rechazado). Solo de un par
  aceptado se hace TOFU y se emiten `session_established`/`session_active`.
  - `trusted`: confiado o `--trust` → acepta y recuerda; si no,
    `peer_rejected{peer_unknown}` + `Established::Denied`.
  - `ask`: `session_pending{peer, peer_addr}` y espera por `ctl` un
    `{"accept":"<ID>","trust":bool}` / `{"reject":"<ID>"}`; un par ya confiado
    entra sin preguntar; sin respuesta en el timeout → deniega. Los
    `{"perm":...}` recibidos mientras se espera se encolan para la sesión.
  - `any`: comportamiento previo (documentado como inseguro).
- **Grant del anfitrión = intersección** (`host_grant`): `PermissionResponse`
  es intención del par, no autorización; un permiso no ofrecido no se activa y
  `granted=false` no concede nada. Auditoría `permission_granted{source:"host"}`.
- App: diálogo «Sesión entrante» (Rechazar / Aceptar una vez / Aceptar y
  confiar) que escribe `admissionLine(...)` por stdin; `peer_rejected` y
  `stale_decision` van al registro. Una decisión tardía no cierra la sesión.
- Un `bye` durante la espera de `ask` detiene la escucha (el rechazo no deja el
  proceso colgado).

**Fix de campo (v0.14.1).** El primer uso real reveló que `ask` auto-aceptaba
las entradas de `trusted.ids` creadas por el TOFU **anterior** a C-44 (sin
consentimiento): el cliente veía la pantalla y el anfitrión no recibía
`session_pending`. Se añadió `TrustOrigin { User, Seen }`: el archivo v2 guarda
`ID=hex:user|:seen` y **sin sufijo se interpreta como `seen`** (migración); solo
`user` autoriza en `trusted`/`ask`, y `any` recuerda `seen` sin degradar `user`.
Gates nuevos: `ask_mode_prompts_for_legacy_seen_peer` (e2e),
`legacy_entries_are_seen_and_only_user_trust_authorizes` (unit) y el widget del
diálogo. Detalle en `evidence/peer-authz-gate.md`.

**Gate (verde).** `unknown_peer_rejected_by_default`,
`trusted_peer_gets_active`, `ask_mode_blocks_until_host_decision`,
`ask_mode_rejects_on_host_decision`, `ask_mode_prompts_for_legacy_seen_peer` + unitarios `host_grant_is_intersection_and_never_expands`,
`admission_decision_parses_only_for_the_pending_peer`,
`accept_policy_defaults_to_trusted`; los e2e previos declaran
`insecure_accept_any()` de forma explícita. Evidencia: `evidence/peer-authz-gate.md`.

### 12.2 C-45 · Entrada remota realmente opt-in (F-02) — ✅ cerrado v0.13.16

**Objetivo.** Compartir pantalla no concede teclado ni ratón.

**Cambios (implementados; el diseño inicial se ajustó al separar capacidad y
estado — la app mantiene la capacidad para no perder los toggles dinámicos).**
- `core/daemon/src/lib.rs`: `DaemonConfig.input_on_start` (nuevo, default
  `false`) y flag `--input-on` en `main.rs`; `DynamicPerms::new` usa
  `allow_input && input_on_start`. Separación **capacidad** (`--allow-input`,
  sin ella ninguna orden enciende) vs **estado** (arranca off; lo enciende
  `{"perm":{"input":true}}`).
- `app/lib/main.dart`: `buildListenArgs` conserva `--allow-input` (capacidad,
  necesaria para el toggle en caliente) y **nunca** pasa `--input-on`;
  `_startLocalListen` deja de forzar el toggle; `shareSummary(...)` informa
  «solo ver» o «acepta …»; chip «solo ver» en la cabecera del visor.
- Observabilidad: evento `listening` con `input_capability`/`input_initial` y
  `InputDenied` con `allow_input` (capacidad), `input_on` (estado) y `reason`.
- Docs: README y `--help` documentan `--allow-input --input-on` para la CLI.

**Integración con C-44 (v0.14.2/v0.14.3).** El primer uso real mostró la
incoherencia: autorizar la sesión no autorizaba la entrada y el cliente quedaba
en «solo ver» sin que el anfitrión lo entendiera. El diálogo de sesión entrante
incluye la casilla **«Permitir control remoto»** (marcada por defecto), envía
`{"perm":{"input":...}}` **antes** de la decisión (encolado, aplicado al abrir
la sesión) y avisa si se acepta sin control. Además (v0.14.3) el estado del
toggle es un `Arc<AtomicBool>` del `DaemonConfig` que **persiste entre
sesiones** mientras el listener vive, y `ask` solo auto-acepta a un par `user`
si el control está encendido. Gates: widgets del diálogo,
`input_state_persists_between_sessions_and_needs_capability`,
`ask_mode_auto_accepts_user_trusted_peer_when_control_is_on`,
`ask_mode_prompts_trusted_peer_when_control_is_off`. Detalle:
`evidence/peer-authz-gate.md`.

**Gate (verde).** e2e dirigido por eventos
`input_off_by_default_with_capability_and_enabled_by_perm` (capacidad armada →
denegación `toggle_apagado` → `{"perm":true}` → inyección → acuses y `bye`);
regresión `input_partial_grant_*`, `input_full_grant_*`,
`input_denied_by_default_without_allow_input` y `input_toggles_at_runtime`;
widget de `buildListenArgs`/`shareSummary` y aserción de que el checkbox sigue
apagado tras «Compartir escritorio». Evidencia: `evidence/input-optin-gate.md`.

### 12.3 C-46 · Auditoría sin contenido tecleado (F-05)

**Cambios.**
- `core/daemon/src/lib.rs:1677-1678`: el detalle pasa a
  `{"perm": "<keyboard|mouse>", "kind": "key|mouse"}`; prohibido
  `format!("{iev:?}")` sobre `InputEvent`. Si se necesita correlación, un
  `u64` de hash no reversible, nunca el carácter.
- Revisar los demás `detail`: `clipboard_sync` ya es solo `bytes`;
  `session_error` puede llevar rutas (aceptable) pero no contenido.
- README §Diagnóstico: nota de que los logs **anteriores** a C-46 pueden
  contener texto tecleado y no deben compartirse.
- Test estático (estilo C8, `transport/tests/security_audit.rs`) que prohíbe
  `InputEvent`/`Char` en los `detail` de auditoría.

**Gate.** `audit_never_contains_typed_text`: e2e que inyecta una cadena
centinela y verifica que **no** aparece en el JSONL de la sesión.

### 12.4 C-47 · Compuerta de portapapeles (F-03)

**Cambios.**
- Permiso `"clipboard"` en `requested`/`granted` y campo
  `DynamicPerms.clipboard` (**default `false`**).
- `core/daemon/src/lib.rs:1721-1760` solo envía si el permiso está concedido;
  `:1596-1614` solo aplica si está concedido (si no, `clipboard_denied`).
- App: cuarto toggle «Portapapeles» (default off) y `_sendPerms` amplía el
  JSON a `{"perm":{"input","clipboard","audio","mic"}}`.
- Peer viejo sin el permiso → no se sincroniza (fail-closed, una línea de
  registro por sesión, sin spam).

**Gate.** `clipboard_out_requires_grant` y `clipboard_in_rejected_without_grant`
(e2e con `--clipboard win` y sin concesión), widget del toggle y actualización
de `evidence/clipboard-gate-v2.md` con la matriz de los cuatro estados.

### 12.5 C-48 · Archivos acotados y no bloqueantes (F-04)

**Cambios.**
- Permiso `"files"` (default off; `--one-way-files` sigue siendo la política de
  sentido). `file_policy` deja de ignorar la habilitación.
- `TransferPolicy`: `max_session_bytes` y `max_concurrent`; `accept_offer`
  aplica `max_files_per_batch`, `max_concurrent` y la cuota (hoy solo valida
  tamaño y nombre, `core/filexfer/src/lib.rs:260-294`).
- Recepción **fuera** del `select!`: `receive_file_offer` pasa a tarea con
  `mpsc` al bucle de control, `timeout` por chunk (30 s) y cancelación por
  `bye`/Ctrl+C. El `select!` vuelve a atender entrada, latidos y cierre aunque
  la transferencia esté a medias.
- `TransferStore::sweep()` al abrir y cerrar sesión: borra `.part`/`.meta`
  huérfanos (más antiguos que N días o de sesiones ya cerradas).

**Gate.** `file_offer_without_grant_rejected`,
`receiver_enforces_batch_limit` y `stalled_transfer_does_not_block_control`
(con oferta a medias, `bye` local cierra en <1 s y la sesión deja de retener
memoria).

### 12.6 C-49 · Decodificador con cotas (F-06)

**Cambios.** `core/codec/src/lib.rs:202-219`: `checked_mul` para
`width*height*4`, `MAX_PIXELS` (p. ej. 4096×4096) rechazando lo que exceda, y
descompresión con `Read::take(expect as u64 + 1)` + error si sobrepasa. Aplicar
lo mismo al backend VP9/OpenH264 si aceptan dimensiones del wire.

**Gate.** `deflate_bomb_is_rejected` (frame comprimido que expande > tope → error
sin reservar) y `oversized_dimensions_rejected` (w/h absurdos → `Decode`, sin
pánico en debug).

### 12.7 C-50 · Preview autenticado (F-07)

**Cambios.** `core/daemon/src/preview.rs`: token de 16 bytes de `OsRng`
publicado en el evento `preview {port, token}`; el cliente envía el token como
primeros 16 bytes y sin coincidencia exacta se cierra. La app lo pasa al abrir
el socket. Alternativa futura: named pipe con ACL del usuario.

**Gate.** `preview_requires_token` (conexión sin token o con token erróneo →
cierre inmediato; con token → frames).

### 12.8 C-51 · Higiene de confianza y contadores (F-08)

**Cambios.** `AuthGate` cuenta fallos de verdad y solo corta al superar
`MAX_AUTH_FAILURES`; `run_listen` añade rate-limit por `SocketAddr` (tabla
acotada, p. ej. 5 intentos/60 s). El indicador de la UI deja de decir
«✓ verificado» por `peer_trusted`: pasa a «visto antes» salvo que el ID esté en
`trusted.ids` (origen explícito en el evento). `Scroll` usa `checked_mul` con
clamp a `u32`.

**Gate.** `auth_gate_counts_failures`, `trusted_badge_means_allowlisted` y
widget del indicador.

### 12.9 Cierre del lote

Cada ciclo cierra con: gates verdes (`cargo test` + `clippy -D warnings` + `fmt`
+ `flutter analyze/test`), su `evidence/*-gate.md` con la invocación exacta, tag
`<ciclo>-v0.14.x` en `develop`, fila actualizada en la tabla de arriba y en
`specs/04`, y nota en la bóveda. El lote no se declara cerrado hasta que
F-01…F-08 tengan ciclo con evidencia o una decisión explícita de aceptación del
riesgo registrada en `specs/04`.

---

## 13. Lote de campo 2026-11-20 — regresiones de vídeo, UX de aceptación y teclado

**Disparador.** Reporte del usuario tras la sesión del 20/11 (cliente 0.14.1 →
anfitrión 0.14.3). Evidencia y trazas cruzadas:
`evidence/field-2026-11-20.md`. Tres frentes independientes; **cada ciclo se
cierra y valida por separado** (lección: no mezclar seguridad con UX/vídeo en
el mismo lote).

**Orden recomendado:** C-52 (el vídeo ilegible bloquea el uso) → C-53 (UX,
minutos) → C-54 (Caps Lock) → retomar seguridad C-46 → C-55 (teclado global,
requiere investigación). Todos se planifican aquí para sesiones distintas.

### 13.1 C-52 · Regresión de calidad de vídeo/texto (prioridad de campo)

**Síntoma.** Con la sesión por **relay** (351 ms) a 1080p, la imagen queda en
0,18–0,47 bpp: movimiento fluido (render 12–20 ms, `drop=0`) y **texto
ilegible**; al subir el tope adaptativo a 15 Mbps no se recupera.

**Fase A — reproducir y medir (sin cambiar la política).** ✅ **hecha
2026-11-21** (`evidence/video-quality-regression.md`, rama
`feature/c52-video-regression`).
1. Reproducir el camino real y medir `bpp = kbps / (w·h·fps)` en el anfitrión
   y el visor → **hecho con los logs de campo**: el log del anfitrión ya mide
   el bpp real por bloque de 5 s.
2. Registrar por sesión: `captura_ms/escala_ms/encode_ms`, escala aplicada,
   `kbps`, `KB/frame`, `bpp`, `rtt` mínimo y exceso, y si el frame fue IDR →
   **hecho salvo el flag de IDR por frame** (medición M2 pendiente).
3. Confirmar/descartar H1–H4 → **H1 y H2 confirmadas; H3 y H4 descartadas
   con datos**.
4. Gate: informe `evidence/video-quality-regression.md` con la reproducción y
   la causa raíz sustentada (no una hipótesis) → **✅ cerrado**.

**Causa raíz (Fase A).** El control adaptativo gobierna dos magnitudes
(**bitrate** y **escala**) que no son la magnitud percibida (**bpp del
contenido ya codificado**), y ninguna de las dos recodifica el contenido
estático:

| # | Defecto | Dónde | Dato de campo |
|---|---|---|---|
| **D1** | La escalera **no tiene suelo de bpp**: tasa inicial `w*h*3` = 0,200 bpp; tras un paso de escala `size_bps` = 1 bit/px; suelo 200 kbps = 0,003–0,006 bpp | `core/daemon/src/video.rs:302`, `:324-327`, `:311` | S2: 1001×563 @ 200 kbps → 0,003–0,096 bpp (`ui.log:5168`) |
| **D2** | La **recuperación es inalcanzable** con jitter: la zona neutra (100–200 ms) sólo degrada `net_low`; se exigen 5 muestras sanas + cooldown 20 s | `core/daemon/src/video.rs:426`, `:467`, `:268` | S2: **2 min 35 s** sin una sola decisión tras el suelo (09:54:32 → 09:57:07) |
| **D3** | Al **subir** el bitrate **no se pide IDR**: el texto estático conserva la calidad del último IDR (lo que cambia sí sigue la tasa → «movimiento fluido con la imagen degradada») | `core/codec/src/lib.rs:65-76` (sin método de refresco), `core/codec/src/mf_backend.rs:1090-1127` (sólo Mean/MaxBitRate), GOP de 30 entradas `mf_backend.rs:285-287` | S1: bpp de bloque 0,030 y 0,026 con 15 Mbps de objetivo (`ui.log:4713`, `:5052`) |

**Fase B — corrección mínima.**
- Acoplar **resolución ↔ presupuesto**: elegir escala para sostener un mínimo
  de bpp para texto (p. ej. ≥ 0,45–0,5) en lugar de mantener 1080p con 1 Mbps.
- Al **subir** bitrate o escala tras congestión, forzar **IDR/refresco**
  (el frame estático no debe quedarse con la calidad vieja).
- Revisar el arranque: no fijar la calidad visual con los primeros frames de
  calentamiento (265 ms).
- Revisar la **recuperación con jitter**: la zona neutra no debe impedir
  recuperar (D2).
- Gate: `static_text_refreshes_after_recovery`, `ladder_preserves_bpp` +
  medición antes/después a 1080p y 720p.

**Fase B — gates (falsables, con comando).**

| Gate | Qué exige | Comando |
|---|---|---|
| `ladder_preserves_bpp` | Todo escalón de la escalera sostiene ≥0,45 bpp para el tamaño activo; el suelo no baja de ese bpp | `cd core && cargo test -p anyremote-daemon --lib ladder_preserves_bpp` |
| `static_text_refreshes_after_recovery` | Tras una recuperación, el encoder recibe petición de refresco y el contenido estático se recodifica a la tasa nueva | `cd core && cargo test -p anyremote-daemon --lib static_text_refreshes_after_recovery` |
| `recovery_reachable_under_jitter` | Una serie con jitter en la zona neutra recupera en ≤ N muestras | `cd core && cargo test -p anyremote-daemon --lib recovery_reachable_under_jitter` |
| Regresión C-27/C-29/C-32/C-35/C-36 | Los tests de esos ciclos siguen verdes | `cd core && cargo test --workspace --no-fail-fast` |
| Clippy + fmt | 0 avisos | `cd core && cargo clippy --workspace --all-targets -- -D warnings` · `cargo fmt --all -- --check` |
| Binario verificado (D-13c) | `VERSION` = `app/pubspec.yaml` = `core/daemon/Cargo.toml` y build | `powershell -ExecutionPolicy Bypass -File scriptsuild-release.ps1` |

**Fase C — campo.** Julie/Brian con el VPS estable; anotar bpp, resolución y
legibilidad real. Cierre con evidencia y tag. Guion de campo de 2 min en
`evidence/video-quality-regression.md` §7.2.

**No tocar** la política de seguridad (C-44/C-45) en este ciclo. Riesgo de
regresión: el adaptativo comparte estado con C-27/C-29/C-32; los tests de esos
ciclos deben seguir verdes.

### 13.2 C-53 · Aceptar una sesión = poder controlar (UX)

**Decisión de producto.** La casilla «Permitir control remoto» del diálogo
sobra: aceptar una sesión remota implica control (el usuario lo pidió así). El
modo «solo ver» sigue disponible como acción explícita (toggle del anfitrión /
futuro botón «Compartir solo pantalla»), no como opción en el diálogo.

**Fase A.** Quitar la casilla; al aceptar (una vez o confiar) enviar
`{"perm":{"input":true}}` antes de la decisión, y sincronizar el toggle.
**Fase B.** Ajustar mensajes y el test de widget; el chip «solo ver» queda solo
si el anfitrión apaga el toggle después.
**Gate.** `accept_grants_control` (widget: tras aceptar, el toggle está on y la
decisión va precedida del permiso), regresión de C-45 (compartir sigue sin
conceder control por sí solo).

### 13.3 C-54 · Caps Lock no conmuta en el anfitrión (RF-02a)

**Fase A — reproducir.** Comprobar el wire: la app envía la tecla de bloqueo
(código 23) con `down/up`; verificar el mapeo en `key_from_code`, el envío del
keyup y el `SendInput` de `VK_CAPITAL` en el backend. Revisar si el visor
consume la tecla (atajos/foco) o si el estado se pierde entre eventos
(texto libre vs. combinación).
**Fase B — corregir.** Asegurar down+up de `VK_CAPITAL`, no mezclarlo con
`Shift`, y (si aplica) sincronizar el indicador del visor con el estado real
del anfitrión. Test del backend con el inyector `Recording` y prueba manual.
**Gate.** `capslock_toggles_host` (unit/e2e de mapeo + inyección) y prueba de
campo: escribir mayúsculas sostenidas y alternas.

### 13.4 C-55 · Teclado independiente de la distribución (solución global)

**Problema.** Hoy el anfitrión interpreta VK/scancodes según **su** layout
(`VkKeyScanW`), así que un cliente con otra distribución produce caracteres
equivocados; además Caps/Shift/dead keys dependen del estado del anfitrión.

**Fase A — investigación (sin código de producción).**
1. Cómo lo resuelven los programas base (AnyDesk y RustDesk): modos de teclado,
   envío de texto vs. scancodes, tratamiento de atajos, Caps Lock y layouts.
   Fuentes: documentación pública, comportamiento observable y RE permitida
   (patrones H-RD-06; **clean-room D-01**, sin copiar código AGPL).
2. Inventario de nuestro wire: `Key{code, down, extended, ch}` y qué necesita
   (p. ej. `text` para caracteres compuestos, `scancode` para atajos).
3. Dictamen de opciones con coste/riesgo:
   - **A. Carácter-primero**: para texto imprimible, inyectar por Unicode
     (`KEYEVENTF_UNICODE`) usando el carácter del cliente; atajos y teclas
     físicas por scancode. Ventaja: layout-independiente; riesgo: apps que solo
     reaccionan a VK, `Ctrl+letra`, juegos.
   - **B. Layout negociado**: enviar el layout del cliente y mapear a VK del
     anfitrión (requiere conocer ambos layouts y manejar diferencias).
   - **C. Híbrido**: carácter-primero con lista de excepciones (atajos) y
     fallback a scancode.
4. Gate: documento `evidence/keyboard-layout-research.md` con la comparativa y
   la decisión recomendada.

**Fase B — diseño.** Wire (campo nuevo o semántica de `ch`), estado de
modificadores (Caps/Shift/AltGr), dead keys y composición (ya hay base en
C-37/C-38), y compatibilidad con peers antiguos (negociación por
capacidades).

**Fase C — implementación + gates.** `unicode_text_ignores_host_layout`,
`shortcuts_still_use_scancodes`, `capslock_state_consistency`,
`dead_keys_with_foreign_layout`; regresión de acentos (C-37/C-38) y de atajos
(Ctrl+C/V).

**Fase D — campo.** Cliente con layout distinto al anfitrión (p. ej. US vs
latinoamericano): texto, mayúsculas, acentos, `@`, `#`, atajos. Evidencia y
tag.

### 13.5 Regla de proceso (para no repetir el lote)

1. Un ciclo = un frente (seguridad, vídeo o UX); no se mezclan en el mismo tag.
2. Cada ciclo se valida en campo antes de abrir el siguiente que toque la misma
   capa (entrada/teclado o vídeo).
3. Toda regresión de una capacidad ya cerrada se abre como **ciclo propio** con
   la evidencia del campo (como este §13) y referencias a los ciclos que la
   resolvieron (C-32/C-33/C-35/C-36 para vídeo; C-37/C-38 para teclado).
## 14. Plan de reorganización del desarrollo (2026-11-21)

**Disparador.** El usuario reporta que «se generan bugs constantemente o vuelven
a aparecer otros ya resueltos». El escaneo del repo, de RustDesk y de la
evidencia previa de AnyDesk (detalle en `evidence/plan-2026-11-21.md`) muestra
que el problema **no es de disciplina sino de verificación**: el proyecto
verifica *unidades* y rompe *fronteras*.

### 14.1 Diagnóstico en cifras

| Métrica | Valor | Lectura |
|---|---|---|
| `fix` vs `feat` (120 commits) | 23 vs 22 | Un fix por cada feature: el ritmo de regresión es estructural |
| `fix` que tocan `app/` | 4 de 6 | La capa Flutter es la más frágil |
| `core/daemon/src/lib.rs` | 3.642 líneas, 1 archivo | Config + CLI + admisión + handshake + host + controller + vídeo + audio + archivos |
| `app/lib/main.dart` | 2.065 líneas, 1 archivo | UI + parser del protocolo + procesos |
| Contrato app↔daemon | 19 eventos, 36 `case`, **1 `default:` silencioso**, sin esquema | Un rename rompe en producción sin error |
| Cobertura | huecos: vídeo adaptativo e2e, wire→backend de teclado, Caps Lock, cuotas de archivos, decodificador, preview | Lo más tocado es lo menos probado |
| CI | fmt + clippy + test (Windows) | No valida binario, ni versión, ni campo |

### 14.2 Decisiones

- **D-12** — Un ciclo = un frente; el estado de una capacidad es del anfitrión y
  sobrevive al ciclo de sesión.
- **D-13** — Definition of Done por ciclo (test + e2e si aplica + **binario
  reconstruido y verificado** + evidencia + specs + bóveda + tag).
- **D-14** — Gate de campo: el ciclo N+1 no empieza hasta que el N está validado
  por el usuario.
- **D-15** — El contrato app↔daemon se versiona y se valida en CI.
- **D-16** — Agentes por rol en `.opencode/agents/`, con `anyremote-orchestrator`
  como dueño del ciclo. Modelos: `minimax-m3.1-flash` (orquestación, spec,
  test, cleanroom, release) y `deepseek-v4.1-flash` (análisis de campo,
  implementación Rust/Flutter, seguridad).

### 14.3 Fases

| Fase | Contenido | Puerta de salida |
|---|---|---|
| **F0 Andamiaje** | 9 agentes en `.opencode/agents/` + `opencode.jsonc`; `AGENTS.md` con D-12..D-16 y toolchain corregido; este §14; `evidence/plan-2026-11-21.md`; bóveda | Revisión del usuario de los ficheros de agente |
| **F1 Campo-1 · vídeo** | **C-52**: A (medir/causa raíz) ✅ 2026-11-21 → B (política bits/píxel + refresco + recuperación con jitter) → C (campo) | Gate de campo (D-14) |
| **F2 Campo-2 · UX y Caps** | **C-53** (aceptar = control) y **C-54** (Caps Lock) | Gate de campo |
| **F3 Seguridad** | **C-46** → **C-47/C-48** → **C-49/C-50/C-51** | Gates de cada ciclo |
| **F4 Teclado** | **C-55**: investigación clean-room → diseño → implementación → campo con dos layouts | Gate de campo |
| **F5 Contrato** | **D-15**: esquema versionado de eventos/stdio, parser único, test de compatibilidad, CI lo valida | Sin rupturas app↔daemon |
| **F6 Estructura** | Partir `daemon/lib.rs` y `main.dart` en módulos por responsabilidad, **sin cambio de comportamiento** | GATES verdes + paridad de comportamiento |
| **F7 Cierre** | Release **v0.15.0** (con OK del usuario) + retrospectiva y limpieza de backlog | Release publicado |

**Reglas transversales**: un solo frente activo; `docs` no cuenta como feature;
todo `fix` de campo abre ciclo con evidencia; nada se cierra sin binario
verificado; el push y el release los hace la persona.

### 14.4 Responsables por fase

| Fase | Orquestador | Agentes participantes |
|---|---|---|
| F0 | — | `anyremote-spec` (specs), `anyremote-release` (build) |
| F1 | `anyremote-orchestrator` | `anyremote-field-analyst` (semilla), `anyremote-spec`, `anyremote-rust`, `anyremote-test` |
| F2 | `anyremote-orchestrator` | `anyremote-spec`, `anyremote-flutter`, `anyremote-rust`, `anyremote-test` |
| F3 | `anyremote-orchestrator` | `anyremote-security` (revisión), `anyremote-rust`, `anyremote-test` |
| F4 | `anyremote-orchestrator` | `anyremote-cleanroom` (investigación), `anyremote-spec`, `anyremote-rust`, `anyremote-flutter`, `anyremote-test` |
| F5 | `anyremote-orchestrator` | `anyremote-spec`, `anyremote-rust`, `anyremote-flutter`, `anyremote-test` |
| F6 | `anyremote-orchestrator` | `anyremote-rust`, `anyremote-flutter`, `anyremote-test` |
| F7 | `anyremote-orchestrator` | `anyremote-release` (build/tag) |

### 14.5 Huella de referencia (clean-room, D-01)

De **RustDesk** (AGPL, `C:\proyectos\fixed\rustdesk`) se toman **patrones**:
teclado en 3 modos (`legacy`/`map`/`translate`) con `KeyEvent.union` que incluye
`seq` (texto por caracteres) y `modifiers` independiente de la tecla; QoS con
**dos palancas** (bitrate-ratio **y** fps) y sonda de delay; refresco de
pantalla estática; `approve-mode`/`access-mode` para el diálogo; `scope` por
tipo de sesión; `MAX_FILES` y digest de reanudación.

De **AnyDesk** solo hay **política observable** (el binario está empaquetado con
Themida: 27 strings, IAT vacía) y comportamiento documentado (aprobar
conexión, contraseña temporal/permanente, modo view-only).

**No se copia código**: solo se referencian rutas lógicas de símbolo, como ya
exige D-01.
