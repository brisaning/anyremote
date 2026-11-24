# Bug de campo — Caps Lock solo afecta a los acentos (2026-11-24)

| Campo | Valor |
|---|---|
| Reportado por | el usuario, tras probar el **0.14.5** (24/11) |
| Binario | v0.14.5 (el resto del flujo de aceptación validado como correcto) |
| Ciclo | **C-54** (Caps Lock) — ya estaba planificado; este reporte **acota el síntoma y la causa** |
| Logs | `C:\Users\wnuke\AppData\Roaming\anyremote` (anfitrión) · `E:\audiany\anyremote` (cliente) |

## Síntoma

Con **Caps Lock** encendido en el cliente, al escribir en el equipo remoto:

- ❌ el **texto normal** (a, b, c…) **no** pasa a mayúsculas;
- ✅ los **caracteres acentuados** (á, é, ñ…) **sí** salen correctos.

## Causa (derivada del código, no del log)

El camino de una tecla es: cliente → wire → backend → `SendInput`.

1. **Cliente** (`app/lib/main.dart`): el `TextField` de entrada del visor entrega el
   **carácter ya resuelto por el SO del cliente** (con Caps → `'A'`), y el visor
   lo manda como `{"input":{"key":{"code":0,"ch":"A"}}}`.
2. **Backend** (`core/input/src/windows_backend.rs:46-54`): `Key::Char(c)` →
   `VkKeyScanW(c)` → para `'A'` devuelve **VK_A con `shift = true`** → inyecta
   **Shift + tecla**.
3. **Host**: con **Caps Lock encendido**, Shift + letra produce la **minúscula**
   (Caps invierte el efecto de Shift) → el texto normal sale en minúsculas.
4. **Acentuados** (`core/input/src/windows_backend.rs`, camino `KEYEVENTF_UNICODE`,
   introducido en C-37): se inyectan **por carácter**, no por VK+Shift → el
   estado de Caps/Shift del host **no interviene** → salen correctos.

Por eso el fallo aparece **solo en el texto normal**: el camino por VK arrastra el
estado de modificadores del host, y el camino por Unicode no.

**Conclusión**: no es un problema de `VK_CAPITAL` (la tecla de bloqueo sí llega y
cambia el estado del host), sino de **quién decide el modificador**: hoy lo decide
el SO del cliente (que trae el Caps ya aplicado dentro del carácter) y luego el
host lo invierte. Con dos distribuciones distintas (C-55) el problema es peor:
el VK que el host deduce de `VkKeyScanW` es **de la distribución del host**.

## Fix previsto (C-54, con el modelo de estado de C-55)

- El cliente debe enviar la **tecla base + el estado de los modificadores**
  (Caps/Shift), no el carácter resultante; el backend decide la combinación
  final. Así el Caps del **host** manda (que es lo que el usuario ve) y el
  cliente no lo hornea dos veces.
- Alternativa mínima (solo C-54): que el backend, ante unVK de letra con el
  **Caps del host** encendido, invierta el Shift que `VkKeyScanW` calculó.
  Requiere leer el estado de Caps del host (`GetKeyState(VK_CAPITAL)`) → es un
  parche; el modelo de estado de C-55 es el arreglo de fondo.

## Cómo se comprueba (gate)

1. Con Caps **encendido** en el cliente: escribir `Hola` → debe salir `HOLA` en
   el host.
2. Con Caps **encendido en el host** y apagado en el cliente: el host debe ver
   mayúsculas (su Caps manda).
3. Acentuados con Caps en cualquiera de los dos: deben seguir saliendo bien
   (no deben romperse).
4. Mayúsculas sostenidas (Shift mantenido): no deben «doblarse» al pasar por
   Caps.

## Nota de trazabilidad

Este bug es **de campo** y no lo detectó el CI: los tests de acentos (C-37/C-38)
son de composición en Dart y el backend nunca inyecta un carácter real. Queda
como hueco de cobertura (matriz de regresión, `anyremote-test`).
