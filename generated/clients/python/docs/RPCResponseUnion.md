# RPCResponseUnion


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**error** | [**RPCErrorPayload**](RPCErrorPayload.md) |  | 
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**result** | [**SdkStatusV2Result**](SdkStatusV2Result.md) |  | 

## Example

```python
from lxmfclient.models.rpc_response_union import RPCResponseUnion

# TODO update the JSON string below
json = "{}"
# create an instance of RPCResponseUnion from a JSON string
rpc_response_union_instance = RPCResponseUnion.from_json(json)
# print the JSON string representation of the object
print(RPCResponseUnion.to_json())

# convert the object into a dict
rpc_response_union_dict = rpc_response_union_instance.to_dict()
# create an instance of RPCResponseUnion from a dict
rpc_response_union_from_dict = RPCResponseUnion.from_dict(rpc_response_union_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


