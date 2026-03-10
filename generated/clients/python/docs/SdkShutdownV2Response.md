# SdkShutdownV2Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**result** | [**SdkShutdownV2Result**](SdkShutdownV2Result.md) |  | 

## Example

```python
from lxmfclient.models.sdk_shutdown_v2_response import SdkShutdownV2Response

# TODO update the JSON string below
json = "{}"
# create an instance of SdkShutdownV2Response from a JSON string
sdk_shutdown_v2_response_instance = SdkShutdownV2Response.from_json(json)
# print the JSON string representation of the object
print(SdkShutdownV2Response.to_json())

# convert the object into a dict
sdk_shutdown_v2_response_dict = sdk_shutdown_v2_response_instance.to_dict()
# create an instance of SdkShutdownV2Response from a dict
sdk_shutdown_v2_response_from_dict = SdkShutdownV2Response.from_dict(sdk_shutdown_v2_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


