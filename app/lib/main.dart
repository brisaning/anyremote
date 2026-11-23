import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'video_assembler.dart';

/// anyremote — shell de escritorio v0.1.0 con integración daemon (`anyremoted`).
///
/// Conexión real: la app spawnea `anyremoted connect <target>`, lee sus eventos
/// JSONL por stdout y refleja estado/registro en vivo. Ciclos S0–C8 en el backend.
void main() {
  runApp(const AnyremoteApp());
}

/// Versión de la app: la inyecta `scripts/build-release.ps1` desde `VERSION`
/// (`--dart-define=APP_VERSION`). Sin define, un build de desarrollo se
/// identifica como `dev` en vez de mentir con una versión fija.
const String kVersion = String.fromEnvironment('APP_VERSION', defaultValue: 'dev');
const String kGitRev = String.fromEnvironment('GIT_REV', defaultValue: 'desconocido');
const String kFlutterBuild = String.fromEnvironment('FLUTTER_BUILD', defaultValue: 'unknown');

/// Normaliza el destino escrito por el usuario: acepta `host:puerto`,
/// `host,puerto` y `alias,host,puerto` (formato profiles --add) → `host:puerto`.
/// Un valor sin separadores se interpreta como alias de perfil (sin ':' → --profile).
String normalizeTarget(String raw) {
  var s = raw.trim();
  final trio = RegExp(r'^([^,]+),([^,]+),(\d+)$').firstMatch(s);
  if (trio != null) return '${trio.group(2)}:${trio.group(3)}';
  final duo = RegExp(r'^([^:,]+),(\d+)$').firstMatch(s);
  if (duo != null) return '${duo.group(1)}:${duo.group(2)}';
  return s;
}

bool looksLikeAlias(String target) => !target.contains(':');

/// C-22: servidor de encuentro por defecto (el VPS propio `serveremote`) —
/// el campo de la app puede quedar vacío y la sesión igual sabe dónde
/// registrar/resolver por WAN.
const String kDefaultRdv = '51.222.81.81:21120';

/// C-22: valor efectivo del servidor de encuentro (vacío → [kDefaultRdv]).
String effectiveRdv(String raw) {
  final t = raw.trim();
  return t.isEmpty ? kDefaultRdv : t;
}

/// C-22: ¿la dirección resuelta es LAN (privada/loopback/ULA)? Para
/// etiquetar la auto-detección LAN/WAN en el registro.
bool isLanAddress(String hostPort) {
  String h;
  if (hostPort.startsWith('[')) {
    final end = hostPort.indexOf(']');
    if (end < 0) return false;
    h = hostPort.substring(1, end);
  } else {
    final ci = hostPort.lastIndexOf(':');
    h = ci > 0 ? hostPort.substring(0, ci) : hostPort;
  }
  if (h == '::1' || h.startsWith('fc') || h.startsWith('fd') || h.startsWith('fe80')) {
    return true;
  }
  final parts = h.split('.');
  if (parts.length != 4) return false;
  final o = int.tryParse(parts[0]) ?? -1;
  final second = int.tryParse(parts[1]) ?? -1;
  return o == 10 ||
      o == 127 ||
      (o == 172 && second >= 16 && second <= 31) ||
      (o == 192 && second == 168) ||
      (o == 169 && second == 254);
}

/// C-22: etiqueta LAN/WAN de una resolución (evento `resolve_ok` del daemon:
/// broadcast → LAN · rdv → WAN · caché → según la dirección resultante).
String resolveViaLabel(String source, String addr) => switch (source) {
      'broadcast' => 'LAN (broadcast)',
      'rdv' => 'WAN (servidor de encuentro)',
      _ => isLanAddress(addr) ? 'caché — LAN' : 'caché — WAN',
    };

/// Punto del visor → **coordenadas normalizadas 0..65535** del wire (RF-02a).
///
/// Independiente de la resolución del vídeo y de la pantalla del anfitrión: con
/// el mapeo anterior a píxeles de *frame* (v0.6.0-v0.6.1), un vídeo a 720p o
/// 1080p comprimía el clic al cuadrante superior-izquierdo de la pantalla y el
/// control remoto dejaba de funcionar (solo funcionaba en nativa).
(int, int) normalizedPointer(Offset local, Size box) {
  if (box.width <= 0 || box.height <= 0) return (0, 0);
  final nx = (local.dx / box.width * 65535).round().clamp(0, 65535);
  final ny = (local.dy / box.height * 65535).round().clamp(0, 65535);
  return (nx, ny);
}

/// Códigos de tecla del wire ARSP (RF-02a) — espejo de `arsp.proto::Key` y de
/// `anyremote-transport::arsp::input_event_from_wire`. `null` = tecla sin código
/// propio → se envía como carácter (`ch`).
int? keyCodeFor(LogicalKeyboardKey k) {
  if (k == LogicalKeyboardKey.escape) return 1;
  if (k == LogicalKeyboardKey.tab) return 2;
  if (k == LogicalKeyboardKey.enter || k == LogicalKeyboardKey.numpadEnter) return 3;
  if (k == LogicalKeyboardKey.backspace) return 4;
  if (k == LogicalKeyboardKey.delete) return 5;
  if (k == LogicalKeyboardKey.arrowLeft) return 6;
  if (k == LogicalKeyboardKey.arrowRight) return 7;
  if (k == LogicalKeyboardKey.arrowUp) return 8;
  if (k == LogicalKeyboardKey.arrowDown) return 9;
  if (k == LogicalKeyboardKey.home) return 10;
  if (k == LogicalKeyboardKey.end) return 11;
  // C-37: edición/navegación y bloqueos que faltaban.
  if (k == LogicalKeyboardKey.insert) return 12;
  if (k == LogicalKeyboardKey.pageUp) return 13;
  if (k == LogicalKeyboardKey.pageDown) return 14;
  if (k == LogicalKeyboardKey.capsLock) return 23;
  if (k == LogicalKeyboardKey.numLock) return 24;
  if (k == LogicalKeyboardKey.scrollLock) return 25;
  if (k == LogicalKeyboardKey.controlLeft || k == LogicalKeyboardKey.controlRight) return 20;
  if (k == LogicalKeyboardKey.altLeft || k == LogicalKeyboardKey.altRight) return 21;
  if (k == LogicalKeyboardKey.shiftLeft || k == LogicalKeyboardKey.shiftRight) return 22;
  // Teclas de función F1..F12 (30..41) — menores RF-02a.
  if (k == LogicalKeyboardKey.f1) return 30;
  if (k == LogicalKeyboardKey.f2) return 31;
  if (k == LogicalKeyboardKey.f3) return 32;
  if (k == LogicalKeyboardKey.f4) return 33;
  if (k == LogicalKeyboardKey.f5) return 34;
  if (k == LogicalKeyboardKey.f6) return 35;
  if (k == LogicalKeyboardKey.f7) return 36;
  if (k == LogicalKeyboardKey.f8) return 37;
  if (k == LogicalKeyboardKey.f9) return 38;
  if (k == LogicalKeyboardKey.f10) return 39;
  if (k == LogicalKeyboardKey.f11) return 40;
  if (k == LogicalKeyboardKey.f12) return 41;
  return null;
}

/// C-37: acentos que Windows trata como **dead key** (no producen carácter
/// hasta la siguiente pulsación).
bool isDeadAccent(String ch) => ch == '´' || ch == '`' || ch == '^' || ch == '¨' || ch == '~';

/// C-37: compone la dead key con la vocal/siguiente carácter. `null` si esa
/// pareja no forma carácter compuesto.
///
/// Antes la app enviaba `´` y después la vocal: el anfitrión escribía `´o` en
/// vez de `ó`. La composición se hace aquí (cliente) y viaja ya compuesta.
String? composeDeadAccent(String dead, String base) {
  const agudo = {
    'a': 'á', 'e': 'é', 'i': 'í', 'o': 'ó', 'u': 'ú', 'y': 'ý',
    'A': 'Á', 'E': 'É', 'I': 'Í', 'O': 'Ó', 'U': 'Ú', 'Y': 'Ý',
  };
  const grave = {
    'a': 'à', 'e': 'è', 'i': 'ì', 'o': 'ò', 'u': 'ù',
    'A': 'À', 'E': 'È', 'I': 'Ì', 'O': 'Ò', 'U': 'Ù',
  };
  const circunflejo = {
    'a': 'â', 'e': 'ê', 'i': 'î', 'o': 'ô', 'u': 'û',
    'A': 'Â', 'E': 'Ê', 'I': 'Î', 'O': 'Ô', 'U': 'Û',
  };
  const dieresis = {
    'a': 'ä', 'e': 'ë', 'i': 'ï', 'o': 'ö', 'u': 'ü',
    'A': 'Ä', 'E': 'Ë', 'I': 'Ï', 'O': 'Ö', 'U': 'Ü',
  };
  const tilde = {
    'a': 'ã', 'n': 'ñ', 'o': 'õ',
    'A': 'Ã', 'N': 'Ñ', 'O': 'Õ',
  };
  final tabla = switch (dead) {
    '´' => agudo,
    '`' => grave,
    '^' => circunflejo,
    '¨' => dieresis,
    '~' => tilde,
    _ => const <String, String>{},
  };
  return tabla[base];
}

/// C-41: segundos sin frames (null si el vídeo es reciente o nunca llegó).
/// Se usa para avisar de una imagen congelada en vez de mostrarla en silencio.
int? stallSeconds(int nowMs, int lastFrameMs, {int umbralMs = 5000}) {
  if (lastFrameMs <= 0) return null;
  final d = nowMs - lastFrameMs;
  return d > umbralMs ? (d / 1000).round() : null;
}

/// C-38: resuelve una dead key pendiente con el carácter que llega.
///
/// Devuelve `(acentoSuelto, caracterAEnviar)`:
/// - Si la pareja compone (`´`+`o`) → `(null, 'ó')`.
/// - Si el Sistema Operativo **ya compuso** el carácter y llega compuesto
///   (`´`+`o` → `ó`) → `(null, 'ó')`: antes se enviaba el acento suelto y
///   quedaba `´ó` en el anfitrión.
/// - Si no compone y el carácter es ASCII (`´`+`x`) → `('´', 'x')`, como el SO.
(String?, String) resolveDeadKey(String dead, String base) {
  final compuesto = composeDeadAccent(dead, base);
  if (compuesto != null) return (null, compuesto);
  if (base.codeUnitAt(0) > 0x7F) return (null, base); // ya venía compuesto
  return (dead, base);
}

/// Flecha del cursor remoto (C-11) — dibujo propio en canvas, sin assets.
/// La punta en (0,0) coincide con la posición exacta del cursor (hotspot).
class _RemoteCursorPainter extends CustomPainter {
  const _RemoteCursorPainter();

  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / 13.0;
    final path = Path()
      ..moveTo(0, 0)
      ..lineTo(0, 16.5 * s)
      ..lineTo(4.0 * s, 12.8 * s)
      ..lineTo(6.6 * s, 18.8 * s)
      ..lineTo(8.9 * s, 17.7 * s)
      ..lineTo(6.3 * s, 11.9 * s)
      ..lineTo(11.9 * s, 11.9 * s)
      ..close();
    canvas.drawPath(
        path,
        Paint()
          ..color = Colors.black
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.4);
    canvas.drawPath(
        path,
        Paint()
          ..color = Colors.white
          ..style = PaintingStyle.fill);
  }

  @override
  bool shouldRepaint(covariant _RemoteCursorPainter oldDelegate) => false;
}

/// Estado inmutable del overlay del cursor remoto (C-11 → C-19): al vivir en
/// un `ValueNotifier` el overlay repinta SOLO el visor, no la página entera.
class _CursorState {
  const _CursorState({this.nx, this.ny, this.visible = false});

  /// Posición normalizada 0..65535 del anfitrión (null = sin posición).
  final int? nx;
  final int? ny;
  final bool visible;
}

/// Argumentos de `anyremoted listen` (extraídos para testearlos).
///
/// v0.8.2: **`--max-sessions 0`** hace el listener *repetidor*: tras cerrar
/// una sesión (p. ej. el invitado pulsa «Detener») vuelve a `accept()` en
/// lugar de terminar — antes el proceso moría (y de paso se colgaba) y el
/// estado «Escuchar (red)» quedaba activo sin aceptar reconexiones.
List<String> buildListenArgs({
  required String size,
  required int kbps,
  bool udp = false,
  String rdv = '',
}) =>
    [
      'listen',
      '--bind', '0.0.0.0:21118',
      // C-15: udp = QUIC con stream de vídeo dedicado; el listener acepta
      // TCP y UDP a la vez, así que los peers viejos siguen conectando.
      if (udp) ...['--transport', 'udp'],
      // C-17: servidor de encuentro (rendezvous + relay) para WAN.
      if (rdv.isNotEmpty) ...['--rdv', rdv],
      '--clipboard', 'win',
      // M2: el escuchador comparte SU pantalla por defecto (sino no hay qué ver).
      '--video', 'on',
      '--video-size', size,
      '--video-fps', '15',
      '--max-bitrate', '$kbps',
      // C-14: H.264 por Media Foundation (menos CPU que VP9); si la máquina
      // no tiene MFT utilizable, el daemon cae a VP9 y lo dice en el log.
      '--codec', 'h264',
      // C-2/C-45: la **capacidad** de entrada viaja siempre armada para que
      // los toggles sigan siendo dinámicos durante la sesión, pero **no**
      // concede nada por sí sola: el **estado** arranca apagado y solo lo
      // enciende `{"perm":{"input":true}}` (ver `_sendPerms`). La app nunca
      // pasa `--input-on` (C-45, F-02): compartir pantalla ≠ dar teclado.
      '--allow-input',
      // C-44 (F-01): autorización del par. La app **pregunta** antes de abrir
      // sesión (diálogo de sesión entrante); el daemon bloquea el handshake
      // hasta la decisión y deniega por timeout si no llega.
      '--accept', 'ask',
      '--audio', 'on',
      '--audio-source', 'system,mic',
      // v0.8.2: escucha repetidora (ver doc de esta función).
      '--max-sessions', '0',
    ];

/// C-45 (F-02): resumen honesto de lo que comparte el listener al abrirse.
/// El control remoto es **opt-in**: con el toggle apagado el anfitrión queda
/// en «solo ver» aunque la capacidad `--allow-input` viaje armada (capacidad
/// ≠ estado; el daemon solo inyecta tras `{"perm":{"input":true}}`).
String shareSummary({
  required bool control,
  required bool audio,
  required bool mic,
}) {
  final activos = <String>[
    if (control) 'control remoto',
    if (audio) 'audio del sistema',
    if (mic) 'micrófono',
  ];
  return activos.isEmpty
      ? 'solo ver (control remoto y audio apagados; se pueden activar en caliente)'
      : 'acepta ${activos.join(' + ')}';
}

/// C-44 (F-01): línea de decisión que la UI escribe en el stdin del listener
/// (`--accept ask`). `choice`: `accept` (una vez) · `trust` (recordar) · `reject`.
String admissionLine({required String peer, required String choice}) {
  if (choice == 'accept' || choice == 'trust') {
    return jsonEncode({
      'accept': peer,
      if (choice == 'trust') 'trust': true,
    });
  }
  return jsonEncode({'reject': peer});
}

/// C-53: ventana durante la que se recuerda la decisión de un par. El cliente
/// abre varios intentos (QUIC → TCP → relay) y cada intento emite su propio
/// `session_pending`; sin memoria, el diálogo reaparecía sobre un intento ya
/// decidido (reporte de campo 2026-11-23, P3).
const kAdmissionDecisionWindow = Duration(seconds: 60);

/// C-53: líneas que la UI escribe en el stdin del listener al decidir por
/// PRIMERA vez.
///
/// Al **aceptar**, el `{"perm":...}` va **antes** de la decisión: el daemon
/// encola los permisos mientras espera la decisión y los aplica al abrir la
/// sesión, así que el primer clic del invitado ya encuentra `input:true` (el
/// bug de campo eran 244 `input_injected` que el usuario no "sentía" porque el
/// anfitrión estaba en «solo ver»).
///
/// Al **rechazar** no se envía ningún permiso: rechazar no debe alterar el
/// estado del anfitrión.
List<String> admissionWrites({
  required String peer,
  required String choice,
  required bool audio,
  required bool mic,
}) {
  // `admissionLine` degrada a rechazo cualquier elección desconocida; aquí se
  // replica para no conceder permiso en ese caso (fail-closed).
  if (choice != 'accept' && choice != 'trust') {
    return [admissionLine(peer: peer, choice: choice)];
  }
  return [
    jsonEncode({
      'perm': {'input': true, 'audio': audio, 'mic': mic},
    }),
    admissionLine(peer: peer, choice: choice),
  ];
}

/// Ubica `anyremoted.exe`: env ANYREMOTE_DAEMON (override) o junto al ejecutable.
String resolveDaemonPath() {
  final override = Platform.environment['ANYREMOTE_DAEMON'];
  if (override != null && override.isNotEmpty) return override;
  final exe = Platform.resolvedExecutable;
  final dir = exe.substring(0, exe.lastIndexOf(Platform.pathSeparator));
  return '$dir${Platform.pathSeparator}anyremoted.exe';
}

class AnyremoteApp extends StatelessWidget {
  const AnyremoteApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'anyremote',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF2E7D6F),
          brightness: Brightness.dark,
        ),
        useMaterial3: true,
      ),
      home: const HomePage(),
    );
  }
}

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final TextEditingController _target = TextEditingController(text: '');
  String _status = 'Desconectado';
  Color _statusColor = Colors.grey;
  Process? _daemon;
  Process? _listener;

  // Perfiles (C6) cargados del daemon: `anyremoted profiles --list`.
  List<String> _aliases = [];
  bool _loadingProfiles = false;

  // RF-02a: entrada en sesión — foco del visor, sesión controladora activa,
  // teclas de carácter mantenidas (para emparejar keyup con su carácter) y el
  // flag D-07 que añade `--allow-input` al listener (denegado por defecto).
  final FocusNode _inputFocus = FocusNode();
  final Map<int, String> _heldChars = {};
  /// C-37: dead key pendiente de componer (`´`, `` ` ``, `^`, `¨`, `~`).
  String? _deadPending;
  bool _sessionActive = false;
  bool _acceptRemoteControl = false;
  /// C-44: evita apilar diálogos si llegan varias solicitudes (el daemon
  /// atiende una sesión a la vez, pero la UI debe ser idempotente).
  bool _admissionPromptOpen = false;
  /// C-53: decisión ya tomada por par (`peer` → elección + instante). El
  /// cliente reintenta por QUIC → TCP → relay y cada intento emite otro
  /// `session_pending`; recordar la decisión durante
  /// `kAdmissionDecisionWindow` evita volver a preguntar lo ya decidido.
  final Map<String, ({String choice, DateTime at})> _admissionDecisions = {};
  bool _shareAudio = false;
  bool _shareMic = false;
  /// Calidad del vídeo que comparte el listener. **720p por defecto**: en la
  /// prueba de campo de 2026-10-27 `native` solo alcanzó ~4.6 fps en el
  /// anfitrión y el enlace marcó RTT 57–782 ms (vídeo demasiado pesado).
  String _videoQuality = '720p';
  /// C-15: transporte UDP/QUIC para listen y connect. **Preseleccionado desde
  /// v0.12.3** (más rápido que TCP: handshake integrado con TLS 1.3 en 1 RTT
  /// y sin bloqueo entre streams): el par TCP-only sigue conectando (fallback
  /// `quic_fallback` a los 3 s) y el listener ata TCP+UDP en el mismo puerto.
  bool _useUdp = true;
  /// C-17/C-22: servidor de encuentro (rendezvous + relay) — el campo
  /// arranca **vacío** y el valor efectivo lo fija `effectiveRdv()` (VPS
  /// propio por defecto): el daemon resuelve el ID broadcast LAN → rdv →
  /// caché (**auto-detección LAN/WAN**) y la LAN nunca depende del VPS.
  /// Escribir otra dirección sustituye al default.
  final TextEditingController _rdv = TextEditingController(text: '');
  static const Map<String, String> _qualities = {
    '720p': '1280x720',
    '1080p': '1920x1080',
    'Nativa': 'native',
  };

  /// Tope de ancho de banda por calidad en kbps (QW-3: `--max-bitrate`).
  ///
  /// C-32 (campo 17/11): 3/6/8 Mbps daban 0,06–0,22 bits/píxel/frame y el
  /// texto del escritorio quedaba ilegible en las tres calidades. Con LAN
  /// directa (rtt 3–4 ms) sobran 6–25 Mbps; en WAN el adaptativo sigue
  /// bajando hasta lo que aguante el enlace (el tope solo pone el techo).
  static const Map<String, int> _qualityKbps = {
    '720p': 6000,
    '1080p': 15000,
    'Nativa': 25000,
  };
  /// Botón mantenido en el visor (0/1/2) para emitir su `up` al soltar.
  int? _heldButton;
  /// Contador visible de eventos de entrada (enviados por el visor o
  /// inyectados/observados como anfitrión) — sin volcar cada uno al registro.
  int _inputEvents = 0;
  /// Indicador de audio en la cabecera del visor (`audio ✓` / `sin señal`).
  String _audioHint = '';
  /// C-40: `mic` / `system` / `mic,system` — el anfitrión lo anuncia en
  /// `audio_start` y el visor lo usa para no decir «sonido del sistema»
  /// cuando en realidad comparte el micrófono.
  String _audioSource = '';
  /// Último motivo de rechazo acusado (para no repetirlo en cada evento).
  String _lastInputDenied = '';
  /// C-44/C-45 (v0.14.2): dedupe del aviso de denegación en el ANFITRIÓN.
  /// Sin esto, cada movimiento de ratón denegado añadía una línea (608 en una
  /// sesión del 20/11) e inundaba el registro.
  String _lastHostDenied = '';

  // C-9: ID estable de este equipo (identidad D-03) — visible en la tarjeta
  // Conexión. Proviene de `anyremoted id` al arrancar o del evento `listening`.
  String? _localNodeId;

  // Hito M2: visor remoto (preview local del daemon controlador).
  Socket? _preview;
  /// C-23: `dispose` en marcha — `Socket.destroy()` dispara `onDone` de
  /// forma SÍNCRONA y esos cierres llaman a `_stopViewer`/`setState` con el
  /// elemento ya `defunct` (crash de depuración al cerrar con el visor
  /// abierto). Con la bandera, las rutas de cierre se saltan el `setState`.
  bool _disposed = false;
  /// C-28: timer de tasa del visor (log cada5 s, sin setState).
  Timer? _fpsLogTimer;
  /// C-19: ensamblador de frames con buffer amortizado — antes el buffer
  /// completo se recopilaba en CADA chunk de TCP (coste cuadrático/frame).
  final VideoFrameAssembler _assembler = VideoFrameAssembler();
  /// C-19: frame + estado + cursor viven en NOTIFIERS: solo el visor se
  /// repinta por frame (antes: `setState` de la página entera ~30 veces/s y
  /// ~20 Hz por el cursor remoto).
  final ValueNotifier<ui.Image?> _frame = ValueNotifier<ui.Image?>(null);
  final ValueNotifier<String> _videoStatus = ValueNotifier<String>('sin vídeo');
  final ValueNotifier<_CursorState> _cursor =
      ValueNotifier<_CursorState>(const _CursorState());
  /// Los dos listenables del visor, combinados (un solo `ListenableBuilder`).
  late final Listenable _viewerPaint = Listenable.merge([_frame, _cursor]);
  int _videoFrames = 0;
  double _renderFps = 0;
  /// C-33: zoom del visor. `0` = ajustar a la ventana; `1.0` = 1:1 (un píxel
  /// remoto = un píxel físico). Con un anfitrión 4K y un cliente 1080p,
  /// «ajustar» reduce el texto a la mitad (8 px) y no se lee: 100 % es la
  /// forma de leerlo (se desplaza con las barras).
  double _viewerZoom = 0;
  /// C-33: tamaño dibujado del último frame (para el log de escala).
  Size _viewerBox = Size.zero;
  final ScrollController _hScroll = ScrollController();
  final ScrollController _vScroll = ScrollController();
  int _lastRenderMs = 0;
  /// C-41: instante del último frame RENDERIZADO. Si el visor deja de recibir
  /// (red caída), antes seguía mostrando la última imagen en silencio: en WAN
  /// se quedó 50 s congelado sin avisar.
  int _lastFrameMs = 0;
  /// C-41: ya se avisó del congelamiento actual (para loguear una vez).
  bool _stallLogged = false;
  /// Última actualización del texto de cabecera (C-19: tope de 2 Hz).
  int _lastStatusMs = 0;
  /// C-27: errores de decode vistos (para tasar el registro: en campo un
  /// MFT con SPS in-band inundó el log con735 líneas/sesión).
  int _videoDecodeErrors = 0;
  bool _rendering = false;
  /// C-42: contadores del visor para decidir si hace falta una `Texture`
  /// nativa: frames RECIBIDOS, RENDERIZADOS, DESCARTADOS por el guard de
  /// reentrada y coste medio del render (ms).
  int _framesRecv = 0;
  int _framesRendered = 0;
  int _framesDropped = 0;
  double _renderCostMs = 0;
  final List<String> _log = [
    '[build] anyremote $kVersion · rev $kGitRev',
    '[build] Flutter $kFlutterBuild · windows-x64',
    '[core] ciclos S0..C8 completos · daemon anyremoted integrado',
  ];

  @override
  void initState() {
    super.initState();
    _loadLocalId();
    WidgetsBinding.instance.addPostFrameCallback((_) => _refreshProfiles());
  }

  /// C-9: obtiene el ID estable del equipo (`anyremoted id`, D-03) para
  /// mostrarlo en la UI sin iniciar la escucha. Silencioso si no hay daemon
  /// (entonces el ID llega con el evento `listening` al pulsar Escuchar).
  Future<void> _loadLocalId() async {
    final daemon = resolveDaemonPath();
    if (!File(daemon).existsSync()) return;
    try {
      final res = await Process.run(daemon, ['id']);
      final out = (res.stdout ?? '').toString().trim();
      if (res.exitCode == 0 && RegExp(r'^[A-Z2-7]{10}$').hasMatch(out)) {
        if (mounted) setState(() => _localNodeId = out);
      }
    } catch (_) {
      // sin daemon no hay ID — se mostrará al iniciar la escucha.
    }
  }

  /// Costura de test (C-9/C-19): inyecta un frame en el visor por el MISMO
  /// notifier que usa el render real, para verificar el layout sin red.
  void debugShowViewerFrame(ui.Image image) {
    final prev = _frame.value;
    _frame.value = image;
    prev?.dispose();
    setState(() {});
  }

  /// Costura de test (C-19): empuja bytes crudos por el mismo camino que el
  /// preview (ensamblador → render) sin socket.
  void debugPushVideoBytes(Uint8List chunk) => _onVideoBytes(chunk);

  /// Costura de test (C-42): contadores del visor.
  (int, int, int, double) get debugRenderStats =>
      (_framesRecv, _framesRendered, _framesDropped, _renderCostMs);

  /// Costura de test (C-23): abre el visor contra un puerto local para
  /// verificar el estado «esperando primer frame» con input activo.
  void debugStartViewer(int port) => unawaited(_startViewer(port));

  /// Costura de test (C-25): inyecta una línea de evento del daemon.
  void debugHandleEvent(String line) => _handleEvent(line);

  /// Costura de test (C-9): fija el ID local mostrado en la tarjeta Conexión.
  void debugSetLocalNodeId(String id) {
    setState(() => _localNodeId = id);
  }

  /// Costura de test (C-10): cierra el visor para verificar que el layout
  /// vuelve a las secciones Conexión + Registro.
  void debugStopViewerForTest() {
    unawaited(_stopViewer());
    setState(() {});
  }

  /// Costura de test (C-11): fija el cursor remoto del overlay.
  void debugSetRemoteCursor(int nx, int ny, bool visible) {
    _cursor.value = _CursorState(nx: nx, ny: ny, visible: visible);
  }

  /// Carga los aliases de perfil desde el daemon (`profiles --list`, JSONL).
  Future<void> _refreshProfiles() async {
    final daemon = resolveDaemonPath();
    if (!File(daemon).existsSync()) {
      _append('[ui] perfiles: anyremoted.exe no encontrado en $daemon');
      return;
    }
    setState(() => _loadingProfiles = true);
    try {
      final res = await Process.run(daemon, ['profiles', '--list']);
      final out = (res.stdout ?? '').toString();
      final aliases = <String>[];
      for (final line in const LineSplitter().convert(out)) {
        final t = line.trim();
        if (!t.startsWith('{')) continue;
        try {
          final v = jsonDecode(t) as Map<String, dynamic>;
          if (v['event'] == 'profile' && v['alias'] is String) {
            aliases.add(v['alias'] as String);
          }
        } catch (_) {}
      }
      if (mounted) {
        setState(() {
          _aliases = aliases;
          _loadingProfiles = false;
        });
        _append(aliases.isEmpty
            ? '[ui] sin perfiles guardados (anyremoted profiles --add alias,host,puerto)'
            : '[ui] perfiles cargados: ${aliases.join(", ")}');
      }
    } catch (e) {
      if (mounted) setState(() => _loadingProfiles = false);
      _append('[ui] error leyendo perfiles: $e');
    }
  }

  void _append(String line) {
    _logToFile(line);
    setState(() => _log.add(line));
  }

  /// C-28: como `_append` pero SIN `setState` — el log de tasa NO debe
  /// forzar repaints (enmascararía el problema que mide). El archivo y la
  /// lista se actualizan; la UI lo mostrará en el próximo rebuild natural.
  void _appendQuiet(String line) {
    _logToFile(line);
    _log.add(line);
  }

  void _setStatus(String text, Color color) {
    setState(() {
      _status = text;
      _statusColor = color;
    });
  }

  /// Log persistente de la app (C-5): TODAS las líneas del registro se añaden
  /// a `%APPDATA%\anyremote\ui.log` (misma carpeta que `audit/`, que ya se
  /// copia para diagnóstico). Sin esto, si la app se cierra el registro se
  /// pierde con la ventana. Se trunca al superar 1 MB.
  static File? _uiLogFile;

  static File? uiLogFile() {
    final base = Platform.environment['APPDATA'];
    if (base == null || base.isEmpty) return null;
    final dir = '$base\\anyremote';
    final f = File('$dir\\ui.log');
    try {
      Directory(dir).createSync(recursive: true);
    } catch (_) {}
    return f;
  }

  void _logToFile(String line) {
    try {
      final f = _uiLogFile ??= uiLogFile();
      if (f == null) return;
      if (f.existsSync() && f.lengthSync() > 1024 * 1024) f.deleteSync();
      f.writeAsStringSync('${DateTime.now().toIso8601String()} $line\n',
          mode: FileMode.append);
    } catch (_) {}
  }

  /// `isListener`: la línea viene del proceso que COMPARTE (rol anfitrión).
  /// En ese rol no se abre el visor: el anfitrión no debe mostrar su propia
  /// pantalla compartida, solo el registro (requisito de campo C-5).
  void _handleEvent(String line, {bool isListener = false}) {
    final trimmed = line.trim();
    if (trimmed.isEmpty) return;
    if (trimmed.startsWith('{')) {
      try {
        final ev = jsonDecode(trimmed) as Map<String, dynamic>;
        final kind = ev['event'] as String? ?? 'unknown';
        switch (kind) {
          case 'listening':
            _append('[daemon] anyremoted v${ev['daemon'] ?? '?'} escuchando en ${ev['bind']} (node ${ev['node_id']})');
            // C-9: el evento trae el mismo ID estable que muestra `anyremoted id`.
            final node = ev['node_id'];
            if (node is String && node.isNotEmpty) {
              setState(() => _localNodeId = node);
            }
            if (_daemon == null) _setStatus('Escuchando (red)', Colors.teal);
            _showLanIps();
          case 'connecting':
            _append('[daemon] anyremoted v${ev['daemon'] ?? '?'} conectando a ${ev['target']}…');
            _setStatus('Conectando', Colors.orange);
          case 'session_pending':
            // C-44: el anfitrión decide; el daemon bloquea el handshake hasta
            // recibir `{"accept"|"reject":"<ID>"}` por stdin.
            final pendingPeer = (ev['peer'] as String?) ?? '?';
            // C-53: el mismo cliente reintenta por varios transportes y cada
            // intento emite `session_pending`. Si ya se decidió hace poco, se
            // reenvía la misma decisión sin volver a preguntar; si no, se
            // pregunta (un segundo evento con el diálogo abierto se ignora).
            final recent = _recentAdmission(pendingPeer);
            if (recent != null) {
              _append('[daemon] sesión entrante de $pendingPeer — decisión '
                  'reciente ($recent): se reenvía sin volver a preguntar');
              _writeAdmission(
                  peer: pendingPeer, choice: recent, replay: true);
            } else {
              _append('[daemon] sesión entrante de $pendingPeer — esperando tu decisión');
              unawaited(_promptIncoming(pendingPeer));
            }
          case 'peer_rejected':
            _append('[daemon] sesión rechazada (${ev['peer'] ?? '?'}): '
                '${ev['reason'] ?? 'sin motivo'}');
          case 'session_active':
            // D-03: `peer_trusted` = este equipo ya se autenticó antes (TOFU).
            _append('[daemon] SESIÓN ACTIVA con ${ev['peer']}'
                '${ev['peer_trusted'] == true ? ' ✓ verificado' : ''} (${ev['role']})');
            _setStatus('Conectado', Colors.green);
            // La entrada solo sale desde el rol controlador (el visor).
            if (ev['role'] == 'controller') _sessionActive = true;
          case 'heartbeat':
            _setStatus('Conectado · rtt ${ev['rtt_ms']} ms', Colors.green);
          case 'file_ack':
            _append('[daemon] archivo confirmado por el receptor (hash ${ev['sha256']})');
          case 'file_received':
            _append('[daemon] archivo recibido: ${ev['name']} (${ev['bytes']} B)');
          case 'clipboard_synced':
            _append('[daemon] portapapeles sincronizado (${ev['dir']})');
          case 'clipboard_error':
            _append('[daemon] portapapeles desactivado en esta sesión (${ev['dir']}): ${ev['detail']}');
          case 'file_error':
            _append('[daemon] transferencia fallida (la sesión continúa): ${ev['detail']}');
          case 'client_failed':
            _append('[daemon] cliente rechazado desde ${ev['peer']}: ${ev['detail']} (el listener sigue escuchando)');
          case 'cert_pinned':
            _append('[daemon] certificado del servidor pineado para ${ev['target']}');
          case 'preview':
            // C-5: el anfitrión ignora SU preview (no muestra la pantalla que
            // comparte); solo el controlador abre visor con su propio preview.
            if (isListener) return;
            _startViewer((ev['port'] as num).toInt());
          case 'resolve_ok':
            // C-22: auto-detección LAN/WAN del ID — el daemon resuelve
            // broadcast (LAN) → servidor de encuentro (WAN) → caché; aquí
            // se pinta el camino usado en el registro.
            _append('[ui] ID resuelto → ${resolveViaLabel('${ev['source']}', '${ev['addr']}')}: ${ev['addr']}');
          case 'rdv_error':
            _append('[ui] servidor de encuentro no respondió (${ev['target']}): ${ev['detail']}');
          case 'video_start':
            _append('[daemon] vídeo iniciando: ${ev['width']}x${ev['height']} @${ev['fps']} fps (${ev['codec']})');
          case 'video_stats':
            // C-19: el texto va por SU notifier → sin `setState` de página.
            _videoStatus.value =
                'vídeo ${ev['w']}x${ev['h']} · ${ev['frames']} frames';
          case 'video_adapt':
            // C-41: el RTT que disparó la decisión (diagnóstico de WAN).
            final rtt = ev['rtt'];
            _append('[daemon] calidad adaptada: ${ev['reason']} → ${ev['size']}'
                '${ev['bps'] != null ? ' @${ev['bps']} bps' : ''}'
                '${rtt != null ? ' · rtt ${rtt}ms' : ''}');
          case 'video_backend':
            // C-14: backend realmente usado (mf_h264 o fallback vpx_vp9).
            _append('[daemon] códec de vídeo: ${ev['encoder']}'
                '${ev['detail'] != null ? ' — ${ev['detail']}' : ''}');
          case 'video_error':
            // C-25: el detalle de decode al registro. C-27: TASAADO — en
            // campo un MFT con SPS in-band por frame inundó el log (735
            // líneas en una sesión) y cada `_append` reconstruía la página.
            // Primeros3 completos + resumen cada50; la auditoría sigue
            // grabando TODOS los errores.
            _videoDecodeErrors += 1;
            final nErr = _videoDecodeErrors;
            if (nErr <= 3 || nErr % 50 == 0) {
              _append('[daemon] vídeo: ${ev['detail'] ?? ev}'
                  '${nErr > 3 ? ' (errores: $nErr)' : ''}');
            }
          case 'cursor':
            // C-11: posición del cursor del anfitrión → overlay sobre el
            // vídeo. Sin visor no hay dónde dibujarlo. Sin `_append`: ~20 Hz
            // inundarían el registro. C-19: su notifier repinta SOLO el
            // visor (antes: `setState` de página ~20 veces/s).
            final cnx = ev['nx'] as num?;
            final cny = ev['ny'] as num?;
            if (cnx != null &&
                cny != null &&
                (_frame.value != null || _preview != null)) {
              _cursor.value = _CursorState(
                nx: cnx.toInt(),
                ny: cny.toInt(),
                visible: ev['visible'] == true,
              );
            }
          case 'file_resume':
            _append('[daemon] reanudando envío de ${ev['name']} desde offset ${ev['from_offset']}');
          case 'file_rejected':
            _append('[daemon] oferta rechazada: ${ev['detail']}');
          case 'input_injected':
            // RF-02a: no se loguea cada evento (inundaría el registro); solo el contador.
            setState(() => _inputEvents += 1);
          case 'input_denied':
            final inpWhy = ev['allow_input'] != true
                ? 'capacidad de entrada desactivada (--allow-input)'
                : ev['input_on'] == false
                    ? 'control remoto apagado en el anfitrión'
                    : ev['reason'] == 'unsupported'
                        ? 'evento de entrada no soportado'
                        : 'permiso ${ev['perm'] ?? '?'} no concedido';
            if (inpWhy != _lastHostDenied) {
              _lastHostDenied = inpWhy;
              _append('[daemon] entrada denegada en el anfitrión: $inpWhy'
                  '${ev['input_on'] == false ? ' — activa «Control remoto» para permitir teclado/ratón' : ''}');
            }
          case 'input_error':
            _append('[daemon] evento de entrada no soportado: ${ev['detail']}');
          case 'input_result':
            // Acuse del anfitrión (RF-02a): se cuentan las INYECTADAS de verdad
            // y los rechazos se loguean solo cuando cambia el motivo.
            if (ev['ok'] == true) {
              _lastInputDenied = '';
              setState(() => _inputEvents += 1);
            } else {
              final why = '${ev['perm'] ?? ''} ${ev['detail'] ?? ''}'.trim();
              if (why.isNotEmpty && why != _lastInputDenied) {
                _lastInputDenied = why;
                _append('[daemon] entrada RECHAZADA por el anfitrión: $why');
              }
            }
          case 'audio_start':
            _audioSource = (ev['source'] as String?) ?? '';
            _append(ev.containsKey('source')
                ? '[daemon] audio compartido desde ${ev['source']} (${ev['rate']} Hz)'
                : '[daemon] reproducción de audio activada (${ev['rate']} Hz)');
          case 'audio_denied':
            _append(ev['detail'] != null
                ? '[daemon] audio no concedido al inicio: ${ev['detail']}'
                : '[daemon] audio no concedido (D-07: activa "Audio del sistema" o "Micrófono" en el anfitrión)');
          case 'permissions_updated':
            // C-2: los toggles cambiaron en caliente (se aplica sin reconectar).
            // v0.14.2: al activar el control se rearma el aviso de denegación.
            if (ev['input'] == true) _lastHostDenied = '';
            _append('[daemon] permisos actualizados: control ${ev['input'] == true ? 'activado' : 'desactivado'}'
                ' · audio ${ev['audio'] == true ? 'activo' : 'inactivo'}'
                ' · mic ${ev['mic'] == true ? 'activo' : 'inactivo'}');
          case 'audio_stopped':
            _append('[daemon] audio detenido (permisos actualizados)');
          case 'audio_stats':
            // C-40: diagnóstico en la cabecera. Se distingue el micrófono del
            // audio del sistema: antes, con el mic activo, decía «¿está
            // reproduciendo sonido el anfitrión?» y confundía la validación.
            final rms = (ev['rms'] as num?)?.toDouble() ?? 0.0;
            final soloMic = _audioSource == 'mic';
            final etiqueta = soloMic ? 'mic' : 'audio';
            final hint = rms > 0.002 ? '$etiqueta ✓' : '$etiqueta: sin señal';
            if (_audioHint != hint) {
              setState(() => _audioHint = hint);
              if (hint != '$etiqueta ✓') {
                _append(soloMic
                    ? '[daemon] micrófono sin señal (rms 0): el anfitrión no está captando voz'
                    : '[daemon] audio sin señal (rms 0): ¿está reproduciendo sonido el anfitrión?');
              }
            }
          case 'audio_error':
            _append('[daemon] audio desactivado en esta sesión: ${ev['detail']}');
          case 'closed':
            _append('[daemon] sesión cerrada: ${ev['reason']}');
            // C-10: fin de sesión → se limpia el visor; Conexión y Registro
            // vuelven a mostrarse (permite reconectar sin reiniciar la app).
            unawaited(_stopViewer());
            _sessionActive = false;
            _heldButton = null;
            _heldChars.clear();
            _deadPending = null;
            _audioHint = '';
            _audioSource = '';
            _cursor.value = const _CursorState();
            // v0.8.2: con el listener repetidor, cerrar una sesión (p. ej. el
            // invitado pulsa «Detener») vuelve a «Escuchando (red)»: sigue
            // aceptando reconexiones. Sin listener → Desconectado.
            if (isListener && _listener != null) {
              _setStatus('Escuchando (red)', Colors.teal);
            } else {
              _setStatus('Desconectado', Colors.grey);
            }
          case 'error':
            _append('[daemon] error: ${ev['detail']}');
            final detail = '${ev['detail']}'.toLowerCase();
            if (detail.contains('10061') ||
                detail.contains('refused') ||
                detail.contains('rechazada')) {
              _append('[ui] nadie escucha en el destino — usa "Compartir escritorio" '
                  'o abre `anyremoted listen` en otra terminal');
            }
            _setStatus('Error', Colors.red);
          default:
            _append('[daemon] $kind');
        }
        return;
      } catch (_) {
        // cae al log crudo
      }
    }
    _append('[daemon] $trimmed');
  }

  String get _daemonBin => resolveDaemonPath();

  /// C-2: envía el estado de los tres toggles al listener
  /// (`{"perm":{"input":bool,"audio":bool,"mic":bool}}`). El daemon lo aplica
  /// **en caliente durante la sesión** (sin reconectar) y, si aún no hay
  /// sesión, queda encolado y se aplica al abrirla.
  void _sendPerms() {
    final proc = _listener;
    if (proc == null) return;
    try {
      proc.stdin.writeln(jsonEncode({
        'perm': {
          'input': _acceptRemoteControl,
          'audio': _shareAudio,
          'mic': _shareMic,
        },
      }));
    } catch (e) {
      if (mounted) _append('[ui] no se pudieron enviar los permisos: $e');
    }
  }

  /// C-53: recuerda la decisión de [peer] y poda las entradas caducadas (la
  /// ventana es corta; sin poda el mapa crecería con cada invitado del día).
  void _rememberAdmission(String peer, String choice) {
    final now = DateTime.now();
    _admissionDecisions.removeWhere(
        (_, d) => now.difference(d.at) > kAdmissionDecisionWindow);
    _admissionDecisions[peer] = (choice: choice, at: now);
  }

  /// C-53: decisión vigente para [peer], o `null` si toca preguntar.
  String? _recentAdmission(String peer) {
    final d = _admissionDecisions[peer];
    if (d == null) return null;
    if (DateTime.now().difference(d.at) <= kAdmissionDecisionWindow) {
      return d.choice;
    }
    _admissionDecisions.remove(peer);
    return null;
  }

  /// C-53: escribe en el listener la decisión de [peer].
  ///
  /// Primera decisión: `admissionWrites` pone el `{"perm":{"input":true}}`
  /// **antes** de la línea de decisión (el daemon lo encola mientras espera).
  /// Reenvío de un intento ya decidido: solo la línea de decisión — los
  /// toggles ya están sincronizados con el daemon y repetir `input:true`
  /// reactivaría un control que el anfitrión apagara después de aceptar.
  void _writeAdmission({
    required String peer,
    required String choice,
    bool replay = false,
  }) {
    final proc = _listener;
    final lines = replay
        ? [admissionLine(peer: peer, choice: choice)]
        : admissionWrites(
            peer: peer,
            choice: choice,
            audio: _shareAudio,
            mic: _shareMic,
          );
    if (proc == null) {
      _append('[ui] decisión «$choice» para $peer no enviada: no hay listener '
          'local (el anfitrión la denegará por timeout)');
      return;
    }
    try {
      for (final line in lines) {
        proc.stdin.writeln(line);
      }
      final suffix = replay
          ? ' (reenvío de un intento ya decidido)'
          : choice == 'reject'
              ? ''
              : ' · con control';
      _append('[ui] decisión para $peer: $choice$suffix');
    } catch (e) {
      _append('[ui] no se pudo enviar la decisión: $e');
    }
  }

  /// C-44 (F-01): diálogo de **sesión entrante**. Escribe la decisión en el
  /// stdin del listener; el daemon (`--accept ask`) mantiene el handshake
  /// bloqueado hasta entonces y deniega por timeout si no hay respuesta.
  ///
  /// **C-53 (v0.14.5):** fuera la casilla «Permitir control remoto». Aceptar
  /// una sesión remota implica poder controlarla: `session_pending` es el
  /// diálogo de **admisión**, no de permisos. «Solo ver» sigue existiendo como
  /// estado explícito del anfitrión (toggle «Control remoto»), no como opción
  /// del diálogo. La decisión se recuerda por par (`kAdmissionDecisionWindow`)
  /// porque el cliente reintenta por varios transportes.
  Future<void> _promptIncoming(String peer) async {
    if (_admissionPromptOpen) {
      // C-53: un segundo evento mientras el diálogo está abierto se descarta
      // (no se apila otro diálogo sobre el actual).
      _append('[ui] ya hay una solicitud de sesión en pantalla: se ignora');
      return;
    }
    _admissionPromptOpen = true;
    var choice = 'reject';
    try {
      if (!mounted) return;
      final res = await showDialog<String>(
        context: context,
        barrierDismissible: false,
        builder: (ctx) => AlertDialog(
          title: const Text('Sesión entrante'),
          content: Text('El equipo $peer quiere conectarse a este equipo.\n\n'
              'Si aceptas, podrá ver la pantalla y controlarla (teclado y '
              'ratón). Puedes retirar el control después con «Control '
              'remoto».\n\n'
              '«Aceptar y confiar» la recordará para próximas sesiones.'),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx, 'reject'),
              child: const Text('Rechazar'),
            ),
            TextButton(
              onPressed: () => Navigator.pop(ctx, 'accept'),
              child: const Text('Aceptar una vez'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(ctx, 'trust'),
              child: const Text('Aceptar y confiar'),
            ),
          ],
        ),
      );
      if (res != null) choice = res;
    } finally {
      _admissionPromptOpen = false;
    }
    // C-53: sin ventana montada no se toca el estado (la app se está cerrando;
    // el listener muere con ella).
    if (!mounted) return;
    _rememberAdmission(peer, choice);
    if (choice != 'reject') {
      // C-53: aceptar = conceder control. El permiso se escribe justo después,
      // antes de la decisión (ver `_writeAdmission`).
      setState(() => _acceptRemoteControl = true);
    }
    _writeAdmission(peer: peer, choice: choice);
  }

  /// C-2/C-10: los tres toggles de permisos DINÁMICOS (una sola copia).
  /// Viven en la sección Conexión y — en modo visor, donde esa sección
  /// queda oculta (C-10) — en la cabecera del visor. En ambos lugares se
  /// aplican EN CALIENTE durante la sesión (D-07).
  List<Widget> _permToggles({double fontSize = 12}) => [
        Checkbox(
          value: _acceptRemoteControl,
          onChanged: (v) {
            setState(() => _acceptRemoteControl = v ?? false);
            _sendPerms();
          },
          visualDensity: VisualDensity.compact,
          materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        Text('Control remoto', style: TextStyle(fontSize: fontSize)),
        const SizedBox(width: 10),
        Checkbox(
          value: _shareAudio,
          onChanged: (v) {
            setState(() => _shareAudio = v ?? false);
            _sendPerms();
          },
          visualDensity: VisualDensity.compact,
          materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        Text('Audio del sistema', style: TextStyle(fontSize: fontSize)),
        const SizedBox(width: 10),
        Checkbox(
          value: _shareMic,
          onChanged: (v) {
            setState(() => _shareMic = v ?? false);
            _sendPerms();
          },
          visualDensity: VisualDensity.compact,
          materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        Text('Micrófono', style: TextStyle(fontSize: fontSize)),
      ];

  Future<void> _startLocalListen() async {
    if (_listener != null) {
      _append('[ui] el listener local ya corre');
      return;
    }
    // C-53: cada «Compartir escritorio» abre un contexto de consentimiento
    // nuevo; no se reutilizan las decisiones recordadas de la ventana anterior
    // (si no, un par aceptado antes entraría sin preguntar tras reiniciar).
    _admissionDecisions.clear();
    // C-43: con una sesión de cliente activa no se abre la escucha (mismo
    // conflicto que al revés): se pide detenerla.
    if (_daemon != null) {
      _append('[ui] hay una sesión activa — pulsa Detener antes de compartir');
      return;
    }
    // C-45 (F-02): compartir pantalla NO enciende el control remoto. El
    // toggle queda como lo dejó el usuario (apagado por defecto) y el daemon
    // arranca en «solo ver» hasta que se marque aquí o en el visor.
    if (!File(_daemonBin).existsSync()) {
      _append('[ui] anyremoted.exe no encontrado en $_daemonBin');
      _setStatus('Daemon no encontrado', Colors.orange);
      return;
    }
    try {
      final args = buildListenArgs(
        size: _qualities[_videoQuality] ?? '1280x720',
        kbps: _qualityKbps[_videoQuality] ?? 3000,
        udp: _useUdp,
        rdv: effectiveRdv(_rdv.text),
      );
      final proc = await Process.start(
        _daemonBin,
        args,
        mode: ProcessStartMode.normal,
      );
      setState(() => _listener = proc);
      _sendPerms(); // estado inicial de los toggles (se aplica al abrir sesión)
      _append('[ui] listener de red lanzado (0.0.0.0:21118 — ${_useUdp ? 'tcp+udp (QUIC)' : 'tcp'} — vídeo $_videoQuality@15 h264'
          ' — encuentro ${effectiveRdv(_rdv.text)}'
          ' — ${shareSummary(control: _acceptRemoteControl, audio: _shareAudio, mic: _shareMic)}'
          ')');
      _logToFile('--- listen args: ${args.join(' ')}');
      proc.stdout
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen((line) => _handleEvent(line, isListener: true));
      proc.stderr
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen((line) => _append('[listener-err] $line'));
      final code = await proc.exitCode;
      if (mounted) {
        setState(() => _listener = null);
        _append('[ui] listener finalizado (código $code)');
        if (_daemon == null) _setStatus('Desconectado', Colors.grey);
      }
    } catch (e) {
      _append('[ui] error al lanzar listener: $e');
      _setStatus('Error', Colors.red);
    }
  }

  Future<void> _stopLocalListen() async {
    final proc = _listener;
    if (proc == null) return;
    _append('[ui] deteniendo listener (bye ordenado)…');
    proc.stdin.writeln('bye');
    await proc.exitCode.timeout(const Duration(seconds: 3), onTimeout: () {
      proc.kill(ProcessSignal.sigkill);
      return -9;
    });
  }

  /// Muestra las IPs LAN desde las que otros dispositivos pueden conectarse.
  Future<void> _showLanIps() async {
    try {
      final ifs = await NetworkInterface.list(
        includeLoopback: false,
        type: InternetAddressType.IPv4,
      );
      final ips = ifs.expand((i) => i.addresses).map((a) => a.address).toList();
      if (!mounted) return;
      _append(ips.isEmpty
          ? '[ui] no se detectó ninguna IP LAN'
          : '[ui] desde otra máquina usa: ${ips.join("  |  ")}');
      _append('[ui] si falla la conexión, abre el puerto (admin): '
          'netsh advfirewall firewall add rule name="anyremote" dir=in action=allow protocol=TCP localport=21118');
      if (_useUdp) {
        _append('[ui] ...y para UDP (QUIC): '
            'netsh advfirewall firewall add rule name="anyremote-udp" dir=in action=allow protocol=UDP localport=21118');
      }
      _append('[ui] ...y para conexión por ID (descubrimiento): '
          'netsh advfirewall firewall add rule name="anyremote-discover" dir=in action=allow protocol=UDP localport=21119');
    } catch (e) {
      if (mounted) _append('[ui] no se pudieron listar las IPs: $e');
    }
  }

  // ---------- RF-02a: entrada en sesión (visor → stdin del daemon) ----------

  /// Envía una línea `{"input":{...}}` al daemon controlador. El controlado es
  /// quien valida el permiso real (D-07): aquí solo se traslada lo que el
  /// usuario hace en el visor.
  void _sendInput(Map<String, dynamic> spec) {
    final proc = _daemon;
    if (proc == null || !_sessionActive) return;
    try {
      // `IOSink.writeln` es síncrono: un pipe roto se notifica en `done`
      // (suscriptor único instalado al lanzar el daemon). El contador de
      // `entrada:` avanza con el ACUSE del anfitrión (`input_result`), no aquí.
      proc.stdin.writeln(jsonEncode({'input': spec}));
    } catch (e) {
      if (mounted) _append('[ui] entrada no enviada: $e');
    }
  }

  /// Punto del visor → coordenadas **normalizadas 0..65535** (ver
  /// `normalizedPointer`): funciona con cualquier calidad de vídeo.
  void _sendPointer(Offset local, Size box) {
    if (box.width <= 0 || box.height <= 0) return;
    final (nx, ny) = normalizedPointer(local, box);
    _sendInput({
      'pointer': {'x': nx, 'y': ny, 'absolute': true},
    });
  }

  /// Botón de ratón: 0=izquierdo, 1=derecho, 2=medio.
  void _sendButton(int button, bool down) {
    _sendInput({
      'button': {'button': button, 'down': down},
    });
  }

  /// Suelta el botón que estuviera mantenido en el visor (si lo había).
  void _releaseButton() {
    final b = _heldButton;
    if (b == null) return;
    _heldButton = null;
    _sendButton(b, false);
  }

  /// Rueda del ratón: una muesca por evento (el backend la multiplica a 120).
  void _sendScroll(double dy) {
    if (dy == 0) return;
    _sendInput({
      'button': {'wheel': dy > 0 ? 1 : -1},
    });
  }

  /// Teclado del visor → wire: código propio (`keyCodeFor`) o carácter (`ch`).
  KeyEventResult _onViewerKey(FocusNode node, KeyEvent event) {
    if (!_sessionActive) return KeyEventResult.ignored;
    final down = event is KeyDownEvent || event is KeyRepeatEvent;
    if (!down && event is! KeyUpEvent) return KeyEventResult.ignored;

    final key = event.logicalKey;
    final code = keyCodeFor(key);
    if (code != null) {
      // C-37: una dead key pendiente no debe perderse si la siguiente tecla
      // no es un carácter: se envía suelta antes (como haría el SO).
      if (down && _deadPending != null) {
        final d = _deadPending!;
        _deadPending = null;
        _sendInput({'key': {'code': 0, 'ch': d, 'down': true}});
        _sendInput({'key': {'code': 0, 'ch': d, 'down': false}});
      }
      _sendInput({
        'key': {'code': code, 'down': down},
      });
      return KeyEventResult.handled;
    }
    String? ch;
    if (down) {
      // El carácter producido respeta mayúsculas/dígitos; si no hay, su etiqueta.
      final produced =
          event is KeyDownEvent ? event.character : (event as KeyRepeatEvent).character;
      ch = (produced != null && produced.length == 1) ? produced : key.keyLabel;
      if (ch.length != 1) return KeyEventResult.ignored; // F5, Escape largo…
      // C-37: la dead key no se envía; se compone con la siguiente pulsación.
      if (isDeadAccent(ch)) {
        _deadPending = ch;
        return KeyEventResult.handled;
      }
      final dead = _deadPending;
      if (dead != null) {
        _deadPending = null;
        if (ch == dead) {
          // Doble pulsación (`´´`) → el acento suelto.
          ch = dead;
        } else {
          final (suelto, enviar) = resolveDeadKey(dead, ch);
          ch = enviar;
          if (suelto != null) {
            // No componen (`´x`): se envía el acento suelto y luego el carácter.
            _sendInput({'key': {'code': 0, 'ch': suelto, 'down': true}});
            _sendInput({'key': {'code': 0, 'ch': suelto, 'down': false}});
          }
        }
      }
      _heldChars[key.keyId] = ch;
    } else {
      ch = _heldChars.remove(key.keyId);
      if (ch == null) return KeyEventResult.ignored;
    }
    _sendInput({
      'key': {'code': 0, 'ch': ch, 'down': down},
    });
    return KeyEventResult.handled;
  }

  // ---------- Hito M2: visor remoto (preview local) ----------

  Future<void> _startViewer(int port) async {
    await _stopViewer();
    try {
      final sock = await Socket.connect(
        InternetAddress.loopbackIPv4,
        port,
        timeout: const Duration(seconds: 3),
      );
      if (!mounted) {
        sock.destroy();
        return;
      }
      setState(() {
        _preview = sock;
        _videoStatus.value = 'visor conectado (esperando frames)…';
      });
      _append('[ui] visor conectado al preview 127.0.0.1:$port');
      // C-28: tasa del visor cada5 s — si `frames` no crece entre líneas,
      // el render no está recibiendo (o no produciendo) frames.
      _fpsLogTimer?.cancel();
      _fpsLogTimer = Timer.periodic(const Duration(seconds: 5), (_) {
        if (!mounted || _preview == null) return;
        // C-41: si no llega ningún frame, la imagen congelada engaña. Se
        // avisa (una vez por episodio) y se pone el fps a 0.
        final ahora = DateTime.now().millisecondsSinceEpoch;
        final seg = stallSeconds(ahora, _lastFrameMs);
        if (seg != null && _sessionActive) {
          _renderFps = 0;
          _videoStatus.value = 'sin vídeo desde hace $seg s (¿red?)';
          if (!_stallLogged) {
            _stallLogged = true;
            _appendQuiet(
                '[ui] visor: sin frames desde hace $seg s — la sesión puede estar caída');
          }
        }
        final img = _frame.value;
        if (img != null) {
          final escala = _viewerBox.width / img.width * 100;
          _appendQuiet('[ui] visor: fps render=${_renderFps.toStringAsFixed(1)} · '
              '$_videoFrames frames · ${img.width}x${img.height} '
              '· caja=${_viewerBox.width.round()}x${_viewerBox.height.round()} '
              '· escala=${escala.toStringAsFixed(0)}% '
              '(${_viewerZoom == 0 ? 'ajustar' : '${(_viewerZoom * 100).round()}%'}) '
              '· recv=$_framesRecv render=$_framesRendered drop=$_framesDropped '
              'coste=${_renderCostMs.toStringAsFixed(1)}ms');
        } else {
          _appendQuiet(
              '[ui] visor: fps render=${_renderFps.toStringAsFixed(1)} · $_videoFrames frames');
        }
      });
      Timer(const Duration(seconds: 5), () {
        if (mounted && _frame.value == null && _preview != null) {
          _videoStatus.value =
              'sin frames: el escuchador debe iniciar con --video on';
        }
      });
      sock.listen(
        _onVideoBytes,
        onDone: () {
          // C-10: el preview terminó → se limpia el visor; sin esto
          // quedaría el último frame con el layout de modo visor «congelado».
          unawaited(_stopViewer());
          if (mounted && !_disposed) _videoStatus.value = 'visor finalizado';
        },
        onError: (Object e) {
          unawaited(_stopViewer());
          if (mounted && !_disposed) _videoStatus.value = 'visor con error: $e';
        },
        cancelOnError: true,
      );
    } catch (e) {
      _append('[ui] preview no disponible: $e');
    }
  }

  Future<void> _stopViewer() async {
    // C-23: dispose en marcha → `Socket.destroy()` dispara `onDone` (a veces
    // en el mismo hilo) y los notifiers están a punto de morir: no tocarlos.
    if (_disposed) return;
    _fpsLogTimer?.cancel();
    _fpsLogTimer = null;
    _preview?.destroy();
    _preview = null;
    _cursor.value = const _CursorState();
    _viewerBox = Size.zero;
    _assembler.reset();
    final img = _frame.value;
    _frame.value = null;
    img?.dispose();
    _videoFrames = 0;
    _framesRecv = 0;
    _framesRendered = 0;
    _framesDropped = 0;
    _renderCostMs = 0;
    _videoDecodeErrors = 0;
    _lastFrameMs = 0;
    _stallLogged = false;
    // C-19: al soltar el frame, la página reevalúa `viewerVisible` (antes
    // dependía de que cada llamante hiciera su propio `setState`). C-23:
    // nunca durante `dispose` (ver `_disposed`).
    if (mounted && !_disposed) setState(() {});
  }

  void _onVideoBytes(Uint8List chunk) {
    // C-19: el ensamblador amortiza el buffer (antes: recopiar el buffer
    // COMPLETO en cada chunk → cuadrático en bytes por frame) y devuelve
    // los frames completos con una sola copia de payload por frame.
    for (final f in _assembler.push(chunk)) {
      _framesRecv += 1;
      unawaited(_renderFrame(f.bgra, f.width, f.height));
    }
  }

  /// C-33: superficie interactiva del vídeo. `box` es el tamaño DIBUJADO del
  /// frame: la normalización del puntero y el overlay del cursor usan la
  /// misma referencia, también con zoom 1:1.
  Widget _viewerSurface(ui.Image? img, _CursorState cur, Size box) {
    _viewerBox = box;
    return Listener(
      // Ratón: cualquier movimiento (arrastrando o pasando por encima) →
      // cursor remoto. El botón se mantiene mientras el puntero esté
      // pulsado (arrastrar = down…moves…up).
      onPointerDown: (e) {
        _inputFocus.requestFocus();
        _sendPointer(e.localPosition, box);
        final b = e.buttons;
        if (b & kPrimaryMouseButton != 0) {
          _heldButton = 0;
        } else if (b & kSecondaryMouseButton != 0) {
          _heldButton = 1;
        } else if (b & kMiddleMouseButton != 0) {
          _heldButton = 2;
        }
        if (_heldButton != null) {
          _sendButton(_heldButton!, true);
        }
      },
      onPointerMove: (e) => _sendPointer(e.localPosition, box),
      onPointerUp: (_) => _releaseButton(),
      onPointerCancel: (_) => _releaseButton(),
      onPointerSignal: (sig) {
        if (sig is PointerScrollEvent) {
          // C-33: la rueda es del REMOTO también a 100 % (si no, haría
          // scroll de las barras locales); se reclama el evento para que el
          // `SingleChildScrollView` no lo consuma.
          GestureBinding.instance.pointerSignalResolver.register(sig, (event) {
            if (event is PointerScrollEvent) {
              _sendScroll(event.scrollDelta.dy);
            }
          });
        }
      },
      child: Focus(
        focusNode: _inputFocus,
        autofocus: true,
        onKeyEvent: _onViewerKey,
        // C-11: vídeo + overlay del cursor remoto. `StackFit.expand` conserva
        // las restricciones ajustadas de antes (RawImage rellena la caja).
        child: Stack(
          fit: StackFit.expand,
          children: [
            if (img != null)
              RawImage(
                image: img,
                fit: BoxFit.contain,
                // C-33: a 1:1 sin remuestreo (texto nítido); al ajustar,
                // filtrado de calidad para reducir sin aliasing.
                filterQuality: _viewerZoom == 1
                    ? FilterQuality.none
                    : FilterQuality.medium,
              )
            else
              // C-23: el placeholder vive ya DENTRO del Listener/Focus.
              const Center(
                child: Text('conectado… esperando primer frame',
                    style: TextStyle(color: Colors.grey)),
              ),
            if (img != null && cur.visible && cur.nx != null && cur.ny != null)
              Positioned(
                // Normalizado del anfitrión → caja de la imagen (0..65535).
                left: cur.nx! / 65535 * box.width,
                top: cur.ny! / 65535 * box.height,
                child: const CustomPaint(
                  key: Key('remote-cursor'),
                  size: Size(16, 24),
                  painter: _RemoteCursorPainter(),
                ),
              ),
          ],
        ),
      ),
    );
  }

  Future<void> _renderFrame(Uint8List bgra, int w, int h) async {
    if (_rendering) {
      // Bajo carga se descarta en vez de encolar (latencia); se CUENTA.
      _framesDropped += 1;
      return;
    }
    _rendering = true;
    final tRender = DateTime.now();
    try {
      final buf = await ui.ImmutableBuffer.fromUint8List(bgra);
      final desc = ui.ImageDescriptor.raw(
        buf,
        width: w,
        height: h,
        pixelFormat: ui.PixelFormat.bgra8888,
      );
      final codec = await desc.instantiateCodec();
      final frame = await codec.getNextFrame();
      final img = frame.image;
      // C-19: liberar los recursos del decode en cuanto se usan — antes
      // quedaban al GC (a 1080p son ~8 MB por frame de presión de GC).
      codec.dispose();
      desc.dispose();
      buf.dispose();
      if (!mounted) {
        img.dispose();
        return;
      }
      final prev = _frame.value;
      final now = DateTime.now().millisecondsSinceEpoch;
      if (_lastRenderMs > 0 && now > _lastRenderMs) {
        final inst = 1000.0 / (now - _lastRenderMs);
        _renderFps = _renderFps == 0 ? inst : 0.8 * _renderFps + 0.2 * inst;
      }
      _lastRenderMs = now;
      _videoFrames += 1;
      _framesRendered += 1;
      final cost = DateTime.now().difference(tRender).inMicroseconds / 1000.0;
      _renderCostMs = _renderCostMs == 0 ? cost : 0.8 * _renderCostMs + 0.2 * cost;
      _lastFrameMs = now;
      _stallLogged = false;
      // C-19: la cabecera se actualiza como máximo a 2 Hz (contador/fps no
      // necesitan más) y por SU notifier → no repinta la cabecera por frame.
      if (now - _lastStatusMs >= 500) {
        _lastStatusMs = now;
        _videoStatus.value =
            'vídeo ${w}x$h · $_videoFrames frames · ${_renderFps.toStringAsFixed(0)} fps render';
      }
      // C-19: solo escucha el visor (ListenableBuilder) → la página entera
      // ya no se reconstruye por frame (antes: `setState` completo ~30 veces/s).
      _frame.value = img;
      prev?.dispose();
      if (prev == null && mounted) {
        setState(() {}); // primer frame: `viewerVisible` pasa a true
      }
    } catch (e) {
      if (mounted) _videoStatus.value = 'render error: $e';
    } finally {
      _rendering = false;
    }
  }

  Future<void> _connect() async {
    final target = normalizeTarget(_target.text);
    if (target.isEmpty) {
      _append('[ui] indica un destino host:puerto o alias de perfil');
      return;
    }
    // C-16: un ID (10 chars Base32) se resuelve por descubrimiento LAN en el
    // daemon (consulta UDP + caché); NO es un alias de perfil (ver abajo).
    if (_daemon != null) {
      _append('[ui] ya hay una sesión en curso — pulsa Detener primero');
      return;
    }
    // C-43: si el escritorio se está compartiendo (listener local), al pulsar
    // Conectar se DEJA de compartir primero: el mismo equipo no debe ser
    // anfitrión y cliente a la vez (dos daemons compitiendo por el puerto y
    // por el vídeo). El usuario pidió este comportamiento explícitamente.
    if (_listener != null) {
      _append('[ui] dejando de compartir el escritorio antes de conectar…');
      await _stopLocalListen();
      if (!mounted) return;
    }
    if (!File(_daemonBin).existsSync()) {
      _append('[ui] anyremoted.exe no encontrado en $_daemonBin');
      _setStatus('Daemon no encontrado', Colors.orange);
      return;
    }
    // C-16: `looksLikeAlias` es «sin dos puntos» — un ID también lo cumple,
    // así que los IDs se excluyen explícitamente y viajan en mayúsculas
    // (el daemon valida la forma con NodeId::parse).
    final isId = RegExp(r'^[A-Z2-7]{10}$').hasMatch(target.toUpperCase());
    final effective = isId ? target.toUpperCase() : target;
    final asAlias = !isId && looksLikeAlias(target);
    final args = asAlias
        ? ['connect', '--profile', target, '--clipboard', 'win']
        : ['connect', effective, '--clipboard', 'win'];
    // RF audio: reproduce lo que el anfitrión comparte (si no comparte, no hay audio).
    args.addAll(['--audio', 'on']);
    // C-15: udp = QUIC con fallback automático a TCP si el par no lo habla.
    if (_useUdp) args.addAll(['--transport', 'udp']);
    // C-17/C-22: servidor de encuentro (resolución WAN + relay de respaldo)
    // — SIEMPRE presente: vacío → VPS por defecto. El daemon resuelve el ID
    // broadcast LAN → rdv → caché (auto-detección LAN/WAN).
    args.addAll(['--rdv', effectiveRdv(_rdv.text)]);
    _append('[ui] lanzando anyremoted ${asAlias ? 'connect --profile $target' : 'connect $target'}');
    try {
      final proc = await Process.start(
        _daemonBin,
        args,
        mode: ProcessStartMode.normal,
      );
      setState(() => _daemon = proc);
      _setStatus('Conectando', Colors.orange);
      // Un pipe roto (daemon fuera) llega por `done`: se traza, no revienta.
      proc.stdin.done.catchError((Object e) {
        if (mounted) _append('[ui] stdin del daemon cerrado: $e');
      });
      // C-12: línea de gestión inofensiva (el daemon la ignora: JSON sin
      // `input`) — registra que HAY gestor desde el primer instante: si la
      // app muere (cerrar ventana no ejecuta `dispose`), el daemon detecta el
      // EOF del dueño y cierra la sesión en lugar de quedar huérfano.
      proc.stdin.writeln('{"ui":"ready"}');

      _logToFile('--- connect args: ${args.join(' ')}');
      proc.stdout
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen(_handleEvent);
      proc.stderr
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen((line) => _append('[daemon-err] $line'));
      final code = await proc.exitCode;
      if (mounted) {
        setState(() => _daemon = null);
        _append('[ui] daemon finalizado (código $code)');
        if (_status != 'Error') _setStatus('Desconectado', Colors.grey);
      }
    } catch (e) {
      _append('[ui] error al lanzar daemon: $e');
      _setStatus('Error', Colors.red);
    }
  }

  Future<void> _stop() async {
    final proc = _daemon;
    if (proc == null) return;
    _append('[ui] deteniendo sesión (bye ordenado)…');
    proc.stdin.writeln('bye');
    final code = await proc.exitCode.timeout(const Duration(seconds: 3), onTimeout: () {
      proc.kill(ProcessSignal.sigkill);
      return -9;
    });
    if (mounted) {
      setState(() {
        _daemon = null;
        _sessionActive = false;
        _heldButton = null;
        _heldChars.clear();
      });
      _append('[ui] daemon detenido (código $code)');
      if (_listener != null) {
        _setStatus('Escuchando (red)', Colors.teal);
      } else {
        _setStatus('Desconectado', Colors.grey);
      }
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _fpsLogTimer?.cancel();
    _hScroll.dispose();
    _vScroll.dispose();
    _daemon?.kill(ProcessSignal.sigkill);
    _listener?.kill(ProcessSignal.sigkill);
    _preview?.destroy();
    final img = _frame.value;
    _frame.dispose();
    img?.dispose();
    _videoStatus.dispose();
    _cursor.dispose();
    _inputFocus.dispose();
    _target.dispose();
    _rdv.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final running = _daemon != null;
    // C-10: modo visor — con el visor visible, la sección Conexión y el
    // Registro quedan OCULTOS y «Detener» sube a la barra superior.
    final viewerVisible = _frame.value != null || _preview != null;
    return Scaffold(
      appBar: AppBar(
        title: Row(
          children: [
            const Icon(Icons.desktop_windows, size: 22),
            const SizedBox(width: 10),
            const Text('anyremote'),
            const SizedBox(width: 12),
            Chip(
              label: Text('v$kVersion', style: const TextStyle(fontSize: 12)),
              visualDensity: VisualDensity.compact,
            ),
          ],
        ),
        actions: [
          Padding(
            padding: const EdgeInsets.only(right: 16),
            child: Center(
              child: Row(
                children: [
                  // C-10: la sección Conexión está oculta en modo visor →
                  // «Detener» vive en la barra superior, justo a la izquierda
                  // del estado («Conectado» durante la sesión).
                  if (viewerVisible) ...[
                    OutlinedButton.icon(
                      onPressed: running ? _stop : null,
                      icon: const Icon(Icons.stop, size: 16),
                      label: const Text('Detener'),
                      style: OutlinedButton.styleFrom(
                          visualDensity: VisualDensity.compact),
                    ),
                    const SizedBox(width: 12),
                  ],
                  CircleAvatar(radius: 6, backgroundColor: _statusColor),
                  const SizedBox(width: 8),
                  Text(_status),
                ],
              ),
            ),
          ),
        ],
      ),
      body: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (!viewerVisible)
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const Text('Conexión', style: TextStyle(fontSize: 16, fontWeight: FontWeight.bold)),
                    const SizedBox(height: 12),
                    Row(
                      children: [
                        Expanded(
                          flex: 3,
                          child: TextField(
                            controller: _target,
                            enabled: !running,
                            decoration: const InputDecoration(
                              labelText: 'ID remoto o dirección (host:puerto)',
                              hintText: 'p. ej. 127.0.0.1:21118',
                              border: OutlineInputBorder(),
                              isDense: true,
                            ),
                            onSubmitted: (_) => _connect(),
                          ),
                        ),
                        const SizedBox(width: 12),
                        FilledButton.icon(
                          onPressed: running ? null : _connect,
                          icon: const Icon(Icons.link),
                          label: const Text('Conectar'),
                        ),
                        const SizedBox(width: 8),
                        OutlinedButton.icon(
                          onPressed: running ? _stop : null,
                          icon: const Icon(Icons.stop),
                          label: const Text('Detener'),
                        ),
                        const SizedBox(width: 8),
                        OutlinedButton.icon(
                          onPressed: _listener == null ? _startLocalListen : _stopLocalListen,
                          icon: Icon(_listener == null ? Icons.sensors : Icons.stop_circle_outlined),
                          label: Text(_listener == null ? 'Compartir escritorio' : 'Detener compartir'),
                        ),
                      ],
                    ),
                    const SizedBox(height: 6),
                    // C-9: el ID del equipo (identidad estable D-03) — visible y
                    // copiable. El ID del REMOTO aparece en la sesión/registro.
                    Row(
                      children: [
                        const Text('ID de este equipo:',
                            style: TextStyle(
                                fontSize: 12, fontWeight: FontWeight.w600)),
                        const SizedBox(width: 8),
                        SelectableText(
                          _localNodeId ?? '—',
                          style: const TextStyle(
                              fontSize: 13,
                              fontFamily: 'monospace',
                              letterSpacing: 1.2),
                        ),
                        const SizedBox(width: 4),
                        IconButton(
                          tooltip: 'Copiar ID',
                          icon: const Icon(Icons.copy, size: 15),
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(
                              minWidth: 28, minHeight: 28),
                          onPressed: _localNodeId == null
                              ? null
                              : () {
                                  Clipboard.setData(
                                      ClipboardData(text: _localNodeId!));
                                  _append('[ui] ID $_localNodeId copiado');
                                },
                        ),
                        const SizedBox(width: 10),
                        const Expanded(
                          child: Text(
                            'el ID del anfitrión aparece en la sesión como «SESIÓN ACTIVA con …»',
                            style:
                                TextStyle(fontSize: 11, color: Colors.grey),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 4),
                    // C-2: los tres toggles son DINÁMICOS — se aplican en
                    // caliente durante la sesión (el anfitrión manda sobre lo
                    // que comparte; D-07 en caliente). En modo visor (C-10)
                    // la MISMA fila vive en la cabecera del visor.
                    Row(children: _permToggles()),
                    const SizedBox(height: 10),
                    Row(
                      children: [
                        DropdownButton<String>(
                          hint: const Text('perfiles', style: TextStyle(fontSize: 13)),
                          value: _aliases.contains(_target.text) ? _target.text : null,
                          items: _aliases
                              .map((a) => DropdownMenuItem(value: a, child: Text(a)))
                              .toList(),
                          onChanged: running
                              ? null
                              : (v) {
                                  if (v != null) setState(() => _target.text = v);
                                },
                          isDense: true,
                        ),
                        const SizedBox(width: 8),
                        IconButton(
                          tooltip: 'Actualizar perfiles',
                          icon: const Icon(Icons.refresh, size: 18),
                          onPressed: _loadingProfiles ? null : _refreshProfiles,
                        ),
                        if (_aliases.isNotEmpty)
                          Text('${_aliases.length} perfil(es)',
                              style: const TextStyle(fontSize: 11, color: Colors.grey)),
                        const SizedBox(width: 14),
                        const Text('vídeo:',
                            style: TextStyle(fontSize: 12)),
                        const SizedBox(width: 4),
                        // Calidad: 720p por defecto (nativo resultó demasiado
                        // pesado para el anfitrión en la prueba de campo).
                        DropdownButton<String>(
                          value: _videoQuality,
                          items: _qualities.keys
                              .map((q) => DropdownMenuItem(
                                    value: q,
                                    child: Text(q, style: const TextStyle(fontSize: 12)),
                                  ))
                              .toList(),
                          onChanged: _listener != null
                              ? null
                              : (v) {
                                  if (v != null) {
                                    setState(() => _videoQuality = v);
                                  }
                                },
                          isDense: true,
                        ),
                        const SizedBox(width: 14),
                        // C-15: transporte UDP/QUIC — el vídeo va por un stream
                        // dedicado y no bloquea el control en enlaces malos.
                        Tooltip(
                          message: 'UDP (QUIC): el vídeo corre por un stream '
                              'dedicado — un enlace malo degrada el vídeo, no '
                              'el control. El anfitrión acepta TCP y UDP a la '
                              'vez y el cliente cae a TCP si el par no lo habla',
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Checkbox(
                                value: _useUdp,
                                onChanged: _listener != null || running
                                    ? null
                                    : (v) => setState(() => _useUdp = v ?? false),
                                visualDensity: VisualDensity.compact,
                                materialTapTargetSize:
                                    MaterialTapTargetSize.shrinkWrap,
                              ),
                              const Text('UDP (QUIC)',
                                  style: TextStyle(fontSize: 12)),
                            ],
                          ),
                        ),
                        const SizedBox(width: 10),
                        // C-17: servidor de encuentro (rendezvous + relay) —
                        // en la fila HORIZONTAL del toggle UDP: no suma ALTURA
                        // a la sección de conexión (evita el overflow
                        // vertical que fallaba en CI con sus métricas).
                        SizedBox(
                          width: 160,
                          child: TextField(
                            controller: _rdv,
                            enabled: !running,
                            style: const TextStyle(fontSize: 12),
                            decoration: const InputDecoration(
                              hintText: 'servidor de encuentro (vacío = VPS)',
                              hintStyle: TextStyle(fontSize: 11),
                              isDense: true,
                              border: OutlineInputBorder(),
                              contentPadding: EdgeInsets.symmetric(
                                  horizontal: 6, vertical: 6),
                            ),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 4),
                    Text(
                      'Daemon: ${resolveDaemonPath()}',
                      style: const TextStyle(fontSize: 11, color: Colors.grey),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 12),
            if (viewerVisible)
              // C-9: tarjeta FLEXIBLE — antes era rígida: la caja del visor
              // derivaba su ALTO del ancho de la ventana (AspectRatio) y
              // desbordaba el Column; al maximizar solo se veía la parte
              // superior de la pantalla del anfitrión.
              Expanded(
                flex: 3,
                child: Card(
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          const Icon(Icons.monitor, size: 18),
                          const SizedBox(width: 8),
                          const Text('Visor remoto',
                              style: TextStyle(fontSize: 16, fontWeight: FontWeight.bold)),
                          const Spacer(),
                          if (_audioHint.isNotEmpty)
                            Text(_audioHint,
                                style: TextStyle(
                                    fontSize: 12,
                                    color: _audioHint.endsWith('✓')
                                        ? Colors.teal
                                        : Colors.orange)),
                          if (_audioHint.isNotEmpty) const SizedBox(width: 10),
                          // C-45 (F-02): estado real del anfitrión. En modo
                          // visor los toggles están a la vista, pero un chip
                          // explícito evita creer que se está compartiendo
                          // control cuando la sesión es «solo ver».
                          if (_listener != null && !_acceptRemoteControl)
                            const Text('solo ver',
                                style: TextStyle(fontSize: 12, color: Colors.orange)),
                          if (_listener != null && !_acceptRemoteControl)
                            const SizedBox(width: 10),
                          if (_inputEvents > 0)
                            Text('entrada: $_inputEvents',
                                style: const TextStyle(fontSize: 12, color: Colors.teal)),
                          if (_inputEvents > 0) const SizedBox(width: 10),
                          // C-19: la cabecera escucha SU notifier: los
                          // cambios de estado no reconstruyen la página.
                          Flexible(
                            child: ValueListenableBuilder<String>(
                              valueListenable: _videoStatus,
                              builder: (_, status, _) => Text(status,
                                  overflow: TextOverflow.ellipsis,
                                  style: const TextStyle(
                                      fontSize: 12, color: Colors.grey)),
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 6),
                      // C-10: los toggles dinámicos (C-2) viajan a la cabecera
                      // del visor: en modo visor la sección Conexión queda
                      // oculta, pero los permisos siguen cambiando en caliente.
                      Row(
                        children: [
                          ..._permToggles(fontSize: 11),
                          const Spacer(),
                          // C-33: «Ajustar» reduce la pantalla completa a la
                          // ventana (texto pequeño si el anfitrión es 4K);
                          // «100 %» es 1:1 y se desplaza con las barras.
                          const Icon(Icons.zoom_in, size: 15),
                          const SizedBox(width: 4),
                          DropdownButton<double>(
                            value: _viewerZoom,
                            isDense: true,
                            underline: const SizedBox.shrink(),
                            style: const TextStyle(fontSize: 12),
                            items: const [
                              DropdownMenuItem(value: 0, child: Text('Ajustar')),
                              DropdownMenuItem(value: 1, child: Text('100 %')),
                              DropdownMenuItem(value: 1.5, child: Text('150 %')),
                            ],
                            onChanged: (v) =>
                                setState(() => _viewerZoom = v ?? 0),
                          ),
                        ],
                      ),
                      const SizedBox(height: 6),
                      // C-9: el marco reparte el alto restante de la tarjeta
                      // y se centra → la pantalla del anfitrión se ve COMPLETA
                      // a cualquier tamaño de ventana.
                      Expanded(
                        child: Center(
                          // C-19: un ÚNICO listener (frame + cursor) del visor:
                          // solo repinta ÉSTE subtree — la página entera ya no
                          // se reconstruye por frame ni ~20 Hz por el cursor.
                          child: ListenableBuilder(
                            listenable: _viewerPaint,
                            builder: (context, _) {
                              final img = _frame.value;
                              final cur = _cursor.value;
                              // C-23 (A): el input se captura SIEMPRE,
                              // también sin frame (si no, la pantalla del
                              // anfitrión no despierta y el vídeo no arranca).
                              if (img == null) {
                                return LayoutBuilder(
                                  builder: (context, c) =>
                                      _viewerSurface(null, cur, c.biggest),
                                );
                              }
                              // C-33: «ajustar» (por defecto) o 1:1. Con un
                              // anfitrión 4K al 125 % y un cliente 1080p, el
                              // texto llega a 8 px: sólo se lee a 100 %.
                              if (_viewerZoom <= 0) {
                                return AspectRatio(
                                  aspectRatio: img.width / img.height,
                                  child: LayoutBuilder(
                                    builder: (context, c) => _viewerSurface(
                                        img, cur, c.biggest),
                                  ),
                                );
                              }
                              final box = Size(
                                img.width * _viewerZoom,
                                img.height * _viewerZoom,
                              );
                              // C-34: las DOS barras van SOBRE el viewport.
                              // Antes la horizontal envolvía el contenido
                              // dentro del scroll vertical: quedaba pintada
                              // al final del frame (2.880 px abajo) y no se
                              // veía nunca.
                              return ClipRect(
                                child: Scrollbar(
                                  controller: _vScroll,
                                  thumbVisibility: true,
                                  thickness: 10,
                                  child: Scrollbar(
                                    controller: _hScroll,
                                    thumbVisibility: true,
                                    thickness: 10,
                                    // Solo las notificaciones del eje
                                    // horizontal (las verticales van arriba).
                                    notificationPredicate: (n) =>
                                        n.metrics.axis == Axis.horizontal,
                                    child: SingleChildScrollView(
                                      controller: _vScroll,
                                      child: SingleChildScrollView(
                                        controller: _hScroll,
                                        scrollDirection: Axis.horizontal,
                                        child: SizedBox(
                                          width: box.width,
                                          height: box.height,
                                          child: _viewerSurface(img, cur, box),
                                        ),
                                      ),
                                    ),
                                  ),
                                ),
                              );
                            },
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              ),
            if (!viewerVisible) const SizedBox(height: 12),
            if (!viewerVisible)
              Expanded(
                child: Card(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      const Text('Registro', style: TextStyle(fontSize: 16, fontWeight: FontWeight.bold)),
                      const SizedBox(height: 10),
                      Expanded(
                        child: ListView.builder(
                          // Invertido y anclado al final: la entrada más reciente
                          // queda visible abajo aunque el registro crezca.
                          reverse: true,
                          itemCount: _log.length,
                          itemBuilder: (_, i) => Text(
                            _log[_log.length - 1 - i],
                            style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            const SizedBox(height: 8),
            Text(
              'build $kGitRev · Flutter $kFlutterBuild · anyremote $kVersion (desktop windows-x64)',
              textAlign: TextAlign.center,
              style: const TextStyle(fontSize: 11, color: Colors.grey),
            ),
          ],
        ),
      ),
    );
  }
}
