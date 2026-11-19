# Gate C-42 — `fps render`: medir y decidir `Texture` (v0.13.14)

**Fecha**: 2026-11-19 · Rama: `feature/c42-render-decision` · Origen: pendiente
abierto en C-19 («`Texture` nativo condicionado a la medición de campo»).

## Qué se ha hecho

1. **Instrumentación del visor** (queda en producción): cada 5 s el log del
   cliente añade

   ```
   · recv=N render=M drop=D coste=X.Xms
   ```

   - `recv`: frames que entrega el ensamblador (llegaron).
   - `render`: frames convertidos a `ui.Image` y pintados.
   - `drop`: descartados por el guard de reentrada (el render anterior seguía en
     curso).
   - `coste`: EMA del tiempo de `_renderFrame` (buffer inmutable → descriptor →
     codec → frame).

2. **Banco en release** (`--dart-define=VIEWER_BENCH`, retirado tras medir):
   30 frames BGRA de **1920×1080** por el mismo `_renderFrame`, cronometrados.

## Medición

| Entorno | Coste por frame 1080p |
|---|---|
| `flutter test` (JIT, renderer de software) | 70,8 ms — **cota superior**, no representativa |
| **Release Windows (AOT)** | **11,0 ms** (EMA 9,6 ms; 30 frames) |

Presupuesto a 15 fps: **66 ms**. El render usa **~17 %** del hueco (6× de margen);
incluso a 30 fps (33 ms) cabe sin descartes. `drop = 0` en la prueba.

Además, los logs de campo ya mostraban que el visor **iba al ritmo de lo que
llegaba**: el anfitrión emitía 2–5,6 fps en pantalla estática y hasta ~8 fps en
movimiento, y `fps render` seguía esos valores.

## Decisión

**No se implementa `Texture` nativo.** El pipeline `ui.Image` (BGRA →
`ImmutableBuffer` → `ImageDescriptor.raw` → `RawImage`) sostiene 1080p a 15 fps
con 6× de margen y sin descartes; el cuello era el **origen** (DXGI entrega solo
cambios + tope de 15 fps), no el visor. Un plugin nativo (D3D11/ANGLE + registro
de texturas) añadiría superficie y riesgo sin un problema medido que resolver.

Los contadores quedan en el log para **confirmar en Julie** (hardware distinto):
si `coste` superara ~30 ms o `drop` creciera con el vídeo en movimiento, se
reabre la decisión con esos datos.

## Cierre de Juank/Juank2

Pruebas reportadas por el usuario el 18–19/11: sesiones **fluidas** desde
NRRNYXSKLG y RY4GHIHKIU. No hay incidencias que registrar; el caso queda
**cerrado** (sin logs de esos equipos, la validación es la del usuario).

## Gates

| Gate | Resultado |
|---|---|
| `cargo test --workspace` | **159 / 0** |
| `cargo clippy --workspace --all-targets` | **0 avisos** |
| `cargo fmt --all -- --check` | **0 diffs** |
| `flutter analyze` / `flutter test` | limpio · **31 / 31** (nuevo test C-42: 10 frames 1080p con cadencia de 15 fps, `drop ≤ 1`) |
