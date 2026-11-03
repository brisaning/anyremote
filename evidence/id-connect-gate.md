# Gate C-16 — conexión por ID (resolutor LAN, v0.11.0)

**Fecha**: 2026-11-03 · **Rama**: `feature/id-resolve` · **Diseño**: `specs/02` §9

## Qué se implementó

| Pieza | Implementación |
|---|---|
| Protocolo | UDP **broadcast** puerto **21119**: controlador emite `ANYREMOTE/1 WHO <id>` (3 intentos × 900 ms) → anfitrión responde **unicast** `ANYREMOTE/1 HERE <id> <puerto>` **desde su socket** (la IP de origen de la respuesta = dirección correcta del anfitrión, multi-homing sin ambigüedad). Parse estricto: magia+versión+id validado con `NodeId::parse`+puerto; basura/versión desconocida/id ajeno → se ignora. |
| Servidor | `discover::DiscoveryServer` (bind `0.0.0.0:21119`) durante `listen`: responde **solo** a consultas por NUESTRO id, con **throttling por origen** ≤1/500 ms (patrón H-RD-02). `Drop` → aborta la tarea al terminar la escucha. Puerto ocupado/flag inválido → evento `discover_error` y la escucha sigue. |
| Caché | Tras una respuesta viva: `resolved/<ID>.txt` (`ip:port`). Si la broadcast no responde → caché; tampoco → error con el **`netsh` exacto** del puerto usado. Nunca se asume dirección (solo se escribe tras respuesta). |
| **Seguridad** | La resolución **no autentica** (cualquiera puede responder): `connect` por ID exige tras **C2** que `peer_id.node_id == ID solicitado` **antes** de recordar TOFU o guardar pin → una caché envenenada/intermediario provoca «no es el ID solicitado» sin dejar residuos. El **pin por destino usa el ID** (`trusted/<ID>.pem`): estable entre cambios de DHCP. |
| Flag | `--discover <ip:puerto|puerto>` en `listen` y `connect` (default `255.255.255.255:21119`). Evento `resolve_ok {id, addr, source: broadcast\|cache}`. |
| App | eliminado el bloqueo «por ID aún no se puede»; `looksLikeAlias` («sin `:`») ya no se traga los IDs → el ID viaja en mayúsculas como target normal; hint `netsh` para UDP 21119. |

## Tests

**Transporte (`discover`, 5 unit)**: roundtrip query/reply · rechazo de
basura/versión desconocida/id ajeno/puerto 0 o >u16 · `query` resuelve un
`DiscoveryServer` en loopback → `127.0.0.1:21118` · `query` timeout con id
ajeno · caché save/load + archivo corrupto → `None`.

**Daemon e2e (`daemon_resolve_e2e.rs`, 3)**:
1. `resolves_id_by_discovery_and_sessions` — listener con `--discover
   127.0.0.1:P` + controlador conectando **por ID** → `resolve_ok` con
   `source:"broadcast"` + `session_active` (sesión completa por ID).
2. `unresolved_id_fails_with_netsh_hint` — sin anfitrión ni caché → error
   «no se pudo resolver» **con el `netsh` del puerto** (`localport=<P>`).
3. `cache_redirect_to_wrong_machine_is_rejected` — caché apuntando a una
   máquina con OTRO id → rechazo «no es el ID solicitado» **y sin guardar**
   `trusted/<ID>.pem` (la verificación va antes de TOFU/pin).

## Gates (2026-11-03)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **50 suites / 0 fallos** (incluye los2 de escritorio: pantalla desbloqueada) |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` | limpio |
| `flutter test` | **9/9** |

## Pendiente de validación de campo

- Conectar desde la app tecleando solo el **ID del anfitrión** (sin IP) con el
  firewall UDP 21119 abierto en el anfitrión (hint en la app) → sesión normal
  + `resolve_ok` en el registro del visor.


---

## Intento de campo 1 (2026-11-03 13:01) — falló por broadcast · fix **v0.11.1**

**Síntoma**: la visora pasó el ID correctamente (`connect args: connect ONDH6IU4RU …
--transport udp`, daemon 0.11.0) pero las4 tentativas de `resolve_id` terminaron
con «sin respuesta de descubrimiento y sin caché». El anfitrión estaba bien:
`anyremoted 0.11.0` escuchando (ambos modos), **sin** `discover_error` (la app
registra eventos desconocidos → el `DiscoveryServer` había prendido).

**Diagnóstico por sondas (desde la visora)**:

| Sonda | Resultado |
|---|---|
| WHO unicast → `192.168.6.152:21119` | **`ANYREMOTE/1 HERE ONDH6IU4RU 21118` ✓** (el servidor respondía) |
| WHO broadcast de subred → `192.168.6.255:21119` | **Respuesta ✓** |
| WHO broadcast limitada → `255.255.255.255:21119` | **Nunca llega ✗** (la query actual usaba ÉSTA) |

**Causa raíz**: la visora tiene **varios adaptadores** (múltiples puertas de
enlace en `ipconfig`); la broadcast limitada `255.255.255.255` sale por la
interfaz equivocada y no alcanza la LAN, mientras la broadcast **de subred**
(`x.x.x.255`) sigue la ruta de la interfaz del LAN y sí llega.

**Fix (v0.11.1)**: `discover::query_targets()` **abanica** la consulta a:
el destino configurado + **la broadcast de cada subred local IPv4** (ip |
!netmask, sin loopback/link-local, sin duplicados) — un envío por intento a
TODOS los destinos (mejor esfuerzo: una interfaz rota no mata la consulta) y
una sola espera de respuesta válida. Enumeración con `if-addrs` (nueva dep
menor del transporte). Test nuevo `query_targets_fans_out_without_duplicates`.

**Gates v0.11.1**: Rust **50/0** · clippy **0/0** · fmt **0** · Flutter
analyze limpio · **9/9**.

**Nota**: el firewall NO era el problema (el unicast respondía sin regla
adicional); el hint `netsh` 21119 se mantiene por si acaso.


---

## Intento de campo 2 (2026-11-03 13:41, v0.11.1) — fallo instantáneo · fix **v0.11.2**

**Síntoma**: con el fan-out de v0.11.1 el error llegaba en **~30 ms** (la
query debería tardar ≥2,7 s) — señal de fallo **instantáneo** oculto por el
mensaje fijo («sin respuesta…» no incluía la causa real de la query).

**Diagnóstico**:
1. `ipconfig`/`Get-NetIPAddress` de la visora: `172.16.0.2/32 **CloudflareWARP**`.
2. Para un /32, `subnet_broadcast` calculaba `ip | !mask` = **la propia IP** →
   un **unicast sin listener** en la propia máquina.
3. En Windows, enviar UDP a un unicast cerrado genera ICMP y el **siguiente
   `recv_from` del mismo socket devuelve 10054 al instante** (reproducido con
   una sonda: `ConnectionResetError [WinError 10054]`).
4. El código trataba `Ok(Err(e))` como fatal → la query abortaba en 30 ms
   **sin esperar jamás la respuesta buena** (la que mi sonda manual sí
   recibía: `HERE ONDH6IU4RU 21118` por unicast y por `192.168.6.255`).

**Fix (v0.11.2, dos capas + diagnóstico)**:
1. **`subnet_broadcast(ip, netmask)`**: `None` para **/31 y /32** (no tienen
   dirección de broadcast real) y si la calculada coincide con la propia IP —
   el pozo de ICMP ni siquiera se crea. Test `subnet_broadcast_skips_hosts_and_p2p`
   (incluye el caso WARP `172.16.0.2/32` → `None`).
2. **`query_with_targets`**: los errores de recepción **no abortan** — el
   reset se consume y se sigue esperando hasta el **deadline del intento**
   (900 ms); un pozo de ICMP de cualquier origen solo cuesta lo que reste del
   intento. Test regresión `recv_errors_do_not_abort_the_query` (poison
   `127.0.0.2:59998` → espera ≥800 ms en vez de abortar).
3. **Mensaje con la causa real**: `resolve_id` incluye el error de la query
   (`{detail}`) — el fallo de campo ya no puede quedar oculto.

**Gates v0.11.2**: Rust **50/0** · clippy **0/0** · fmt **0** · Flutter
analyze limpio · **9/9** (descubrimiento:8 unit).
