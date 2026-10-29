# Gate C-10 — modo visor (cliente) · v0.8.4

| Campo | Valor |
|---|---|
| Fecha | 2026-10-29 |
| Rama | `feature/visor-mode` (D-11) · tag `visor-mode-v0.8.4` |
| Disparador | Ajuste de UX del usuario (con visor remoto visible) |
| Gates | Rust **44 suites / 0 fallos** · **clippy 0 avisos / 0 errores** · **fmt 0 diffs** · `flutter analyze` limpio · `flutter test` **8/8** |

## Petición

> «En el cliente, al mostrar el visor remoto se deben ocultar la sección de
> Conexión y Registro, y el botón desconectar debe estar en la parte superior
> a la izquierda del estado "Conectado".»

**Decisión de diseño (preguntada y aprobada por el usuario)**: los tres
toggles dinámicos (Control remoto · Audio del sistema · Micrófono) viven
dentro de la sección Conexión; ocultarla entera los dejaría inaccesibles
durante la sesión (perdiendo la función caliente C-2 pedida en v0.7.0) →
**se mudan a la cabecera del visor** en modo visor.

## Qué se entregó (v0.8.4)

1. **Modo visor** (`viewerVisible = _lastImage != null || _preview != null`):
   - la sección **Conexión** queda **oculta**;
   - la sección **Registro** queda **oculta** (el visor pasa a ocupar todo el
     cuerpo, con su `Expanded(flex:3)` de C-9);
   - el pie `build …` se conserva (no es ninguna de las dos secciones).
2. **`Detener` en la barra superior**: dentro de `AppBar.actions`, **justo a
   la izquierda** del indicador de estado (avatar + texto) — solo en modo
   visor; fuera del modo visor sigue dentro de Conexión (sin duplicados).
3. **Toggles en la cabecera del visor**: helper `_permToggles()` con **una
   sola copia** del markup (la sección Conexión reutiliza la misma fila) —
   siguen aplicándose **en caliente** durante la sesión.
4. **Las secciones vuelven al cerrar** (para poder reconectar sin reiniciar):
   - `case 'closed'` → `unawaited(_stopViewer())` (limpia el último frame);
   - el preview `onDone`/`onError` → también limpia (evita el «último frame
     congelado» manteniendo el layout de modo visor).
   - Sin esto, el `else` de `_lastImage` dejaría el visor fantasma tras
     desconectar.

## Tests — `C-10: modo visor oculta Conexión/Registro y sube Detener a la barra`

Verifica, en una sola pasada:
1. Sin visor → layout normal: Conexión + Registro + **un** `Detener` en la
   tarjeta + 3 checkboxes;
2. Con frame inyectado → `Conexión` **ausente**, `Registro` **ausente**;
3. **Un único** `Detener` y por **posición**: `btn.top < visor.top` (barra
   superior) y `btn.right ≤ estado.left` (**a la izquierda del estado**);
4. 3 toggles en la cabecera del visor (labels completos);
5. Tras `debugStopViewerForTest()` → Conexión + Registro + Detener + toggles
   **vuelven**.

Ajuste del test C-9: en modo visor ahora espera `Registro` **ausente**
(comportamiento C-10), manteniendo su aserción de «sin desbordes» a 800×600
y 1600×900.

## Regresión

Rust **44/0** (sin cambios Rust en este ciclo) · clippy **0/0** · fmt **0** ·
Flutter **analyze limpio + 8/8** (C-8/C-9 y los e2e del listener siguen
verdes).

## Limitaciones

- El **ID de este equipo** vive en la sección Conexión → también queda
  oculto durante la sesión (se recupera al cerrar; el ID del anfitrión sigue
  visible en «SESIÓN ACTIVA con …» en el registro — ahora oculto en modo
  visor, pero ya mostrado al inicio de la sesión).
- Sesiones **sin vídeo** (viewer nunca visible) mantienen el layout actual
  con la sección Conexión (y su `Detener`/toggles) — pedido solo «al mostrar
  el visor remoto».
- Carrera cosmética residual: un frame en vuelo podría llegar tras `closed`
  y reensayar el modo visor unos ms hasta que el preview cierre (ventana de
  milisegundos, sin efecto funcional).
