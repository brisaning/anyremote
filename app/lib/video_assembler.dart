import 'dart:typed_data';

/// Frame BGRA ya ensamblado del stream de preview.
///
/// Cabecera de 16 bytes little-endian (la que escribe `core/daemon/src/preview.rs`):
/// (0) `w` u32, (4) `h` u32, (8) `seq` u32, (12) `len` u32, seguido del
/// payload BGRA de `len` bytes.
class AssembledFrame {
  const AssembledFrame(this.bgra, this.width, this.height);

  /// Payload BGRA (copia propia: el buffer del ensamblador sigue creciendo).
  final Uint8List bgra;
  final int width;
  final int height;
}

/// Ensamblador de frames del preview con **buffer amortizado** (C-19).
///
/// El consumidor anterior reconstruía el buffer COMPLETO en cada chunk de TCP
/// (`merged = old + chunk`), un coste **cuadrático** en bytes por frame: un
/// frame de 720p (≈3,7 MB) llega en ~200 chunks → ~200 copias de hasta
/// 3,7 MB cada uno (cientos de MB de `memcpy` por frame). Aquí:
///
/// - El buffer crece **por duplicación solo cuando hace falta** y se
///   **compacta reutilizando la capacidad** → coste amortizado por byte.
/// - Cada frame se extrae con **una sola copia** del payload.
/// - Cabecera inválida (resincronía) → se descarta TODO y se cuenta en
///   [dropped], igual que antes.
class VideoFrameAssembler {
  Uint8List _buf = Uint8List(0);
  int _start = 0; // inicio de los bytes válidos
  int _end = 0; // fin de los bytes válidos

  /// Resincronizaciones (cabecera inválida). Diagnóstico para tests/campo.
  int dropped = 0;

  /// Bytes retenidos a medio frame.
  int get buffered => _end - _start;

  /// Capacidad actual del buffer (para asserts de crecimiento amortizado).
  int get capacity => _buf.length;

  /// Descarta todo lo acumulado (cierre de visor / nueva sesión).
  void reset() {
    _buf = Uint8List(0);
    _start = 0;
    _end = 0;
  }

  /// Empuja un chunk del socket y devuelve los frames completos que ya
  /// contiene (0..n, en orden). Los bytes incompletos quedan retenidos.
  List<AssembledFrame> push(Uint8List chunk) {
    if (chunk.isEmpty) return const [];
    _ensure(chunk.length);
    _buf.setAll(_end, chunk);
    _end += chunk.length;

    List<AssembledFrame>? frames;
    while (_end - _start >= 16) {
      final bd = ByteData.sublistView(_buf, _start, _start + 16);
      final w = bd.getUint32(0, Endian.little);
      final h = bd.getUint32(4, Endian.little);
      final len = bd.getUint32(12, Endian.little);
      if (w == 0 || h == 0 || w > 8192 || h > 8192 || len > 128 * 1024 * 1024) {
        // Pérdida de sincronía: se descarta TODO (como antes) y se cuenta.
        _start = 0;
        _end = 0;
        dropped += 1;
        break;
      }
      if (_end - _start < 16 + len) break; // frame incompleto: esperar
      frames ??= <AssembledFrame>[];
      // Una sola copia del payload por frame (antes: ~200 por frame).
      frames.add(AssembledFrame(_buf.sublist(_start + 16, _start + 16 + len), w, h));
      _start += 16 + len;
    }
    return frames ?? const [];
  }

  /// Reserva espacio para `more` bytes:
  /// 1. si sobra cola → nada que hacer;
  /// 2. si cabe reutilizando → compacta los válidos al frente;
  /// 3. si no cabe → crece duplicando (amortizado) y copia al frente.
  void _ensure(int more) {
    final used = _end - _start;
    if (_buf.length - _end >= more) return; // hueco final suficiente
    if (used + more <= _buf.length) {
      if (_start > 0) {
        // `sublist` SIEMPRE copia → sin rangos solapados en `setAll`.
        final rem = _buf.sublist(_start, _end);
        _buf.setAll(0, rem);
        _start = 0;
        _end = used;
      }
      return;
    }
    var cap = _buf.length < 1024 ? 1024 : _buf.length;
    while (cap < used + more) {
      cap *= 2;
    }
    final grown = Uint8List(cap);
    grown.setRange(0, used, _buf, _start);
    _buf = grown;
    _start = 0;
    _end = used;
  }
}
