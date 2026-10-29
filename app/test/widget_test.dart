import 'dart:ui' as ui;

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

  testWidgets('C-9: el visor remoto cabe en la ventana (sin desbordes)',
      (tester) async {
    await tester.pumpWidget(const AnyremoteApp());
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    // Frame de anfitrión 1280x720 inyectado (misma ruta de render que el
    // preview real: la tarjeta del visor pasa a estar visible).
    final rec = ui.PictureRecorder();
    final canvas = Canvas(rec);
    canvas.drawRect(const Rect.fromLTWH(0, 0, 1280, 720),
        Paint()..color = const Color(0xFF102030));
    final frame = rec.endRecording().toImageSync(1280, 720);

    // 1) Ventana pequeña (800x600, tamaño por defecto del test).
    final st = tester.state(find.byType(HomePage)) as dynamic;
    st.debugShowViewerFrame(frame);
    await tester.pump();
    expect(tester.takeException(), isNull,
        reason: 'el frame del anfitrión debe caber sin desbordar el Column');
    expect(find.text('Visor remoto'), findsOneWidget);
    expect(find.text('Registro'), findsNothing,
        reason: 'C-10: en modo visor el Registro queda oculto');

    // 2) Maximizada (1600x900): la caja del visor crece con el ancho SIN
    //    desbordar — regresión del reporte: antes solo se veía la parte
    //    superior de la pantalla del anfitrión.
    tester.view.physicalSize = const Size(1600, 900);
    tester.view.devicePixelRatio = 1.0;
    await tester.pump();
    expect(tester.takeException(), isNull,
        reason: 'sin desbordes al maximizar la ventana');
    expect(find.text('Visor remoto'), findsOneWidget);
    expect(find.text('Registro'), findsNothing,
        reason: 'C-10: en modo visor el Registro queda oculto');
  });

  testWidgets('C-9: ID de este equipo visible y copiable', (tester) async {
    await tester.pumpWidget(const AnyremoteApp());
    final st = tester.state(find.byType(HomePage)) as dynamic;
    st.debugSetLocalNodeId('ONDH6IU4RU');
    await tester.pump();
    expect(find.text('ID de este equipo:'), findsOneWidget);
    expect(find.text('ONDH6IU4RU'), findsOneWidget);
    expect(find.byTooltip('Copiar ID'), findsOneWidget);
  });

  testWidgets(
      'C-10: modo visor oculta Conexión/Registro y sube Detener a la barra',
      (tester) async {
    await tester.pumpWidget(const AnyremoteApp());
    final st = tester.state(find.byType(HomePage)) as dynamic;

    // Sin visor: layout normal (Conexión + Registro + Detener dentro de la
    // sección Conexión, y los toggles en esa sección).
    expect(find.text('Conexión'), findsOneWidget);
    expect(find.text('Registro'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Detener'), findsOneWidget);
    expect(find.byType(Checkbox), findsNWidgets(3));

    // Con visor visible (frame inyectado = sesión con vídeo en marcha).
    final rec = ui.PictureRecorder();
    final canvas = Canvas(rec);
    canvas.drawRect(const Rect.fromLTWH(0, 0, 1280, 720),
        Paint()..color = const Color(0xFF102030));
    st.debugShowViewerFrame(rec.endRecording().toImageSync(1280, 720));
    await tester.pump();

    // 1) Las secciones pedidas quedan OCULTAS.
    expect(find.text('Conexión'), findsNothing, reason: 'C-10: modo visor');
    expect(find.text('Registro'), findsNothing, reason: 'C-10: modo visor');

    // 2) Un ÚNICO botón Detener y en la barra superior, a la IZQUIERDA del
    //    estado (posición real, no solo presencia).
    final btn = find.widgetWithText(OutlinedButton, 'Detener');
    expect(btn, findsOneWidget);
    final btnRect = tester.getRect(btn);
    final statusRect = tester.getRect(find.text('Desconectado'));
    expect(
        btnRect.top,
        lessThan(tester.getRect(find.text('Visor remoto')).top),
        reason: 'el botón está en la barra superior, sobre el cuerpo');
    expect(btnRect.right, lessThanOrEqualTo(statusRect.left),
        reason: 'Detener a la izquierda del estado');

    // 3) Los toggles dinámicos siguen disponibles en la cabecera del visor.
    expect(find.byType(Checkbox), findsNWidgets(3));
    expect(find.text('Control remoto'), findsOneWidget);
    expect(find.text('Audio del sistema'), findsOneWidget);
    expect(find.text('Micrófono'), findsOneWidget);

    // 4) Al cerrar el visor vuelven las secciones (reconectar sin reiniciar).
    st.debugStopViewerForTest();
    await tester.pump();
    expect(find.text('Conexión'), findsOneWidget);
    expect(find.text('Registro'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Detener'), findsOneWidget);
    expect(find.text('Control remoto'), findsOneWidget);
  });

  testWidgets('C-11: overlay del cursor remoto sobre el vídeo', (tester) async {
    await tester.pumpWidget(const AnyremoteApp());
    final st = tester.state(find.byType(HomePage)) as dynamic;

    // Frame inyectado (modo visor con vídeo en marcha).
    final rec = ui.PictureRecorder();
    final canvas = Canvas(rec);
    canvas.drawRect(const Rect.fromLTWH(0, 0, 1280, 720),
        Paint()..color = const Color(0xFF102030));
    st.debugShowViewerFrame(rec.endRecording().toImageSync(1280, 720));
    await tester.pump();

    // Sin estado de cursor → sin overlay.
    expect(find.byKey(const Key('remote-cursor')), findsNothing);

    // Posición normalizada del anfitrión (nx≈0.75, ny≈0.25) → sobre la caja
    // de la imagen en la MISMA proporción (0..65535).
    st.debugSetRemoteCursor(49151, 16384, true);
    await tester.pump();
    final cur = find.byKey(const Key('remote-cursor'));
    expect(cur, findsOneWidget, reason: 'el overlay se dibuja al llegar el estado');
    final img = tester.getRect(find.byType(RawImage));
    final topLeft = tester.getTopLeft(cur);
    expect((topLeft.dx - img.left) / img.width, closeTo(49151 / 65535, 0.03));
    expect((topLeft.dy - img.top) / img.height, closeTo(16384 / 65535, 0.03));

    // Cursor oculto (CURSOR_SHOWING ausente) → desaparece.
    st.debugSetRemoteCursor(0, 0, false);
    await tester.pump();
    expect(find.byKey(const Key('remote-cursor')), findsNothing);
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
