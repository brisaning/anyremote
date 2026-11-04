import 'dart:typed_data';

import 'package:anyremote_app/video_assembler.dart';
import 'package:flutter_test/flutter_test.dart';

/// Cabecera de16 bytes del preview (misma que escribe `core/daemon/src/preview.rs`).
Uint8List header(int w, int h, int len, {int seq = 0}) {
  final b = ByteData(16);
  b.setUint32(0, w, Endian.little);
  b.setUint32(4, h, Endian.little);
  b.setUint32(8, seq, Endian.little);
  b.setUint32(12, len, Endian.little);
  return b.buffer.asUint8List();
}

/// Frame completo (cabecera + payload de `len` bytes con el patrón `fill`).
Uint8List frame(int w, int h, {int seq = 0, int fill = 0x40}) {
  final len = w * h * 4;
  return Uint8List.fromList([
    ...header(w, h, len, seq: seq),
    ...List.filled(len, fill),
  ]);
}

void main() {
  group('VideoFrameAssembler (C-19)', () {
    test('frame completo en un solo chunk', () {
      final a = VideoFrameAssembler();
      final f = a.push(frame(4, 2, fill: 0x11));
      expect(f, hasLength(1));
      expect(f[0].width, 4);
      expect(f[0].height, 2);
      expect(f[0].bgra, hasLength(4 * 2 * 4));
      expect(f[0].bgra.every((b) => b == 0x11), isTrue);
      expect(a.buffered, 0);
      expect(a.dropped, 0);
    });

    test('frame partido en muchos chunks (multi-chunk TCP)', () {
      final a = VideoFrameAssembler();
      final bytes = frame(8, 8, fill: 0x7F); // 16 + 256 bytes
      final got = <AssembledFrame>[];
      // Cortes impares: 1 byte, resto de cabecera, payload en trozos raros.
      const cuts = [1, 16, 17, 40, 100, 200, 271, 272];
      var prev = 0;
      for (final c in cuts) {
        got.addAll(a.push(Uint8List.sublistView(bytes, prev, c)));
        prev = c;
      }
      expect(got, hasLength(1), reason: 'el frame se completa al último chunk');
      expect(got[0].width, 8);
      expect(got[0].bgra, Uint8List.sublistView(bytes, 16));
      expect(a.buffered, 0);
      expect(a.dropped, 0);
    });

    test('varios frames en un chunk se devuelven en orden', () {
      final a = VideoFrameAssembler();
      final one = frame(2, 2, seq: 1, fill: 0x01);
      final two = frame(2, 2, seq: 2, fill: 0x02);
      final three = frame(2, 2, seq: 3, fill: 0x03);
      final all = Uint8List.fromList([...one, ...two, ...three]);
      final got = a.push(all);
      expect(got, hasLength(3));
      expect(got[0].bgra.every((b) => b == 0x01), isTrue);
      expect(got[1].bgra.every((b) => b == 0x02), isTrue);
      expect(got[2].bgra.every((b) => b == 0x03), isTrue);
      expect(a.buffered, 0);
    });

    test('header incompleto se retiene hasta completarlo', () {
      final a = VideoFrameAssembler();
      final f = frame(2, 2);
      expect(a.push(Uint8List.sublistView(f, 0, 8)), isEmpty);
      expect(a.buffered, 8, reason: 'los8 primeros bytes quedan retenidos');
      final got = a.push(Uint8List.sublistView(f, 8));
      expect(got, hasLength(1));
      expect(a.buffered, 0);
    });

    test('basura → resincroniza y el frame siguiente se recupera', () {
      final a = VideoFrameAssembler();
      // Cabecera inválida (w > 8192) seguida en el MISMO chunk de un frame
      // válido: como antes, TODO se descarta (no hay forma fiable de saber
      // dónde empieza el siguiente frame sin sincronía).
      final bad = header(99999, 720, 4);
      final good = frame(2, 2, fill: 0x22);
      final got = a.push(Uint8List.fromList([...bad, ...good]));
      expect(got, isEmpty);
      expect(a.dropped, 1);
      expect(a.buffered, 0);
      // Un envío limpio posterior sí se ensambla.
      expect(a.push(good), hasLength(1));
    });

    test('crecimiento amortizado: la capacidad queda acotada', () {
      final a = VideoFrameAssembler();
      final f = frame(64, 64); // 16 400 bytes por frame
      // 200 frames en trozos de1024 bytes (≈3 280 pushes).
      for (var i = 0; i < 200; i++) {
        for (var off = 0; off < f.length; off += 1024) {
          final end = off + 1024 > f.length ? f.length : off + 1024;
          a.push(Uint8List.sublistView(f, off, end));
        }
      }
      expect(a.buffered, 0);
      // Antes: el buffer se recopiaba POR CHUNK (cuadrático). Ahora la
      // capacidad debe converger a ~unos pocos tamaños de frame, jamás
      // crecer con el número de chunks (1024 × 200 = 204 800 bytes ya
      // serían señal de regresión; el límite real está muy por debajo).
      expect(a.capacity, lessThan(16 * f.length),
          reason: 'la capacidad no debe crecer con el nº de chunks');
      expect(a.dropped, 0);
    });

    test('reset() descarta lo acumulado', () {
      final a = VideoFrameAssembler();
      final f = frame(2, 2);
      a.push(Uint8List.sublistView(f, 0, 20));
      expect(a.buffered, 20);
      a.reset();
      expect(a.buffered, 0);
      expect(a.capacity, 0);
      // Tras reset, un frame completo vuelve a ensamblarse.
      expect(a.push(f), hasLength(1));
    });

    test('chunks vacíos se ignoran', () {
      final a = VideoFrameAssembler();
      expect(a.push(Uint8List(0)), isEmpty);
      expect(a.buffered, 0);
    });
  });
}
