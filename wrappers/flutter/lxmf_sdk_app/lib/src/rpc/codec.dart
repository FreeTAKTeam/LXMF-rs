import 'dart:typed_data';

import 'package:msgpack_dart/msgpack_dart.dart' as msgpack;

Uint8List encodeRpcFrame(Map<String, Object?> value) {
  final bytes = Uint8List.fromList(msgpack.serialize(value));
  final frame = BytesBuilder(copy: false);
  final length = ByteData(4)..setUint32(0, bytes.length, Endian.big);
  frame.add(length.buffer.asUint8List());
  frame.add(bytes);
  return frame.toBytes();
}

Map<String, Object?> decodeRpcFrame(List<int> bytes) {
  if (bytes.length < 4) {
    throw const FormatException('missing frame header');
  }
  final view = ByteData.sublistView(Uint8List.fromList(bytes));
  final payloadLength = view.getUint32(0, Endian.big);
  if (bytes.length < 4 + payloadLength) {
    throw const FormatException('incomplete frame payload');
  }
  final payload = bytes.sublist(4, 4 + payloadLength);
  final decoded = _normalizeRpcFrame(msgpack.deserialize(Uint8List.fromList(payload)));
  if (decoded is! Map<String, Object?>) {
    throw const FormatException('rpc frame payload must decode to an object');
  }
  return decoded;
}

Object? _normalizeRpcFrame(Object? value) {
  if (value is List) {
    final tuple = _normalizeRpcTuple(value);
    if (tuple != null) {
      return tuple;
    }
  }
  return _normalize(value);
}

Object? _normalize(Object? value) {
  if (value == null ||
      value is String ||
      value is bool ||
      value is int ||
      value is double) {
    return value;
  }
  if (value is Uint8List) {
    return value;
  }
  if (value is List) {
    return value.map(_normalize).toList(growable: false);
  }
  if (value is Map) {
    final normalized = <String, Object?>{};
    value.forEach((key, nested) {
      normalized[key.toString()] = _normalize(nested);
    });
    return normalized;
  }
  return value.toString();
}

Map<String, Object?>? _normalizeRpcTuple(List<Object?> values) {
  if (values.length == 3) {
    final first = _normalize(values[0]);
    final second = _normalize(values[1]);
    final third = _normalize(values[2]);
    if ((first is int || first is String) && second is String) {
      return <String, Object?>{
        'id': first,
        'method': second,
        'params': third,
      };
    }
    if (first is int || first is String) {
      return <String, Object?>{
        'id': first,
        'result': second,
        'error': third,
      };
    }
  }
  if (values.length == 9) {
    return <String, Object?>{
      'code': _normalize(values[0]),
      'message': _normalize(values[1]),
      'machine_code': _normalize(values[2]),
      'category': _normalize(values[3]),
      'retryable': _normalize(values[4]),
      'is_user_actionable': _normalize(values[5]),
      'details': _normalize(values[6]),
      'cause_code': _normalize(values[7]),
      'extensions': _normalize(values[8]),
    };
  }
  return null;
}
