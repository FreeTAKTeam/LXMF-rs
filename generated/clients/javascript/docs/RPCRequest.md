# RPCRequest


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | [default to undefined]
**method** | **string** |  | [default to undefined]
**params** | **{ [key: string]: any; }** |  | [default to undefined]
**jsonrpc** | **string** |  | [optional] [default to undefined]

## Example

```typescript
import { RPCRequest } from 'lxmfclient';

const instance: RPCRequest = {
    id,
    method,
    params,
    jsonrpc,
};
```

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
