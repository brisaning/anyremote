import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

/// anyremote — shell de escritorio v0.1.0 con integración daemon (`anyremoted`).
///
/// Conexión real: la app spawnea `anyremoted connect <target>`, lee sus eventos
/// JSONL por stdout y refleja estado/registro en vivo. Ciclos S0–C8 en el backend.
void main() {
  runApp(const AnyremoteApp());
}

const String kVersion = String.fromEnvironment('APP_VERSION', defaultValue: '0.1.0');
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
  if (k == LogicalKeyboardKey.controlLeft || k == LogicalKeyboardKey.controlRight) return 20;
  if (k == LogicalKeyboardKey.altLeft || k == LogicalKeyboardKey.altRight) return 21;
  if (k == LogicalKeyboardKey.shiftLeft || k == LogicalKeyboardKey.shiftRight) return 22;
  return null;
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
  bool _sessionActive = false;
  bool _acceptRemoteControl = false;
  bool _shareAudio = false;
  /// Calidad del vídeo que comparte el listener. **720p por defecto**: en la
  /// prueba de campo de 2026-10-27 `native` solo alcanzó ~4.6 fps en el
  /// anfitrión y el enlace marcó RTT 57–782 ms (vídeo demasiado pesado).
  String _videoQuality = '720p';
  static const Map<String, String> _qualities = {
    '720p': '1280x720',
    '1080p': '1920x1080',
    'Nativa': 'native',
  };
  /// Botón mantenido en el visor (0/1/2) para emitir su `up` al soltar.
  int? _heldButton;
  /// Contador visible de eventos de entrada (enviados por el visor o
  /// inyectados/observados como anfitrión) — sin volcar cada uno al registro.
  int _inputEvents = 0;
  /// Indicador de audio en la cabecera del visor (`audio ✓` / `sin señal`).
  String _audioHint = '';

  // Hito M2: visor remoto (preview local del daemon controlador).
  Socket? _preview;
  Uint8List _videoBuf = Uint8List(0);
  ui.Image? _lastImage;
  int _videoFrames = 0;
  double _renderFps = 0;
  int _lastRenderMs = 0;
  bool _rendering = false;
  String _videoStatus = 'sin vídeo';
  final List<String> _log = [
    '[build] anyremote $kVersion · rev $kGitRev',
    '[build] Flutter $kFlutterBuild · windows-x64',
    '[core] ciclos S0..C8 completos · daemon anyremoted integrado',
  ];

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _refreshProfiles());
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
    setState(() => _log.add(line));
  }

  void _setStatus(String text, Color color) {
    setState(() {
      _status = text;
      _statusColor = color;
    });
  }

  void _handleEvent(String line) {
    final trimmed = line.trim();
    if (trimmed.isEmpty) return;
    if (trimmed.startsWith('{')) {
      try {
        final ev = jsonDecode(trimmed) as Map<String, dynamic>;
        final kind = ev['event'] as String? ?? 'unknown';
        switch (kind) {
          case 'listening':
            _append('[daemon] anyremoted v${ev['daemon'] ?? '?'} escuchando en ${ev['bind']} (node ${ev['node_id']})');
            if (_daemon == null) _setStatus('Escuchando (red)', Colors.teal);
            _showLanIps();
          case 'connecting':
            _append('[daemon] anyremoted v${ev['daemon'] ?? '?'} conectando a ${ev['target']}…');
            _setStatus('Conectando', Colors.orange);
          case 'session_active':
            _append('[daemon] SESIÓN ACTIVA con ${ev['peer']} (${ev['role']})');
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
            _startViewer((ev['port'] as num).toInt());
          case 'video_start':
            _append('[daemon] vídeo iniciando: ${ev['width']}x${ev['height']} @${ev['fps']} fps (${ev['codec']})');
          case 'video_stats':
            _videoStatus = 'vídeo ${ev['w']}x${ev['h']} · ${ev['frames']} frames';
            if (mounted) setState(() {});
          case 'video_adapt':
            _append('[daemon] calidad adaptada: ${ev['reason']} → ${ev['size']}${ev['bps'] != null ? ' @${ev['bps']} bps' : ''}');
          case 'file_resume':
            _append('[daemon] reanudando envío de ${ev['name']} desde offset ${ev['from_offset']}');
          case 'file_rejected':
            _append('[daemon] oferta rechazada: ${ev['detail']}');
          case 'input_injected':
            // RF-02a: no se loguea cada evento (inundaría el registro); solo el contador.
            setState(() => _inputEvents += 1);
          case 'input_denied':
            _append('[daemon] entrada denegada en el anfitrión: ${ev['allow_input'] == false ? 'sin --allow-input (D-07)' : 'permiso ${ev['perm'] ?? '?'} no concedido'}');
          case 'input_error':
            _append('[daemon] evento de entrada no soportado: ${ev['detail']}');
          case 'audio_start':
            _append(ev.containsKey('source')
                ? '[daemon] audio compartido desde ${ev['source']} (${ev['rate']} Hz)'
                : '[daemon] reproducción de audio activada (${ev['rate']} Hz)');
          case 'audio_denied':
            _append('[daemon] audio no concedido en esta sesión (D-07: el anfitrión debe compartirlo con "Compartir audio")');
          case 'audio_stats':
            // Diagnóstico en la cabecera del visor: rms≈ 0 = silencio (el
            // anfitrión no está reproduciendo nada o no está compartiendo).
            final rms = (ev['rms'] as num?)?.toDouble() ?? 0.0;
            final hint = rms > 0.002 ? 'audio ✓' : 'audio: sin señal';
            if (_audioHint != hint) {
              setState(() => _audioHint = hint);
              if (hint != 'audio ✓') {
                _append('[daemon] audio sin señal (rms 0): ¿está reproduciendo sonido el anfitrión?');
              }
            }
          case 'audio_error':
            _append('[daemon] audio desactivado en esta sesión: ${ev['detail']}');
          case 'closed':
            _append('[daemon] sesión cerrada: ${ev['reason']}');
            _setStatus('Desconectado', Colors.grey);
            _sessionActive = false;
            _heldButton = null;
            _heldChars.clear();
            _audioHint = '';
          case 'error':
            _append('[daemon] error: ${ev['detail']}');
            final detail = '${ev['detail']}'.toLowerCase();
            if (detail.contains('10061') ||
                detail.contains('refused') ||
                detail.contains('rechazada')) {
              _append('[ui] nadie escucha en el destino — usa "Escuchar (red)" '
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

  Future<void> _startLocalListen() async {
    if (_listener != null) {
      _append('[ui] el listener local ya corre');
      return;
    }
    if (!File(_daemonBin).existsSync()) {
      _append('[ui] anyremoted.exe no encontrado en $_daemonBin');
      _setStatus('Daemon no encontrado', Colors.orange);
      return;
    }
    try {
      final args = <String>[
        'listen',
        '--bind', '0.0.0.0:21118',
        '--clipboard', 'win',
        // M2: el escuchador comparte SU pantalla por defecto (sino no hay qué ver).
        '--video', 'on',
        '--video-size', _qualities[_videoQuality] ?? '1280x720',
        '--video-fps', '15',
        '--codec', 'vp9',
      ];
      // RF-02a (D-07): la entrada remota solo se acepta si el usuario la activa.
      if (_acceptRemoteControl) args.add('--allow-input');
      // RF audio (D-07): compartir micrófono solo si el usuario lo activa.
      if (_shareAudio) args.addAll(['--audio', 'on']);
      final proc = await Process.start(
        _daemonBin,
        args,
        mode: ProcessStartMode.normal,
      );
      setState(() => _listener = proc);
      final activos = <String>[
        if (_acceptRemoteControl) 'control remoto',
        if (_shareAudio) 'audio',
      ];
      _append('[ui] listener de red lanzado (0.0.0.0:21118 — vídeo $_videoQuality@15 vp9'
          '${activos.isEmpty
              ? ' — sin control remoto ni audio (D-07)'
              : ' — acepta ${activos.join(' + ')}'}'
          ')');
      proc.stdout
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen(_handleEvent);
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
      // (suscriptor único instalado al lanzar el daemon).
      proc.stdin.writeln(jsonEncode({'input': spec}));
      setState(() => _inputEvents += 1);
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
        _videoStatus = 'visor conectado (esperando frames)…';
      });
      _append('[ui] visor conectado al preview 127.0.0.1:$port');
      Timer(const Duration(seconds: 5), () {
        if (mounted && _lastImage == null && _preview != null) {
          setState(() => _videoStatus =
              'sin frames: el escuchador debe iniciar con --video on');
        }
      });
      sock.listen(
        _onVideoBytes,
        onDone: () {
          if (mounted) {
            setState(() => _preview = null);
            _videoStatus = 'visor finalizado';
          }
        },
        onError: (Object e) {
          if (mounted) {
            setState(() => _preview = null);
            _videoStatus = 'visor con error: $e';
          }
        },
        cancelOnError: true,
      );
    } catch (e) {
      _append('[ui] preview no disponible: $e');
    }
  }

  Future<void> _stopViewer() async {
    _preview?.destroy();
    _preview = null;
    _videoBuf = Uint8List(0);
    final img = _lastImage;
    _lastImage = null;
    img?.dispose();
    _videoFrames = 0;
  }

  void _onVideoBytes(Uint8List chunk) {
    if (chunk.isEmpty) return;
    if (_videoBuf.isEmpty) {
      _videoBuf = chunk;
    } else {
      final merged = Uint8List(_videoBuf.length + chunk.length);
      merged.setAll(0, _videoBuf);
      merged.setAll(_videoBuf.length, chunk);
      _videoBuf = merged;
    }
    while (_videoBuf.length >= 16) {
      final bd = ByteData.sublistView(_videoBuf);
      final w = bd.getUint32(0, Endian.little);
      final h = bd.getUint32(4, Endian.little);
      final len = bd.getUint32(12, Endian.little);
      if (w == 0 || h == 0 || w > 8192 || h > 8192 || len > 128 * 1024 * 1024) {
        _videoBuf = Uint8List(0); // pérdida de sincronía: resincronizar
        return;
      }
      if (_videoBuf.length < 16 + len) break;
      final payload = Uint8List.fromList(_videoBuf.sublist(16, 16 + len));
      _videoBuf = Uint8List.fromList(_videoBuf.sublist(16 + len));
      unawaited(_renderFrame(payload, w, h));
    }
  }

  Future<void> _renderFrame(Uint8List bgra, int w, int h) async {
    if (_rendering) return; // bajo carga: descarta en lugar de encolar
    _rendering = true;
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
      if (!mounted) {
        img.dispose();
        return;
      }
      final prev = _lastImage;
      final now = DateTime.now().millisecondsSinceEpoch;
      if (_lastRenderMs > 0 && now > _lastRenderMs) {
        final inst = 1000.0 / (now - _lastRenderMs);
        _renderFps = _renderFps == 0 ? inst : 0.8 * _renderFps + 0.2 * inst;
      }
      _lastRenderMs = now;
      setState(() {
        _lastImage = img;
        _videoFrames += 1;
        _videoStatus =
            'vídeo ${w}x$h · $_videoFrames frames · ${_renderFps.toStringAsFixed(0)} fps render';
      });
      prev?.dispose();
    } catch (e) {
      if (mounted) _videoStatus = 'render error: $e';
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
    if (_daemon != null) {
      _append('[ui] ya hay una sesión en curso — pulsa Detener primero');
      return;
    }
    if (!File(_daemonBin).existsSync()) {
      _append('[ui] anyremoted.exe no encontrado en $_daemonBin');
      _setStatus('Daemon no encontrado', Colors.orange);
      return;
    }
    final asAlias = looksLikeAlias(target);
    final args = asAlias
        ? ['connect', '--profile', target, '--clipboard', 'win']
        : ['connect', target, '--clipboard', 'win'];
    // RF audio: reproduce lo que el anfitrión comparte (si no comparte, no hay audio).
    args.addAll(['--audio', 'on']);
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
    _daemon?.kill(ProcessSignal.sigkill);
    _listener?.kill(ProcessSignal.sigkill);
    _preview?.destroy();
    _lastImage?.dispose();
    _inputFocus.dispose();
    _target.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final running = _daemon != null;
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
                          label: Text(_listener == null ? 'Escuchar (red)' : 'Detener listen'),
                        ),
                      ],
                    ),
                    const SizedBox(height: 4),
                    // D-07: sin marcar, el listener rechaza la entrada (RF-02a)
                    // y no comparte audio (RF audio).
                    Row(
                      children: [
                        Checkbox(
                          value: _acceptRemoteControl,
                          onChanged: _listener != null
                              ? null // no se cambia con el listener ya lanzado
                              : (v) => setState(() => _acceptRemoteControl = v ?? false),
                          visualDensity: VisualDensity.compact,
                          materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        ),
                        const Text('Aceptar control remoto',
                            style: TextStyle(fontSize: 12)),
                        const SizedBox(width: 10),
                        Checkbox(
                          value: _shareAudio,
                          onChanged: _listener != null
                              ? null // no se cambia con el listener ya lanzado
                              : (v) => setState(() => _shareAudio = v ?? false),
                          visualDensity: VisualDensity.compact,
                          materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        ),
                        const Text('Compartir audio',
                            style: TextStyle(fontSize: 12)),
                        const SizedBox(width: 10),
                        const Text('vídeo:',
                            style: TextStyle(fontSize: 12)),
                        const SizedBox(width: 4),
                        // Calidad:720p por defecto (nativo resultó demasiado
                        // pesado para el anfitrión en la prueba de campo).
                        DropdownButton<String>(
                          value: _videoQuality,
                          items: _qualities.keys
                              .map((q) => DropdownMenuItem(
                                    value: q,
                                    child:
                                        Text(q, style: const TextStyle(fontSize: 12)),
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
                      ],
                    ),
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
            if (_lastImage != null || _preview != null)
              Card(
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
                                    color: _audioHint == 'audio ✓'
                                        ? Colors.teal
                                        : Colors.orange)),
                          if (_audioHint.isNotEmpty) const SizedBox(width: 10),
                          if (_inputEvents > 0)
                            Text('entrada: $_inputEvents',
                                style: const TextStyle(fontSize: 12, color: Colors.teal)),
                          if (_inputEvents > 0) const SizedBox(width: 10),
                          Text(_videoStatus, style: const TextStyle(fontSize: 12, color: Colors.grey)),
                        ],
                      ),
                      const SizedBox(height: 8),
                      AspectRatio(
                        aspectRatio: _lastImage != null
                            ? _lastImage!.width / _lastImage!.height
                            : 16 / 9,
                        child: _lastImage != null
                            ? LayoutBuilder(
                                builder: (context, constraints) {
                                  final box = Size(
                                      constraints.maxWidth, constraints.maxHeight);
                                  final img = _lastImage!;
                                  return Listener(
                                    // Ratón: cualquier movimiento (arrastrando o
                                    // pasando por encima) → cursor remoto. El
                                    // botón se mantiene mientras el puntero esté
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
                                    onPointerMove: (e) =>
                                        _sendPointer(e.localPosition, box),
                                    onPointerUp: (_) => _releaseButton(),
                                    onPointerCancel: (_) => _releaseButton(),
                                    onPointerSignal: (sig) {
                                      if (sig is PointerScrollEvent) {
                                        _sendScroll(sig.scrollDelta.dy);
                                      }
                                    },
                                    child: Focus(
                                      focusNode: _inputFocus,
                                      autofocus: true,
                                      onKeyEvent: _onViewerKey,
                                      child:
                                          RawImage(image: img, fit: BoxFit.contain),
                                    ),
                                  );
                                },
                              )
                            : const Center(
                                child: Text('conectado… esperando primer frame',
                                    style: TextStyle(color: Colors.grey)),
                              ),
                      ),
                    ],
                  ),
                ),
              ),
            const SizedBox(height: 12),
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
