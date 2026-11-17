# Gate C-30 — conectividad: reintento de relay tras fallo directo (v0.13.3)

**Fecha**: 2026-11-17 · Rama: `fix/relay-rdv-retry` · Disparador: campo (Brian ↔ Julie):
Brian→Julie conecta; **Julie→Brian no** (4 intentos, 14:22–14:25).

## Diagnóstico (logs del 17/11)

```
ID resuelto → LAN (broadcast): 192.168.6.230:21118
quic_fallback
error: Io("conexion directa a 192.168.6.230:21118: timeout (8 s)")
daemon finalizado (código 1)
```

- El listener de Brian estaba arriba (`0.0.0.0:21118`) y **Brian→Julie sí conectó**
  (14:18:53) → el problema es de **entrada** en Brian, no del cliente.
- **Discovery LAN funciona** (Julie resuelve el ID por broadcast).
- **Timeout (no *connection refused*)** = SYN descartado → **firewall**: en Brian
  **no existe ninguna regla** para anyremote (`netsh advfirewall firewall show
  rule name=anyremote` → "Ninguna regla coincide"; perfiles en Privado, así que
  no es el perfil).
- **Causa estructural**: al resolver por **LAN**, `resolve_id` devuelve
  `relay: None` (el ticket sólo viene del rendezvous). Si lo directo falla, el
  `match &relay` no tiene reserva y **el daemon muere sin intentar relay**, pese
  a que el anfitrión está registrado y el relay es alcanzable (juank/juank2 →
  Brian ya usaban relay).

## Qué se implementa

| Pieza | Detalle |
|---|---|
| **C-30** | `relay_ticket_on_direct_failure()`: si lo directo falla y no había ticket (ruta LAN/caché), se **re-consulta el servidor de encuentro AHORA** y se usa su ticket de relay. Si el rdv tampoco da ticket, se propaga el error directo original (sin cambiar el mensaje). |
| **Evento** | `relay_fallback` gana `"source": "rdv"` (ticket previo) o **`"rdv_retry"`** (ticket obtenido tras el fallo) para distinguirlos en campo. |
| **Test** | `lan_resolved_direct_failure_retries_rdv_and_uses_relay`: descubrimiento LAN que anuncia un puerto **cerrado** + listener registrado en el rdv → se espera `resolve_ok source:broadcast`, `relay_fallback` con `rdv_retry` y `session_active` (sesión completa por relay). |
| **Ops** | Para que la **LAN directa** funcione (sin relay) hay que crear las reglas que la app ya imprime: `netsh advfirewall firewall add rule name="anyremote" dir=in action=allow protocol=TCP localport=21118` (+ UDP 21118 y UDP 21119). Requiere administrador. |

## Gates (2026-11-17)

| Gate | Resultado |
|---|---|
| `cargo test --no-fail-fast` | **51 suites / 0 fallos** (incluye el test nuevo) |
| `cargo clippy --workspace --all-targets` | **0 / 0** |
| `cargo fmt --check` | **0** |
| `flutter analyze` / `flutter test` | limpio · **25/25** |

## Pendiente de validación de campo

- Repetir **Julie→Brian** con el daemon v0.13.3: debe aparecer
  `relay_fallback` con `"source":"rdv_retry"` y la sesión debe completarse.
- Crear las reglas de firewall en Brian para recuperar la **LAN directa**
  (sin relay) y anotar si el timeout desaparece.
