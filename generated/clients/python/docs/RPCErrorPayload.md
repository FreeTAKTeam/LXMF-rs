# RPCErrorPayload


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**category** | **str** |  | 
**cause_code** | **str** |  | [optional] 
**details** | [**Dict[str, ErrorJsonValue]**](ErrorJsonValue.md) |  | 
**extensions** | **Dict[str, object]** |  | [optional] 
**is_user_actionable** | **bool** |  | 
**machine_code** | **str** |  | 
**message** | **str** |  | 
**retryable** | **bool** |  | 

## Example

```python
from lxmfclient.models.rpc_error_payload import RPCErrorPayload

# TODO update the JSON string below
json = "{}"
# create an instance of RPCErrorPayload from a JSON string
rpc_error_payload_instance = RPCErrorPayload.from_json(json)
# print the JSON string representation of the object
print(RPCErrorPayload.to_json())

# convert the object into a dict
rpc_error_payload_dict = rpc_error_payload_instance.to_dict()
# create an instance of RPCErrorPayload from a dict
rpc_error_payload_from_dict = RPCErrorPayload.from_dict(rpc_error_payload_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


