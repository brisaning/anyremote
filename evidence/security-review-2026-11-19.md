# Revisión de seguridad de fronteras de confianza (2026-11-19)

| Campo | Valor |
|---|---|
| Fecha | 2026-11-19 |
| Commit revisado | `696762058c72ccf174f30cfcc1e5551e937e9919` (`6967620`, rama `develop`) |
| Alcance | Enfocado (decisión del usuario): `transport` (auth/binding/QUIC), `identity`, `session`, `daemon`, `input`, `filexfer`, `clipboard`, `codec` + cableado de `app/lib` |
| Fuera de alcance | `capture`, `audio`, `profiles`, `video.rs` completo, `main.dart` completo, despliegue/VPS y firewall |
| Método | Revisión estática de fuente (lectura dirigida + `grep` sobre el árbol). **Sin ejecución del objetivo** (no había sandbox con red aislada, entorno vacío y límites) |
| Resultado | 8 hallazgos: 2 altos, 3 medios, 1 bajo-medio, 1 bajo, 1 informativo |

**Naturaleza de la evidencia.** Esta nota es la *semilla* de los ciclos C-44…C-51
(`specs/02` §12): cada hallazgo queda trazado a la ruta y línea que lo sostiene.
No contiene instrucciones de explotación contra sistemas desplegados; todas las
comprobaciones propuestas son locales y acotadas.

## Comandos que produjeron la evidencia (reproducibles)

```bash
git -C . rev-parse HEAD
find core -name "*.rs" -exec wc -l {} +            # tamaño del objetivo
grep -n "peer_trusted\|is_trusted" core/daemon/src/*.rs core/transport/src/*.rs
grep -rn "showDialog\|AlertDialog" app/lib/main.dart
grep -n "listen\|allow-input\|clipboard\|perm" app/lib/main.dart
grep -n "equal\|FileOffer\|InputInjected" core/daemon/src/lib.rs
```

## Hallazgos

### F-01 · Alta · Sin autorización del par

La autenticación C2 demuestra posesión de una clave ed25519 autogenerada; no hay
PIN, contraseña, allowlist ni aprobación por conexión, y `peer_trusted` solo
informa.

- `core/transport/src/auth.rs:97-152` — validación de firma/binding/`node_id` (fin de la comprobación).
- `core/daemon/src/lib.rs:760-776` — `TrustStore::remember` acepta a cualquier par; `peer_trusted` solo viaja en el evento y el JSONL.
- `core/daemon/src/lib.rs:778-824` — el `granted` efectivo lo responde **el par**, no el anfitrión.
- `app/lib/main.dart` — no existe diálogo de aceptación (`grep` de `showDialog`/`AlertDialog` sin resultados).
- `core/daemon/src/lib.rs:1003` + `app/lib/main.dart:190` — escucha en `0.0.0.0:21118`.

**Impacto.** Cualquier equipo que alcance el puerto obtiene inyección de
teclado/ratón (`core/input/src/windows_backend.rs:123-151` ⇒ ejecución de código
como el usuario), vídeo, audio y portapapeles. El ID no es requisito para el
atacante: solo se exige cuando *tú* conectas por ID (`lib.rs:2118-2125`).

### F-02 · Alta · La app arma el control remoto por defecto

- `app/lib/main.dart:196-213` — `buildListenArgs` pasa siempre `--allow-input`, `--audio on`, `--audio-source system,mic`.
- `app/lib/main.dart:756-758` — `_startLocalListen` fuerza `_acceptRemoteControl = true` antes de `_sendPerms()`.
- `core/daemon/src/lib.rs:429` — `input_on: cfg.allow_input` (la entrada está permitida desde el inicio de sesión, sin esperar toggle).
- `README.md` §RF-02a — declara "control remoto y audio **opt-in**", en contradicción con lo anterior.

### F-03 · Media · Portapapeles sin compuerta

- `core/daemon/src/lib.rs:1721-1760` — envío del portapapeles local cada 500 ms si `cfg.clipboard != Off`.
- `core/daemon/src/lib.rs:1596-1614` — aplicación del portapapeles remoto.
- `app/lib/main.dart:196` — `--clipboard win` fijo.
- No existe permiso `clipboard` en el handshake ni en `DynamicPerms`.

### F-04 · Media · Transferencia de archivos sin compuerta y bloqueante

- `core/daemon/src/lib.rs:494-499` — `file_policy` ignora `enabled`; no hay flag de recepción.
- `core/daemon/src/lib.rs:1334-1366` y `1615-1639` — `receive_file_offer` se espera dentro del `select!` de la sesión; `ch.recv()` sin timeout.
- `core/filexfer/src/lib.rs:260-294` — `accept_offer` no aplica `max_files_per_batch`; `Partial.buf` retiene el archivo completo (hasta 256 MiB, `lib.rs:31-40`).
- `core/filexfer/src/lib.rs:215-230` — `.part`/`.meta` persistentes; sin limpieza de huérfanos.

### F-05 · Media · Pulsaciones en claro en el registro de auditoría

- `core/daemon/src/lib.rs:1677-1678` — `"input": format!("{iev:?}")` ⇒ `KeyDown(Char('x'))` por evento.
- `core/telemetry/src/lib.rs:93-113` — `flush` por evento a `audit/audit-*.jsonl`.
- `README.md` §Diagnóstico de campo — indica compartir `%APPDATA%\anyremote\`.

### F-06 · Baja-Media · Decodificador sin cotas

- `core/codec/src/lib.rs:202-219` — `expect = width*height*4` en `u32` (overflow) y `read_to_end` sin tope para `Deflate`.
- `core/transport/src/arsp.rs:169-181` — el formato del frame lo elige el par.

### F-07 · Baja · Preview local sin autenticación

- `core/daemon/src/preview.rs:17-44` — primer cliente de `127.0.0.1:<efímero>` recibe BGRA crudo; el puerto se publica en el evento `preview`.

### F-08 · Informativa · Higiene de confianza y contadores

- `core/transport/src/auth.rs:41-51` — `record_failure()` devuelve `Err` en el primer fallo: `MAX_AUTH_FAILURES` es inalcanzable; sin rate-limit por origen.
- `app/lib/main.dart:522-524` — "✓ verificado" se pinta con `peer_trusted` (cualquier par nuevo queda marcado tras la primera sesión).
- `core/input/src/windows_backend.rs:144-149` — `delta * WHEEL_DELTA` sin comprobación de desbordamiento.

## Controles verificados como correctos (no requieren acción)

| Control | Evidencia |
|---|---|
| C2 con channel binding (RFC 5705) en TCP y QUIC | `transport/src/auth.rs:63-78`, `identity/src/lib.rs:59-64,131-139`, `transport/src/quic.rs:10-14` |
| Conexión por ID verifica `peer_id == ID` **antes** de TOFU/pin | `daemon/src/lib.rs:2118-2135` |
| Pin de certificado por destino con comparación de huella | `daemon/src/lib.rs:2425-2438` |
| `TrustStore` exige `id↔pk` coherente y descarta líneas rotas | `identity/src/lib.rs:167-210` |
| Saneado de nombres sin traversal (`/ \ : \0`, `..`, controles, ocultos) | `filexfer/src/lib.rs:73-86` |
| El receptor **no escribe** el archivo recibido en disco (`finish`, no `finish_to_file`) | `daemon/src/lib.rs:1362-1365` |
| Framing con cota 16 MiB y recepción cancel-safe | `transport/src/lib.rs:293-325`, `arsp.rs:55-99` |
| Relay WAN ciego (TLS E2E + C2 encima) | `quic.rs:10-14`, `daemon/src/lib.rs:1145-1152` |
| Gate estático de secretos y verificadores TLS peligrosos | `transport/tests/security_audit.rs` |

## Limitaciones y validación pendiente

1. No se ejecutó el binario ni los tests: la reproducción de F-01/F-02/F-04 exige
   un sandbox (sin red externa, `HOME`/TMP propios, límites de CPU/RAM/tiempo).
   Plan seguro: loopback con `--accept any` y una identidad nueva, verificar que
   el listener **no** registra `Active` en el modo por defecto tras C-44.
2. F-03/F-05 se sostienen en la traza de fuente (no hay camino que las desactive)
   pero conviene un test de auditoría que falle si reaparece el texto tecleado.
3. Exposición real (firewall del anfitrión, VPS `51.222.81.81:21120`) no se
   sondeó: queda como condición de despliegue, no como hallazgo de código.
