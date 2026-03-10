# RPCResponseUnion


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**error** | [**RPCErrorPayload**](RPCErrorPayload.md) |  | [default to undefined]
**id** | [**RpcId**](RpcId.md) |  | [default to undefined]
**result** | [**SdkStatusV2Result**](SdkStatusV2Result.md) |  | [default to undefined]
**jsonrpc** | **string** |  | [optional] [default to undefined]

## Example

```typescript
import { RPCResponseUnion } from 'lxmfclient';

const instance: RPCResponseUnion = {
    error,
    id,
    result,
    jsonrpc,
};
```

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
