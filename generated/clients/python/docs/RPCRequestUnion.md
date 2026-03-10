# RPCRequestUnion


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**method** | **str** |  | 
**params** | [**SdkStatusV2Params**](SdkStatusV2Params.md) |  | 

## Example

```python
from lxmfclient.models.rpc_request_union import RPCRequestUnion

# TODO update the JSON string below
json = "{}"
# create an instance of RPCRequestUnion from a JSON string
rpc_request_union_instance = RPCRequestUnion.from_json(json)
# print the JSON string representation of the object
print(RPCRequestUnion.to_json())

# convert the object into a dict
rpc_request_union_dict = rpc_request_union_instance.to_dict()
# create an instance of RPCRequestUnion from a dict
rpc_request_union_from_dict = RPCRequestUnion.from_dict(rpc_request_union_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


