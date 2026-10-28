import 'package:anyremote_app/main.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show LogicalKeyboardKey;
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('shell integra daemon: versión, conexión y registro', (tester) async {
    await tester.pumpWidget(const AnyremoteApp());

    expect(find.text('anyremote'), findsOneWidget);
    expect(find.text('v$kVersion'), findsOneWidget);
    expect(find.byType(TextField), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'Conectar'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Detener'), findsOneWidget);
    expect(find.textContaining('ciclos S0..C8 completos'), findsOneWidget);

    // Detener inhabilitado sin sesión activa.
    final stop = tester.widget<OutlinedButton>(find.widgetWithText(OutlinedButton, 'Detener'));
    expect(stop.onPressed, isNull);

    // Sin destino → aviso en el registro.
    await tester.tap(find.widgetWithText(FilledButton, 'Conectar'));
    await tester.pump();
    expect(find.textContaining('indica un destino'), findsOneWidget);

    // Formato normalizado: coma → dos puntos (host,puerto).
    expect(normalizeTarget('127.0.0.1,21118'), '127.0.0.1:21118');
    expect(normalizeTarget('casa,127.0.0.1,21118'), '127.0.0.1:21118');
    expect(normalizeTarget('127.0.0.1:21118'), '127.0.0.1:21118');
    expect(looksLikeAlias('smoke'), isTrue);
    expect(looksLikeAlias('127.0.0.1:21118'), isFalse);

    // Con destino: en el entorno de test no hay anyremoted.exe junto al
    // binario → la app degrada con traza explícita (sin daemon real).
    await tester.enterText(find.byType(TextField), '127.0.0.1,21118');
    await tester.tap(find.widgetWithText(FilledButton, 'Conectar'));
    await tester.pump();
    expect(find.textContaining('anyremoted.exe no encontrado'), findsWidgets);
    expect(find.textContaining('Daemon no encontrado'), findsOneWidget);

    // Perfiles (C6): control de actualización presente; sin daemon → traza.
    expect(find.byTooltip('Actualizar perfiles'), findsOneWidget);
    await tester.tap(find.byTooltip('Actualizar perfiles'));
    await tester.pump();
    expect(find.textContaining('perfiles: anyremoted.exe no encontrado'), findsWidgets);

    // Botón de par local presente (listener autocontenido).
    expect(find.widgetWithText(OutlinedButton, 'Escuchar (red)'), findsOneWidget);
    await tester.tap(find.widgetWithText(OutlinedButton, 'Escuchar (red)'));
    await tester.pump();
    expect(find.textContaining('anyremoted.exe no encontrado'), findsWidgets);
  });

  testWidgets('C-2: tres toggles desactivados por defecto y dinámicos (D-07)',
      (tester) async {
    await tester.pumpWidget(const AnyremoteApp());

    // Tres opciones con etiquetas claras (C-2): control, audio del sistema y
    // micrófono (el usuario pedía distinguir "audio" de "micrófono").
    expect(find.text('Control remoto'), findsOneWidget);
    expect(find.text('Audio del sistema'), findsOneWidget);
    expect(find.text('Micrófono'), findsOneWidget);
    expect(find.byType(Checkbox), findsNWidgets(3));
    for (var i = 0; i < 3; i++) {
      expect(
        tester.widget<Checkbox>(find.byType(Checkbox).at(i)).value,
        isFalse,
        reason: 'D-07: todo desactivado por defecto',
      );
    }

    // Dinámicas: se activan y desactivan sin reiniciar (con listener activo el
    // cambio escribe {"perm":...} por stdin → se aplica en caliente).
    await tester.tap(find.byType(Checkbox).at(0)); // control remoto
    await tester.pump();
    expect(tester.widget<Checkbox>(find.byType(Checkbox).at(0)).value, isTrue);

    await tester.tap(find.byType(Checkbox).at(2)); // micrófono
    await tester.pump();
    expect(tester.widget<Checkbox>(find.byType(Checkbox).at(2)).value, isTrue);
    expect(
      tester.widget<Checkbox>(find.byType(Checkbox).at(1)).value,
      isFalse,
      reason: 'el audio del sistema sigue apagado',
    );

    await tester.tap(find.byType(Checkbox).at(0)); // se apaga de nuevo
    await tester.pump();
    expect(tester.widget<Checkbox>(find.byType(Checkbox).at(0)).value, isFalse);
  });

  test('keyCodeFor replica la tabla de teclas del wire ARSP (RF-02a)', () {
    // Códigos fijos (espejo de arsp.proto::Key).
    expect(keyCodeFor(LogicalKeyboardKey.escape), 1);
    expect(keyCodeFor(LogicalKeyboardKey.tab), 2);
    expect(keyCodeFor(LogicalKeyboardKey.enter), 3);
    expect(keyCodeFor(LogicalKeyboardKey.backspace), 4);
    expect(keyCodeFor(LogicalKeyboardKey.delete), 5);
    expect(keyCodeFor(LogicalKeyboardKey.arrowLeft), 6);
    expect(keyCodeFor(LogicalKeyboardKey.arrowRight), 7);
    expect(keyCodeFor(LogicalKeyboardKey.arrowUp), 8);
    expect(keyCodeFor(LogicalKeyboardKey.arrowDown), 9);
    expect(keyCodeFor(LogicalKeyboardKey.home), 10);
    expect(keyCodeFor(LogicalKeyboardKey.end), 11);
    expect(keyCodeFor(LogicalKeyboardKey.controlLeft), 20);
    expect(keyCodeFor(LogicalKeyboardKey.altRight), 21);
    expect(keyCodeFor(LogicalKeyboardKey.shiftLeft), 22);

    // Teclas de carácter y sin código propio → viajan como `ch`.
    expect(keyCodeFor(LogicalKeyboardKey.keyA), isNull);
    expect(keyCodeFor(LogicalKeyboardKey.digit1), isNull);
    expect(keyCodeFor(LogicalKeyboardKey.space), isNull);

    // Teclas de función F1..F12 → códigos 30..41 (menores RF-02a).
    expect(keyCodeFor(LogicalKeyboardKey.f1), 30);
    expect(keyCodeFor(LogicalKeyboardKey.f5), 34);
    expect(keyCodeFor(LogicalKeyboardKey.f12), 41);
    expect(keyCodeFor(LogicalKeyboardKey.f13), isNull);
    expect(keyCodeFor(LogicalKeyboardKey.printScreen), isNull);
  });

  test('buildListenArgs: el listener de la app es repetidor (v0.8.2)', () {
    // Regresión del reporte de campo: al cerrar la sesión desde el invitado el
    // anfitrión debe seguir aceptando reconexiones → la app pasa
    // `--max-sessions 0` (escucha ilimitada hasta «Detener listen»).
    final args = buildListenArgs(size: '1280x720', kbps: 3000);
    expect(args.first, 'listen');
    final i = args.indexOf('--max-sessions');
    expect(i, greaterThan(0), reason: 'la app debe pedir escucha repetidora');
    expect(args[i + 1], '0', reason: '0 = ilimitado: vuelve a aceptar tras cada sesión');

    // El resto de la configuración de campo no cambia.
    expect(args, contains('--allow-input'));
    expect(args, contains('--audio-source'));
    expect(args[args.indexOf('--video-size') + 1], '1280x720');
    expect(args[args.indexOf('--max-bitrate') + 1], '3000');
  });

  test('normalizedPointer mapea el visor a 0..65535 sin depender del vídeo', () {
    // Cualquier calidad de vídeo (720p, 1080p, nativa) produce el mismo
    // resultado para la misma posición relativa (fix v0.6.2: a 720p/1080p el
    // control remoto se perdía porque las coordenadas iban en píxeles de frame).
    const box = Size(640, 360);
    expect(normalizedPointer(Offset.zero, box), (0, 0));
    expect(normalizedPointer(const Offset(640, 360), box), (65535, 65535));

    final (cx, cy) = normalizedPointer(const Offset(320, 180), box);
    expect(cx, inInclusiveRange(32760, 32775));
    expect(cy, inInclusiveRange(32760, 32775));

    // Misma posición relativa en visores de otro tamaño → idéntico valor.
    final (small, _) = normalizedPointer(const Offset(160, 90), const Size(320, 180));
    final (big, _) = normalizedPointer(const Offset(640, 360), const Size(1280, 720));
    expect(small, big);

    // Arrastre fuera del visor → recortado a los bordes (sin crash).
    expect(normalizedPointer(const Offset(-50, 900), box), (0, 65535));
    // Caja degenerada → (0,0) sin división por cero.
    expect(normalizedPointer(Offset.zero, Size.zero), (0, 0));
  });
}
