import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';

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
  final List<String> _log = [
    '[build] anyremote $kVersion · rev $kGitRev',
    '[build] Flutter $kFlutterBuild · windows-x64',
    '[core] ciclos S0..C8 completos · daemon anyremoted integrado',
  ];

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
            _append('[daemon] escuchando en ${ev['bind']} (node ${ev['node_id']})');
            if (_daemon == null) _setStatus('Escuchando (red)', Colors.teal);
            _showLanIps();
          case 'connecting':
            _append('[daemon] conectando a ${ev['target']}…');
            _setStatus('Conectando', Colors.orange);
          case 'session_active':
            _append('[daemon] SESIÓN ACTIVA con ${ev['peer']} (${ev['role']})');
            _setStatus('Conectado', Colors.green);
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
          case 'closed':
            _append('[daemon] sesión cerrada: ${ev['reason']}');
            _setStatus('Desconectado', Colors.grey);
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
      final proc = await Process.start(
        _daemonBin,
        ['listen', '--bind', '0.0.0.0:21118', '--clipboard', 'win'],
        mode: ProcessStartMode.normal,
      );
      setState(() => _listener = proc);
      _append('[ui] listener de red lanzado (0.0.0.0:21118 — todas las interfaces)');
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
    _append('[ui] deteniendo listener de red…');
    proc.kill(ProcessSignal.sigterm);
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
    _append('[ui] lanzando anyremoted ${asAlias ? 'connect --profile $target' : 'connect $target'}');
    try {
      final proc = await Process.start(
        _daemonBin,
        args,
        mode: ProcessStartMode.normal,
      );
      setState(() => _daemon = proc);
      _setStatus('Conectando', Colors.orange);

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
    _append('[ui] deteniendo daemon…');
    proc.kill(ProcessSignal.sigterm);
    final code = await proc.exitCode.timeout(const Duration(seconds: 5), onTimeout: () {
      proc.kill(ProcessSignal.sigkill);
      return -9;
    });
    if (mounted) {
      setState(() => _daemon = null);
      _append('[ui] daemon detenido (código $code)');
      if (_listener != null) {
        _setStatus('Escuchando (local)', Colors.teal);
      } else {
        _setStatus('Desconectado', Colors.grey);
      }
    }
  }

  @override
  void dispose() {
    _daemon?.kill(ProcessSignal.sigkill);
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
                    const SizedBox(height: 10),
                    Text(
                      'Daemon: ${resolveDaemonPath()}',
                      style: const TextStyle(fontSize: 11, color: Colors.grey),
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
                          itemCount: _log.length,
                          itemBuilder: (_, i) => Text(
                            _log[i],
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
