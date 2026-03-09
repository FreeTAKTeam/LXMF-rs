import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    throw ArgumentError(
      'usage: dart run example/workspace_flows_smoke.dart <rpc-endpoint>',
    );
  }

  final workspace = WorkspaceClient.rpc(
    RpcConnectionOptions(endpoint: Uri.parse(args.first)),
  );

  await workspace.start(
    const Config(
      profile: Profile.desktopDefault,
      requestedCapabilities: <String>[
        'sdk.capability.identity_multi',
        'sdk.capability.identity_discovery',
        'sdk.capability.contact_management',
        'sdk.capability.topics',
        'sdk.capability.topic_fanout',
        'sdk.capability.markers',
        'sdk.capability.attachments',
      ],
    ),
  );

  final peer = await workspace.flows.ensurePeerReady('peer-demo');
  final topic = await workspace.flows.ensureTopic('ops/demo');
  final note = await workspace.flows.publishFieldNote(
    topicPath: 'ops/demo',
    payload: const <String, Object?>{'body': 'field note'},
  );
  final sync = await workspace.flows.ensureTopicSync('ops/demo');
  final report = await workspace.flows.publishAttachmentReport(
    topicPath: 'ops/reports',
    attachment: const AttachmentDraft(
      name: 'report.txt',
      contentType: 'text/plain',
      bytesBase64: 'cmVwb3J0',
    ),
    summaryPayload: const <String, Object?>{'title': 'demo report'},
  );
  final mission = await workspace.flows.sendMissionUpdate(
    const MissionUpdateDraft(
      peerIdentity: 'peer-demo',
      content: 'mission update',
      topicPath: 'ops/demo',
      attachments: <AttachmentDraft>[
        AttachmentDraft(
          name: 'sitrep.txt',
          contentType: 'text/plain',
          bytesBase64: 'c2l0cmVw',
        ),
      ],
      metadata: <String, Object?>{'priority': 'high'},
    ),
  );

  print(
    'peer=${peer.identity} created=${peer.wasCreated} '
    'topic=${topic.topic.topicId} note=${note.published} '
    'sync=${sync.subscribed}/${sync.telemetry.length} '
    'report=${report.attachment.attachmentId} '
    'mission=${mission.receipt.messageId}/${mission.attachments.length}',
  );

  await workspace.stop();
}
