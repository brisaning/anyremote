# Gate C-15 — transporte UDP/QUIC (v0.10.0)

**Fecha**: 2026-11-01 · **Rama**: `feature/quic-transport` · **Diseño**: `specs/02-plan-implementacion.md` §8

## Qué se implementó (resumen del diseño §8)

| Pieza | Implementación |
|---|---|
| Flag | `--transport tcp\|udp` en `listen` y `connect` (default `tcp` = histórico sin cambios). `Transport::{Tcp,Udp}` en `DaemonConfig`. |
| listen `udp` | ata **TCP y UDP en el mismo puerto** (bucle `select!` con ambos aceptadores) → peers viejos siguen entrando por TCP. |
| connect `udp` | intenta QUIC (timeout 3 s) → si el par no lo habla: evento **`quic_fallback {target,reason}`** y sesión por TCP. |
| Canal | `Channel<T>` (framing ARSP, C1) reusado **tal cual** sobre `quic::StreamPair{send,recv}` (AsyncRead+AsyncWrite sobre los streams de quinn). |
| Streams | el cliente abre **2 bi-streams** con **prologo de 1 byte** (`STREAM_PROLOGUE`): #1 control (todo el ARSP) y #2 vídeo. Orden determinista (IDs crecientes). |
| Vídeo | en QUIC corre por una **tarea propia** con `mpsc(2)` drop-new (`frames_dropped` en el informe de sesión): el backpressure del stream nunca bloquea el bucle de sesión (control/audio/heartbeats libres). TCP queda idéntico. |
| Binding C2 | `Connection::export_keying_material(buf, "anyremote-auth-v1", &[])` — **misma etiqueta que TCP**: la prueba ed25519 del peer se valida contra el binding local. Test `binding_is_identical_on_both_ends`. |
| ALPN | `anyremote-v1` (ambos lados). TLS 1.3 obligatorio de QUIC; sin 0-RTT. |
| Seguridad | TLS QUIC = acuerdo de claves; autenticación = **C2** (un intermediario que termine TLS a cada lado produce bindings distintos → rechazado). Cliente con pin de `cert_store` → verifica contra raíz (rechazo temprano) y compara el cert del peer (aviso «posible intermediario» igual que el bootstrap TCP); sin pin → verificador accept-any **y** tras C2 OK persiste el cert del servidor (**TOFU**, como el bootstrap de TCP). |
| Transport config | idle 30 s · ventana de stream 16 MiB (= `MAX_FRAME_BYTES`) · ventana de conexión 32 MiB · 8 bi-streams. Congestión: controlador por defecto de quinn. |
| App | checkbox «UDP (QUIC)» (fila de calidad; deshabilitado con listener/sesión activos) → `--transport udp` en listen **y** connect; hint de `netsh` con regla UDP cuando está activo. |
| Eventos | `listening`/`connecting` llevan `transport`; nuevo `quic_fallback`. Auditoría/permisos/heartbeats/NetHint sin cambios (van por el stream de control). |

**Sin datagrams en v1** (decisión documentada en §8): un datagrama perdido = frame
H.264 roto hasta el próximo IDR sin mini-ARQ → futuro (el mpsc drop-new ya limita
la antigüedad a ~2 frames).

## Descubrimientos de implementación (por qué el código es así)

1. **Prologo por stream**: `open_bi()` reserva el ID localmente; el peer solo
   ve el stream al viajar su primer STREAM frame — sin bytes, el `accept_bi()`
   del servidor **nunca desbloquea** aunque el stream exista (y `--video off`
   nunca escribe el de vídeo). Por eso cada stream lleva 1 byte `A` tras abrir
   (además: sanidad de protocolo).
2. **El Endpoint no puede caer**: dropear el `Endpoint` de quinn cierra sus
   conexiones (verificado empíricamente en los tests del transporte) →
   `quic_attempt` devuelve el Endpoint y el caller lo mantiene vivo durante
   la sesión (`_quic_ep`).
3. **Handshake concorrente**: `connect().await` no completa hasta que el
   servidor hace `incoming.await` — patrón `tokio::join!` de los propios tests
   de quinn (en producción el bucle del listener acepta en `select!` mientras
   el cliente conecta; timeout de establecimiento 15 s).
4. **`SendStream::drop` hace `finish()`** (EOF ordenado, no reset) → el cierre
   normal de sesión se ve como `Eof` en el par (mapeado a `connection_gone`).
5. **`conn_gone` ampliado** con `lost`/`timed` (mensajes de quinn) para que un
   enlace QUIC muerto cierre limpio (`connection_gone`), no como `session_error`.

## Tests

**Transporte (`anyremote-transport`, unit)** — 4 nuevos:
- `streampair_reuses_arsp_channel_over_quic` — `Channel<StreamPair>` intercambia
  frames ARSP cifrados por QUIC (reuso del framing C1 sin tocar `Channel`).
- `binding_is_identical_on_both_ends` — ambos extremos exportan el MISMO binding.
- `pinned_handshake_works_and_peer_cert_is_available` — con pin TOFU el
  handshake completa y el cert del servidor queda disponible.
- `two_streams_open_independently` — dos streams con prologo, orden de apertura
  determinista.

**Daemon e2e (`daemon_quic_e2e.rs`)** — 3 nuevos:
- `quic_session_carries_video_and_control` — sesión QUIC completa: auth C2 con
  binding QUIC, permisos, **vídeo de prueba por el stream dedicado**
  (`video_stats` + preview BGRA 320×200 validado), heartbeats/NetHint por el
  stream de control, `transport:"udp"` en eventos y **sin** `quic_fallback`.
- `udp_client_falls_back_to_tcp_when_peer_is_tcp_only` — cliente UDP contra
  listener solo-TCP: `quic_fallback` emitido y sesión (con vídeo) por TCP.
- `tcp_client_connects_to_dual_listener` — cliente TCP (peer viejo) entra al
  listener `udp` por su socket TCP.
- `free_port` del binario endurecido (rango no efímero + offset por test):
  bajo carga de suite completa el efímero liberado lo reciclaban otros tests
  entre probe y bind → «connection refused» intermitente (corregido y
  verificado con 3 corridas completas consecutivas estables).

## Gates (2026-11-01, máquina con **pantalla bloqueada**)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` (workspace) | **47 suites ok / 2 fallos AMBIENTALES** ×3 corridas idénticas: `input_toggles_at_runtime` (**SendInput 0/1** — el escritorio bloqueado bloquea la inyección; **falla IGUAL en develop limpio** verificado con `git stash`) y `windows_clipboard_read_probe` (**OpenClipboard 0x80070005** — portapapeles tomado por la sesión bloqueada; pasó al reintentar). Evidencia del entorno: primer plano = `LockApp` («Pantalla de bloqueo predeterminada de Windows») + `LogonUI` activo. |
| Suites QUIC | **estables**: 3× suite completa sin fallos QUIC (tras el fix de puertos) + 3× binario aislado 3/3. |
| `cargo clippy --workspace --all-targets` | **0 avisos / 0 errores** |
| `cargo fmt --check` | **0** |
| `flutter analyze` | limpio |
| `flutter test` | **9/9** (incluye el nuevo assert: sin toggle → sin `--transport`; con toggle → `--transport udp`) |

**Cobertura no verificada en esta máquina** (por pantalla bloqueada): los 2
tests de escritorio de arriba — el CI (Windows runner con escritorio
interactivo) los cubre como en los ciclos anteriores.

## Pendiente de validación de campo

- Sesión real con el toggle «UDP (QUIC)» en ambos equipos (enlace degradado:
  el vídeo debe degradarse sin matar la sesión — el caso que motivó C-15).
- Ver en el log del anfitrión `listening … "transport":"udp"` y en el visor la
  ausencia de `quic_fallback` cuando ambos van por QUIC.


---

## v0.10.1 — `transport` en la auditoría de sesión (soporte de validación de campo C-15)

**Motivo**: la primera prueba de campo (2026-11-03, 10:03) corrió con el toggle
UDP activo SOLO en el anfitrión → el controlador conectó **sin** `--transport udp`
(línea `connect args: ... --clipboard win --audio on` en su `ui.log`) y la sesión
fue **TCP contra el listener dual**. Eso validó en la vida real el escenario de
compatibilidad (peer sin el flag entra al listener `udp` por su socket TCP ✓),
pero desde el anfitrión no había forma inequívoca de saber qué transporte usó
la sesión de verdad.

**Cambio**: `session_established` de la auditoría (AMBOS roles) incluye ahora
`"transport": "tcp" | "quic"` — el transporte **real** de la sesión, no el
configurado (un listener `udp` acepta también por TCP; el brazo de aceptación
que completó el handshake es el que lo declara).

**Tests** (`daemon_quic_e2e`, aserciones nuevas sobre la auditoría):
- sesión QUIC → **ambos** roles registran `"transport":"quic"`;
- cliente UDP contra listener solo-TCP (fallback) → `"transport":"tcp"` (no quic);
- cliente TCP contra listener dual → `"transport":"tcp"`.

**Gates (2026-11-03)**: Rust **49 suites / 0 fallos** (los2 tests de escritorio
también pasaron: la pantalla estaba desbloqueada en esta corrida) · **clippy 0/0**
· **fmt 0** · `flutter analyze` limpio · **9/9**.

**Sesión de campo 2026-11-03 10:03 (v0.10.0, host `ONDH6IU4RU`)** — valida
compatibilidad + salud de sesión: `tcp+udp (QUIC)` en el listener · `mf_h264`
sin fallback · 131 frames / 1,58 MB · 48 heartbeats (rtt 44→119 ms) · C-13
actuando (`red_bitrate → 1382400`) · `frames_dropped: 0` en el informe · cierre
limpio `bye`. **Pendiente**: sesión QUIC real (toggle en la visora).
