import 'package:anyremote_app/main.dart';
import 'package:flutter/material.dart';
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
}
