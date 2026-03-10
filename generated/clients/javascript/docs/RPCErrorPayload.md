# RPCErrorPayload


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**category** | **string** |  | [default to undefined]
**details** | [**{ [key: string]: ErrorJsonValue; }**](ErrorJsonValue.md) |  | [default to undefined]
**is_user_actionable** | **boolean** |  | [default to undefined]
**machine_code** | **string** |  | [default to undefined]
**message** | **string** |  | [default to undefined]
**retryable** | **boolean** |  | [default to undefined]
**cause_code** | **string** |  | [optional] [default to undefined]
**extensions** | **{ [key: string]: any; }** |  | [optional] [default to undefined]

## Example

```typescript
import { RPCErrorPayload } from 'lxmfclient';

const instance: RPCErrorPayload = {
    category,
    details,
    is_user_actionable,
    machine_code,
    message,
    retryable,
    cause_code,
    extensions,
};
```

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
