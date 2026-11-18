# Gate C-34 — barras del visor en el viewport (v0.13.7)

**Fecha**: 2026-11-18 · Rama: `feature/c34-viewer-scrollbars` · Origen: campo
(validación de C-33): «con esto se resuelve, hace falta una barra para el
desplazamiento horizontal».

## Causa

En C-33 las dos barras se anidaron **al revés**: la `Scrollbar` horizontal
envolvía el `SingleChildScrollView` horizontal *dentro* del scroll vertical.
Como el contenido mide 2.880 px de alto (1080 × 150 %), la barra se pintaba al
final del **contenido**, no del **viewport**: nunca se veía en pantalla.

## Cambio

```
ClipRect
└─ Scrollbar(vertical, controller: _vScroll)        ← borde derecho del viewport
   └─ Scrollbar(horizontal, controller: _hScroll)   ← borde inferior del viewport
      └─ SingleChildScrollView(vertical)
         └─ SingleChildScrollView(horizontal)
            └─ SizedBox(frame * zoom) → superficie interactiva
```

- Ambas barras envuelven el **viewport** (no el contenido), con
  `thumbVisibility: true` y grosor 10 px.
- La horizontal filtra `notificationPredicate` por `Axis.horizontal` para no
  reaccionar al eje vertical.
- La rueda sigue siendo del remoto (el `PointerEvent` se reclama); el paneo es
  por barras, como en RustDesk (*scroll mode*) y AnyDesk.

## Gates (2026-11-18)

| Gate | Resultado |
|---|---|
| `flutter analyze` | limpio |
| `flutter test` | **27 / 27** (nuevo C-34: con un frame 1200×800 al 100 % en una ventana de 800×600, **las dos** barras quedan dentro de la ventana) |
| Rust | sin cambios (154 / 0 del gate C-33) |

## Verificación

El test C-34 reproduce el fallo: con el layout anterior la barra horizontal
quedaba en `y ≈ 800` (fuera de una ventana de 600 px); con el nuevo, ambas
barras están dentro del viewport. La comprobación visual definitiva es la de
campo: 1080p al 100 % → barra horizontal en el borde inferior del visor.
