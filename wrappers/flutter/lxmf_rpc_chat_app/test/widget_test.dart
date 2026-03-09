import 'package:flutter_test/flutter_test.dart';

import 'package:lxmf_rpc_chat_app/main.dart';

void main() {
  testWidgets('signal desk renders core operator surface', (WidgetTester tester) async {
    await tester.pumpWidget(const SignalDeskApp());

    expect(find.text('Signal Desk'), findsOneWidget);
    expect(find.text('Live Feed'), findsOneWidget);
    expect(find.text('RPC endpoint'), findsOneWidget);
    expect(find.text('Peer destination'), findsOneWidget);
    expect(find.text('Connect'), findsOneWidget);
    expect(find.text('Send'), findsOneWidget);
  });
}
