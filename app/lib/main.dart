import 'package:flutter/material.dart';

/// anyremote — shell de escritorio v0.1.0 (ciclo UI inicial).
///
/// Alcance honesto: UI de conexión/estado con versión visible. El enlace real
/// llega con la integración daemon (ARSP sobre `core/`; ciclos C7/C8).
/// Los ciclos S0–C3-bis se prueban hoy con los binarios Rust
/// (`cargo run -p anyremote-transport --example session_demo`).
void main() {
  runApp(const AnyremoteApp());
}

const String kVersion = String.fromEnvironment('APP_VERSION', defaultValue: '0.1.0');
const String kGitRev = String.fromEnvironment('GIT_REV', defaultValue: 'desconocido');
const String kFlutterBuild = String.fromEnvironment('FLUTTER_BUILD', defaultValue: 'unknown');

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
  final List<String> _log = [
    '[build] anyremote $kVersion · rev $kGitRev',
    '[build] Flutter $kFlutterBuild · windows-x64',
    '[core] ciclos S0..C3-bis: sesión ARSP, autenticación mutua, vídeo VP9 (37 fps @720p)',
    '[ui] shell v0 — enlace daemon pendiente (integración C7/C8)',
  ];

  void _connect() {
    final target = _target.text.trim();
    setState(() {
      _status = 'Enlace no disponible';
      _statusColor = Colors.orange;
      _log.add('[ui] intento de conexión a "${target.isEmpty ? '(vacío)' : target}" — daemon no integrado en v0.1.0');
    });
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text('v0.1.0 es UI shell: el enlace real llega con la integración daemon (ciclo C7).'),
        duration: Duration(seconds: 4),
      ),
    );
  }

  @override
  void dispose() {
    _target.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
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
                            decoration: const InputDecoration(
                              labelText: 'ID remoto o dirección (host:puerto)',
                              hintText: 'p. ej. 192.168.1.10:21118',
                              border: OutlineInputBorder(),
                              isDense: true,
                            ),
                            onSubmitted: (_) => _connect(),
                          ),
                        ),
                        const SizedBox(width: 12),
                        FilledButton.icon(
                          onPressed: _connect,
                          icon: const Icon(Icons.link),
                          label: const Text('Conectar'),
                        ),
                      ],
                    ),
                    const SizedBox(height: 10),
                    const Text(
                      'Mi identidad: se generará al integrar el daemon (ed25519 + ID Base32, D-03).',
                      style: TextStyle(fontSize: 12, color: Colors.grey),
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
