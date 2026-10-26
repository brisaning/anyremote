# Gate Integración — daemon `anyremoted` + app de escritorio (v0.1.0)

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/daemon-integration` (D-11) · tag `integracion-v0.1.0` |
| Suite Rust | **32 suites ok, 0 fallos** (workspace completo, incl. daemon) |
| Gates Flutter | `flutter analyze` sin issues · `flutter test` 1/1 |

## Qué se integró (ciclos S0→C8 cableados en un producto)

**Binario `anyremoted`** (release, 3,9 MB) — subcomandos:
- `listen [--bind] [--clipboard off|memory|win] [--grant] [--max-secs]`: acepta UNA sesión — cert-store persistido (S0, validación real de root store), **autenticación mutua ed25519+binding** (C2), hello/welcome + concesión de permisos (C1), eco de latidos, **recepción de archivos con hash** (C5), **sincronización de portapapeles** (C4), auditoría JSONL (C7).
- `connect <host:puerto> | --profile <alias> [--send-file] [--grant] …`: sesión saliente con los mismos módulos + envío de archivo + métricas RTT (C7 report).
- `profiles [--list|--add a,h,p|--remove a]`: CRUD de perfiles TOML (C6) resueltos por `--profile`.
- **Salida**: eventos JSONL por stdout (contrato de la UI) + resumen de diagnóstico en stderr + `audit/*.jsonl`.

**App `anyremote_app.exe`**: botón *Conectar* spawnea `anyremoted connect` (junto al exe o `ANYREMOTE_DAEMON`), parsea eventos en vivo (estado: Conectando/Conectado·rtt/Error/Detener), registro con trazas del daemon, botón *Detener* (sigterm→sigkill).

## Evidencia ejecutada

1. **e2e en proceso** `daemon_listen_connect_full_integration` (5.4 s): auth mutua, permisos concedidos, latidos (≥2, RTT registrado), archivo 40 KB con `file_ack` hash-verificado en ambos reportes, **portapapeles sembrado viajó controlador→controlado** (`clipboard_final == semilla`), auditoría JSONL legible con `SessionEstablished/PermissionGranted/FileTransfer/Disconnected`, cierre limpio por max-secs en ambos lados.
2. **Smoke CLI con procesos reales**: `profiles --add smoke` → `listen` (21118) + `connect --profile smoke --send-file` → eventos:
   `connecting → session_active(peer autenticado) → file_sent → file_ack(sha256 070059c7…) → heartbeats rtt 0–1ms ×4 → closed(max_secs)` y en el listener `listening → session_active → file_received(23 B) → closed`; exit 0.
3. **Robustez**: cierre ante desaparición del par (FIN/RST → `closed{connection_gone}` en vez de error) + gracia de 150 ms tras Bye; transiciones de sesión corregidas (Active→Closing→Closed).
4. **Artefactos** (release):
   - `app\build\windows\x64\runner\Release\anyremote_app.exe` (+ `data\app.so` con el código nuevo verificado por marcadores)
   - `app\build\windows\x64\runner\Release\anyremoted.exe` (copiado desde `core\target\release`)
   - `core\target\release\anyremoted.exe` (origen)

## Prueba manual (para el usuario)

```
Terminal A:  C:\proyectos\fixed\anyremote\core\target\release\anyremoted.exe listen --clipboard win
Terminal B:  …\anyremoted.exe connect 127.0.0.1:21118 --clipboard win --send-file C:\ruta\archivo.txt
UI:          abrir anyremote_app.exe → destino 127.0.0.1:21118 → Conectar (la app lanza su propio connect)
```
Nota: cert-store compartido por defecto en `%APPDATA%\anyremote\` (misma máquina). Copiar `server_cert.pem` a la otra máquina para cross-machine (limitación v0.1 documentada).

## Limitaciones declaradas
- Una sesión por proceso daemon (accept único); streaming de vídeo hacia la UI = siguiente hito (M2+).
- Perfil `one_way` y persistencia de parciales de archivo: no cableados aún (diseño listo en crates).

---

# Addendum v0.1.2 — Escucha en red (LAN/pública) y confianza cross-machine

| Campo | Valor |
|---|---|
| Fecha | 2026-10-26 |
| Rama | `feature/lan-listen` (D-11) · tag `integracion-v0.1.2` |

## Cambios

1. **Escucha en todas las interfaces**: el botón de la app ahora lanza `anyremoted listen --bind 0.0.0.0:21118` (antes 127.0.0.1) y, ante el evento `listening`, **lista las IPs LAN** desde las que otros dispositivos pueden conectarse + regla de firewall sugerida (netsh).
2. **Bootstrap de certificado cross-machine** (rompía la confianza entre máquinas):
   - El **servidor envía su PEM de raíz en claro antes del TLS** (`ANYREMOTE-CERT-V1` + longitud + PEM) — dato público; el ancla de autenticación real la sigue poniendo la capa de aplicación (ed25519 + channel binding): un intermediario no puede enlazar dos sesiones TLS distintas → rechazo en `recv_validate_auth`.
   - El **cliente**: con pin local → compara huellas (SHA-256) y **aborta si difieren** ("posible intermediario"); sin pin → usa el PEM recibido para validar TLS y **solo lo persiste (pin) tras completar la auth mutua** (anti pin-poisoning).
   - Compat: si el par no envía bootstrap → error explícito con instrucción.
3. **Normalización de destino también en CLI** (`host,puerto` / `alias,host,puerto` → `host:puerto`).

## Gates
- Suite Rust **32/32, 0 fallos** (daemon e2e ejercita el flujo con bootstrap en dir compartido) · `flutter analyze` limpio · `flutter test` 1/1 (incluye normalización y botón de red).
- Auditoría C8 verde por diseño (sin verificadores inseguros: validación real + pinning).

## Instrucciones cross-device (prueba desde otro equipo)
```
1. Máquina A (anfitrión): app → "Escuchar (red)"   → el registro muestra "desde otra máquina usa: 192.168.x.x"
2. Si la conexión es rechazada, en A con admin:
   netsh advfirewall firewall add rule name="anyremote" dir=in action=allow protocol=TCP localport=21118
3. Máquina B (invitada): anyremoted connect 192.168.x.x:21118  (o la app con ese destino)
   → primer connect: recibe y pinea el cert del servidor (evento cert_pinned) tras auth OK.
4. Observar: session_active con el peer autenticado + heartbeats rtt.
```
Nota: `server_cert.pem` de A es el pin de B — si A regenera su cert (borrado), B debe borrar su pin para reaparear (mensaje de error con instrucción).
