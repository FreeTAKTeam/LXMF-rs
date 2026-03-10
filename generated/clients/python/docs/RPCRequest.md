# RPCRequest


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**method** | **str** |  | 
**params** | **Dict[str, object]** |  | 

## Example

```python
from lxmfclient.models.rpc_request import RPCRequest

# TODO update the JSON string below
json = "{}"
# create an instance of RPCRequest from a JSON string
rpc_request_instance = RPCRequest.from_json(json)
# print the JSON string representation of the object
print(RPCRequest.to_json())

# convert the object into a dict
rpc_request_dict = rpc_request_instance.to_dict()
# create an instance of RPCRequest from a dict
rpc_request_from_dict = RPCRequest.from_dict(rpc_request_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


