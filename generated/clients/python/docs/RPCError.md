# RPCError


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**error** | [**RPCErrorPayload**](RPCErrorPayload.md) |  | 
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 

## Example

```python
from lxmfclient.models.rpc_error import RPCError

# TODO update the JSON string below
json = "{}"
# create an instance of RPCError from a JSON string
rpc_error_instance = RPCError.from_json(json)
# print the JSON string representation of the object
print(RPCError.to_json())

# convert the object into a dict
rpc_error_dict = rpc_error_instance.to_dict()
# create an instance of RPCError from a dict
rpc_error_from_dict = RPCError.from_dict(rpc_error_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


