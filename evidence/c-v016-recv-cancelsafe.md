# Addendum v0.1.6 — Causa raíz cross-device: recv no cancellation-safe

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/recv-cancelsafe` (D-11) · tag `integracion-v0.1.6` |

## Cadena causal completa (evidencia de AMBOS lados)

**Registro remoto** (`E:\anyremote\audit\…`, peer DYUDTKKXM6):
```
clipboard_sync dir=in (19 B) ✓      ← frame de portapapeles decodificado PERFECTO
+1 s  error session_error: Transport(FrameTooLarge(134967817))
      disconnected → socket cerrado → nuestro lado ve connection_gone
```
- `134967817 = 0x08_0B_E0_69` → **bytes de cuerpo protobuf** (tag 0x08 = campo varint de un Heartbeat) leídos como **longitud de frame** ⇒ desincronización de flujo previa.

**Registro local** (ses-DYUDTKKXM6): 6 heartbeats con RTT 97–117 ms y cero errores — el enlace estaba sano; murió el proceso remoto.

**Causa de código**: `Channel::recv()` (read_exact en el futuro) dentro de `tokio::select!` del daemon: al ganar otra rama (tick de portapapeles 500 ms / latido 1 s — colisión cada ~2 s ✓ coincide con las sesiones de 1–6 s), la recepción cancelada **perdía los bytes parciales leídos** → el siguiente frame se interpretaba desplazado → longitud basura → `FrameTooLarge` → salida del daemon.

## Fix
`recv()` reescrito como `poll_fn` con **buffer persistente en el canal** (`Channel.buf`): todo byte consumido por `poll_read` aterriza en el buffer ANTES de cualquier `Pending` y **sobrevive a la cancelación**. `FrameTooLarge` se conserva como defensa ante corrupción real.

## Prueba de fuego (regresión verificada en AMBOS sentidos)

`transport::recv_cancelsafe::recv_survives_constant_select_cancellation` (frames de ~3 KB con `duplex(512)` — lecturas parciales forzadas, cancelación constante por tick de 1 ms):

| Código | Resultado |
|---|---|
| Viejo (stash) | **FAILED** — `FrameTooLarge(2021161080)` ← misma clase del log remoto |
| Nuevo | **PASSED** — 10 secuencias completas, ordenadas, payloads íntegros |

## Gates
- Suite completa: **35 suites / 0 fallos** (transport 4+7 e2e + recv_cancelsafe + resto intacto).
- Daemon versionado **0.1.6** (los eventos `listening/connecting` lo viajan para verificación remota).

## Confirmación de campo (usuario)

- **2026-10-26**: retest cross-device con v0.1.6 en ambos lados → **la conexión se mantiene estable** (sin `FrameTooLarge` ni `connection_gone`). Causa raíz v0.1.6 confirmada en producción real por el usuario.
