import 'package:anyremote_app/main.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('shell muestra versión, formulario de conexión y registro', (tester) async {
    await tester.pumpWidget(const AnyremoteApp());

    expect(find.text('anyremote'), findsOneWidget);
    expect(find.text('v$kVersion'), findsOneWidget);
    expect(find.byType(TextField), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'Conectar'), findsOneWidget);
    expect(find.textContaining('ciclos S0..C3-bis'), findsOneWidget);

    // Interacción: un intento de conexión deja traza en el registro (sin daemon).
    await tester.enterText(find.byType(TextField), '192.168.1.10:21118');
    await tester.tap(find.widgetWithText(FilledButton, 'Conectar'));
    await tester.pump();
    expect(find.textContaining('intento de conexión'), findsOneWidget);
    expect(find.textContaining('daemon no integrado'), findsOneWidget);
  });
}
