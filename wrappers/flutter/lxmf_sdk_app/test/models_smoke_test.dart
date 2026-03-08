import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';
import 'package:test/test.dart';

void main() {
  test('config from profile preserves event batch defaults', () {
    final config = Config.fromProfile(Profile.desktopDefault);
    expect(config.profile, Profile.desktopDefault);
    expect(config.eventBatchSize, 64);
    expect(config.transportMode, TransportMode.bleOnly);
  });

  test('config can express tcp client transport settings', () {
    const config = Config(
      profile: Profile.testingDefault,
      transportMode: TransportMode.tcpClient,
      tcpHost: '127.0.0.1',
      tcpPort: 4242,
    );

    expect(config.transportMode, TransportMode.tcpClient);
    expect(config.tcpHost, '127.0.0.1');
    expect(config.tcpPort, 4242);
  });

  test('app error formats stable machine code', () {
    const error = AppError(
      code: ErrorCode.deliveryQueuePressure,
      category: ErrorCategory.delivery,
      message: 'queue full',
      retryable: true,
      terminal: false,
    );

    expect(error.code.wireName, 'SDK_APP_DELIVERY_QUEUE_PRESSURE');
    expect(error.retryable, isTrue);
  });
}
