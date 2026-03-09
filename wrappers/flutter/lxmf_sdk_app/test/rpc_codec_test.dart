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

  test('rpc codec does not coerce arbitrary three-item nested lists into tuples', () {
    final encoded = encodeRpcFrame(<String, Object?>{
      'id': 9,
      'result': <String, Object?>{
        'events': <Object?>[
          <String, Object?>{'id': 'e1'},
          <String, Object?>{'id': 'e2'},
          <String, Object?>{'id': 'e3'},
        ],
      },
      'error': null,
    });

    final decoded = decodeRpcFrame(encoded);
    final result = decoded['result']! as Map<String, Object?>;
    expect(result['events'], isA<List<Object?>>());
    expect((result['events']! as List<Object?>), hasLength(3));
  });
}
