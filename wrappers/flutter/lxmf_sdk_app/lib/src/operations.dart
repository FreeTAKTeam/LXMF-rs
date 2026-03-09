import 'client.dart';
import 'models.dart';

typedef OperationPayloadDecoder<T> = T Function(Object? payload);

class OperationCall<T> {
  const OperationCall({
    required this.operationId,
    required this.payload,
    required this.decode,
    this.target,
    this.correlationId,
    this.timeoutMs,
    this.extensions = const <String, Object?>{},
  });

  final String operationId;
  final Object? payload;
  final OperationPayloadDecoder<T> decode;
  final String? target;
  final String? correlationId;
  final int? timeoutMs;
  final Map<String, Object?> extensions;
}

class OperationResult<T> {
  const OperationResult({
    required this.operationId,
    required this.accepted,
    required this.payload,
    this.alias,
    this.correlationId,
    this.extensions = const <String, Object?>{},
  });

  final String operationId;
  final bool accepted;
  final T payload;
  final String? alias;
  final String? correlationId;
  final Map<String, Object?> extensions;
}

class OperationClient {
  OperationClient(this._appClient);

  final AppClient _appClient;
  Future<OperationRegistry>? _registryFuture;

  Future<OperationRegistry> registry() {
    return _registryFuture ??= _appClient.operationRegistry();
  }

  Future<ResolvedOperation> resolve(String operationId) async {
    final resolved = (await registry()).resolve(operationId);
    if (resolved == null) {
      throw AppError(
        code: ErrorCode.validationInvalidArgument,
        category: ErrorCategory.validation,
        message: 'unknown operation id: $operationId',
        userActionRequired: true,
      );
    }
    return resolved;
  }

  Future<OperationResult<T>> query<T>(OperationCall<T> call) async {
    final resolved = await resolve(call.operationId);
    if (!resolved.entry.acceptsEnvelopeKind(EnvelopeKind.query)) {
      throw AppError(
        code: ErrorCode.validationInvalidArgument,
        category: ErrorCategory.validation,
        message:
            'operation ${resolved.canonicalId} does not accept query envelopes',
        userActionRequired: true,
      );
    }
    final response = await _appClient.queryOperation(
      resolved.canonicalId,
      call.payload,
      target: call.target,
      correlationId: call.correlationId,
      timeoutMs: call.timeoutMs,
      extensions: call.extensions,
    );
    return OperationResult<T>(
      operationId: response.operationId,
      accepted: response.accepted,
      payload: call.decode(response.payload),
      alias: resolved.alias,
      correlationId: response.correlationId,
      extensions: response.extensions,
    );
  }

  Future<OperationResult<T>> command<T>(OperationCall<T> call) async {
    final resolved = await resolve(call.operationId);
    if (!resolved.entry.acceptsEnvelopeKind(EnvelopeKind.command)) {
      throw AppError(
        code: ErrorCode.validationInvalidArgument,
        category: ErrorCategory.validation,
        message:
            'operation ${resolved.canonicalId} does not accept command envelopes',
        userActionRequired: true,
      );
    }
    final response = await _appClient.commandOperation(
      resolved.canonicalId,
      call.payload,
      target: call.target,
      correlationId: call.correlationId,
      timeoutMs: call.timeoutMs,
      extensions: call.extensions,
    );
    return OperationResult<T>(
      operationId: response.operationId,
      accepted: response.accepted,
      payload: call.decode(response.payload),
      alias: resolved.alias,
      correlationId: response.correlationId,
      extensions: response.extensions,
    );
  }
}
