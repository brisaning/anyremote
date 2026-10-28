# Gate C-9 — visor remoto completo + ID visible · v0.8.3

| Campo | Valor |
|---|---|
| Fecha | 2026-10-28 |
| Rama | `feature/visor-id` (D-11) · tag `visor-id-v0.8.3` |
| Disparador | Reporte de campo con v0.8.2 (2 hallazgos) |
| Gates | Rust **44 suites / 0 fallos** · **clippy 0 avisos / 0 errores** · **rustfmt 0 diffs** · `flutter analyze` limpio · `flutter test` **7/7** |

## Hallazgos reportados

1. *«Al conectar, el visor remoto muestra una parte de la pantalla y es
   necesario redimensionar para ver la pantalla completa del anfitrión; si se
   maximiza, el visor muestra solo la parte superior de la pantalla perdiendo
   la posibilidad de ver la parte inferior.»*
2. *«No encuentro en qué lugar se puede obtener el ID remoto.»*

## 1) Diagnóstico del visor (por qué se cortaba por abajo)

La tarjeta «Visor remoto» era un hijo **rígido** del `Column` (sin `Expanded`)
y su caja derivaba el **alto del ancho** disponible: `AspectRatio(w/h)` con el
ancho de la ventana → alto = ancho ÷ (16/9). Con el contenido fijo superior
(~270 px: conexión + toggles + perfiles) el conjunto total desbordaba la
ventana:

| Ventana | Caja del visor (≈ancho/1.78) | Total fijo + visor | Resultado |
|---|---|---|---|
| 1280×720 | ≈ 684 px | ≈ 1000 px | **RenderFlex overflow** → recorte por abajo |
| maximizada (ancho ↑) | ≈ 1033 px | ≈ 1330 px | **más recorte**: solo la parte superior ✓ coincide con el reporte |

## Fix (F1)

- La tarjeta pasó a **`Expanded(flex: 3)`** y el marco del frame a
  **`Expanded(child: Center(child: AspectRatio(...)))`** → el visor reparte el
  alto restante de la ventana y se centra: **la pantalla del anfitrión se ve
  COMPLETA a cualquier tamaño** (el registro queda con `flex: 1`).
- El mapeo del ratón **no cambia**: el `box` de `LayoutBuilder` sigue siendo
  exactamente el área de la imagen (la relación de aspecto de la caja sigue
  siendo la de la imagen) → coordenadas 0..65535 exactas como antes.

## 2) ID: dónde obtenerlo (F2)

Antes el ID solo aparecía en una línea del registro
(`escuchando en … (node ONDH6IU4RU)`). Ahora:

1. **`anyremoted id`** (subcomanda nueva): carga — o crea la primera vez — el
   **mismo `identity.key`** que usa `listen` e imprime el ID estable (10 chars
   Base32) en una línea. Disponible también para scripts.
2. **App**: lo consulta al arrancar y lo muestra en la tarjeta Conexión como
   **«ID de este equipo: XXXX XXXX XXXX»** con botón **Copiar ID**; el evento
   `listening` lo refresca.
3. **ID del anfitrión (remoto)**: en SU equipo en la propia fila «ID de este
   equipo» (y en su registro: `node …`); en la sesión aparece en **«SESIÓN
   ACTIVA con XXX ✓ verificado»** (ambos lados).
4. **Aviso al escribir un ID en el campo de destino**: «por ID aún no se puede
   conectar (resolutor pendiente): usa la dirección del anfitrión (ip:puerto)»
   — antes fallaba como «perfil no encontrado». La **conexión por ID** sigue
   pendiente de un resolutor (ya registrada como uso futuro de RF-03).

## Tests (3 nuevos)

| Test | Qué cubre |
|---|---|
| Rust · `id_subcommand_prints_a_stable_well_formed_node_id` | `anyremoted id` → exit 0, ID de 10 caracteres `A–Z/2–7`, **estable entre invocaciones** e `identity.key` persistido |
| Flutter · `C-9: el visor remoto cabe en la ventana (sin desbordes)` | Inyecta un frame 1280×720 y verifica **sin excepción** de layout a 800×600 y **1600×900 (maximizada)**, con la tarjeta y el registro visibles |
| Flutter · `C-9: ID de este equipo visible y copiable` | Fila con el ID, `SelectableText` y tooltip «Copiar ID» |

**Control negativo verificado** (para que el gate no sea vacuo): con el layout
ANTIGUO (tarjeta rígida) el test de overflow **FALLA** (el desborde se captura
como excepción); con el fix **pasa**. Se ejecutó revertiendo solo los tres
cambios de layout y se restauró después.

## Regresión

Rust **44/0** · clippy **0/0** · fmt **0 diffs** · Flutter **analyze limpio +
7/7**. Los tests de las rondas anteriores (incluido el listener repetidor de
v0.8.2) pasan sin cambios.

## Limitaciones

- Conexión **por ID** aún no disponible (requiere resolutor/descubrimiento;
  pendiente en specs). El ID identifica al equipo; la conexión sigue siendo
  `ip:puerto` o perfil.
- En ventanas muy estrechas la ayuda lateral de la fila del ID colapsa con
  elipsis (el ID y el botón de copiar siguen visibles).
- El ancho máximo del frame sigue limitado por la calidad elegida (720p por
  defecto): el visor escala, no inventa detalle.
