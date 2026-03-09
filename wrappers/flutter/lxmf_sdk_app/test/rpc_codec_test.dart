import 'package:lxmf_sdk_app/src/rpc/codec.dart';
import 'package:test/test.dart';

void main() {
  test('rpc codec prefixes payload length and roundtrips map payloads', () {
    final encoded = encodeRpcFrame(<String, Object?>{
      'id': 7,
      'method': 'sdk_snapshot_v2',
      'params': <String, Object?>{'include_counts': true},
    });

    expect(encoded.length, greaterThan(4));
    final decoded = decodeRpcFrame(encoded);
    expect(decoded['id'], 7);
    expect(decoded['method'], 'sdk_snapshot_v2');
    expect((decoded['params'] as Map<String, Object?>)['include_counts'], isTrue);
  });

  test('rpc codec rejects truncated frames', () {
    expect(() => decodeRpcFrame(<int>[0, 0, 0]), throwsFormatException);
    expect(
      () => decodeRpcFrame(<int>[0, 0, 0, 8, 1, 2, 3, 4]),
      throwsFormatException,
    );
  });

  test('rpc codec does not coerce arbitrary three-item lists into tuples', () {
    final encoded = encodeRpcFrame(<String, Object?>{
      'id': 9,
      'result': <String, Object?>{
        'messages': <Object?>[
          <String, Object?>{'id': 'm1', 'content': 'one'},
          <String, Object?>{'id': 'm2', 'content': 'two'},
          <String, Object?>{'id': 'm3', 'content': 'three'},
        ],
      },
      'error': null,
    });

    final decoded = decodeRpcFrame(encoded);
    final result = decoded['result']! as Map<String, Object?>;
    expect(result['messages'], isA<List<Object?>>());
    expect((result['messages']! as List<Object?>), hasLength(3));
  });

  test('rpc codec normalizes nested rpc error tuples', () {
    final encoded = encodeRpcFrame(<String, Object?>{
      'id': 11,
      'result': null,
      'error': <Object?>[
        'SDK_CAPABILITY_DISABLED',
        'feature disabled',
        'SDK_CAPABILITY_DISABLED',
        'Capability',
        false,
        true,
        <String, Object?>{},
        null,
        <String, Object?>{},
      ],
    });

    final decoded = decodeRpcFrame(encoded);
    expect(decoded['error'], isA<Map<String, Object?>>());
    final error = decoded['error']! as Map<String, Object?>;
    expect(error['machine_code'], 'SDK_CAPABILITY_DISABLED');
    expect(error['category'], 'Capability');
  });
}
