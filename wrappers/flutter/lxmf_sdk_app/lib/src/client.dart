import 'models.dart';

abstract interface class AppBinding {
  Future<Handle> start(Config config);

  Future<void> stop();

  Future<RuntimeStatus> status();

  Future<SendReceipt> send(SendRequest request);

  Future<SendReport> sendWithProfileDefaults(SendRequest request);

  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  );

  Future<OperationRegistry> operationRegistry();

  Future<EnvelopeResponse> executeEnvelope(Envelope envelope);

  Stream<AppEvent> subscribeEvents();
}

class AppClient {
  AppClient(this._binding);

  final AppBinding _binding;

  Future<Handle> start(Config config) => _binding.start(config);

  Future<void> stop() => _binding.stop();

  Future<RuntimeStatus> status() => _binding.status();

  Future<SendReceipt> send(SendRequest request) => _binding.send(request);

  Future<SendReport> sendWithProfileDefaults(SendRequest request) {
    return _binding.sendWithProfileDefaults(request);
  }

  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  ) {
    return _binding.sendWithOptions(request, options);
  }

  Future<OperationRegistry> operationRegistry() => _binding.operationRegistry();

  Future<EnvelopeResponse> executeEnvelope(Envelope envelope) {
    return _binding.executeEnvelope(envelope);
  }

  Future<EnvelopeResponse> queryOperation(
    String operationId,
    Object? payload, {
    String? target,
    String? correlationId,
    int? timeoutMs,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) {
    return executeEnvelope(
      Envelope.query(operationId, payload).copyWith(
        target: target,
        correlationId: correlationId,
        timeoutMs: timeoutMs,
        extensions: extensions,
      ),
    );
  }

  Future<EnvelopeResponse> commandOperation(
    String operationId,
    Object? payload, {
    String? target,
    String? correlationId,
    int? timeoutMs,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) {
    return executeEnvelope(
      Envelope.command(operationId, payload).copyWith(
        target: target,
        correlationId: correlationId,
        timeoutMs: timeoutMs,
        extensions: extensions,
      ),
    );
  }

  Stream<AppEvent> subscribeEvents() => _binding.subscribeEvents();

  Future<List<IdentityBundle>> identityList() async {
    try {
      return await (_binding as dynamic).identityList() as List<IdentityBundle>;
    } on NoSuchMethodError {
      throw const AppError(
        code: ErrorCode.capabilityRequiredFeatureMissing,
        category: ErrorCategory.capability,
        message:
            'identityList is only available on bindings that expose identity helpers',
      );
    }
  }

  Future<ContactListPage> contactList({String? cursor, int? limit}) async {
    try {
      return await (_binding as dynamic)
          .contactList(cursor: cursor, limit: limit) as ContactListPage;
    } on NoSuchMethodError {
      throw const AppError(
        code: ErrorCode.capabilityRequiredFeatureMissing,
        category: ErrorCategory.capability,
        message:
            'contactList is only available on bindings that expose contact helpers',
      );
    }
  }

  Future<List<MessageRecord>> messageHistory() async {
    try {
      return await (_binding as dynamic).messageHistory()
          as List<MessageRecord>;
    } on NoSuchMethodError {
      throw const AppError(
        code: ErrorCode.capabilityRequiredFeatureMissing,
        category: ErrorCategory.capability,
        message:
            'messageHistory is only available on bindings that expose message helpers',
      );
    }
  }

  Future<DeliveryStatus?> deliveryStatus(String messageId) async {
    try {
      return await (_binding as dynamic).deliveryStatus(messageId)
          as DeliveryStatus?;
    } on NoSuchMethodError {
      throw const AppError(
        code: ErrorCode.capabilityRequiredFeatureMissing,
        category: ErrorCategory.capability,
        message:
            'deliveryStatus is only available on bindings that expose runtime status helpers',
      );
    }
  }

  Stream<DeliveryStatus> watchMessageStatus(String messageId) {
    try {
      return (_binding as dynamic).watchMessageStatus(messageId)
          as Stream<DeliveryStatus>;
    } on NoSuchMethodError {
      return Stream<DeliveryStatus>.error(
        const AppError(
          code: ErrorCode.capabilityRequiredFeatureMissing,
          category: ErrorCategory.capability,
          message:
              'watchMessageStatus is only available on bindings that expose runtime status helpers',
        ),
      );
    }
  }

  Future<RemoteCommandSession?> commandSession(String correlationId) async {
    try {
      return await (_binding as dynamic).commandSession(correlationId)
          as RemoteCommandSession?;
    } on NoSuchMethodError {
      throw const AppError(
        code: ErrorCode.capabilityRequiredFeatureMissing,
        category: ErrorCategory.capability,
        message:
            'commandSession is only available on bindings that expose remote command helpers',
      );
    }
  }

  Future<RemoteCommandSessionPage> commandSessions({
    String? cursor,
    int? limit,
  }) async {
    try {
      return await (_binding as dynamic).commandSessions(
          cursor: cursor, limit: limit) as RemoteCommandSessionPage;
    } on NoSuchMethodError {
      throw const AppError(
        code: ErrorCode.capabilityRequiredFeatureMissing,
        category: ErrorCategory.capability,
        message:
            'commandSessions is only available on bindings that expose remote command helpers',
      );
    }
  }

  Stream<RemoteCommandSession> watchCommand(String correlationId) {
    try {
      return (_binding as dynamic).watchCommand(correlationId)
          as Stream<RemoteCommandSession>;
    } on NoSuchMethodError {
      return Stream<RemoteCommandSession>.error(
        const AppError(
          code: ErrorCode.capabilityRequiredFeatureMissing,
          category: ErrorCategory.capability,
          message:
              'watchCommand is only available on bindings that expose remote command helpers',
        ),
      );
    }
  }
}
