# SdkConfigureV2Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**result** | [**SdkConfigureV2Result**](SdkConfigureV2Result.md) |  | 

## Example

```python
from lxmfclient.models.sdk_configure_v2_response import SdkConfigureV2Response

# TODO update the JSON string below
json = "{}"
# create an instance of SdkConfigureV2Response from a JSON string
sdk_configure_v2_response_instance = SdkConfigureV2Response.from_json(json)
# print the JSON string representation of the object
print(SdkConfigureV2Response.to_json())

# convert the object into a dict
sdk_configure_v2_response_dict = sdk_configure_v2_response_instance.to_dict()
# create an instance of SdkConfigureV2Response from a dict
sdk_configure_v2_response_from_dict = SdkConfigureV2Response.from_dict(sdk_configure_v2_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


