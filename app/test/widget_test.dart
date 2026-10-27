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

  testWidgets('RF-02a: control remoto y audio desactivados por defecto (D-07)',
      (tester) async {
    await tester.pumpWidget(const AnyremoteApp());

    expect(find.text('Aceptar control remoto'), findsOneWidget);
    expect(find.text('Compartir audio'), findsOneWidget);
    expect(find.byType(Checkbox), findsNWidgets(2));
    final box = tester.widget<Checkbox>(find.byType(Checkbox).at(0));
    expect(box.value, isFalse, reason: 'D-07: sin --allow-input por defecto');
    final audio = tester.widget<Checkbox>(find.byType(Checkbox).at(1));
    expect(audio.value, isFalse, reason: 'D-07: sin --audio por defecto');

    await tester.tap(find.byType(Checkbox).at(0));
    await tester.pump();
    expect(tester.widget<Checkbox>(find.byType(Checkbox).at(0)).value, isTrue);

    await tester.tap(find.byType(Checkbox).at(1));
    await tester.pump();
    expect(tester.widget<Checkbox>(find.byType(Checkbox).at(1)).value, isTrue);

    await tester.tap(find.byType(Checkbox).at(0));
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
    expect(keyCodeFor(LogicalKeyboardKey.f5), isNull);
  });
}
