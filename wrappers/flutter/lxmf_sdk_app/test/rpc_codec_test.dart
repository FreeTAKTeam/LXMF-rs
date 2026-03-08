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
}
