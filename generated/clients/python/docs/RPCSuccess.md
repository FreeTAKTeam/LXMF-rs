# RPCSuccess


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**result** | **Dict[str, object]** |  | 

## Example

```python
from lxmfclient.models.rpc_success import RPCSuccess

# TODO update the JSON string below
json = "{}"
# create an instance of RPCSuccess from a JSON string
rpc_success_instance = RPCSuccess.from_json(json)
# print the JSON string representation of the object
print(RPCSuccess.to_json())

# convert the object into a dict
rpc_success_dict = rpc_success_instance.to_dict()
# create an instance of RPCSuccess from a dict
rpc_success_from_dict = RPCSuccess.from_dict(rpc_success_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


