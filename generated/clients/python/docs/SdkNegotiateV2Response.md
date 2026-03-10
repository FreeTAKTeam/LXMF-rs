# SdkNegotiateV2Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**result** | [**SdkNegotiateV2Result**](SdkNegotiateV2Result.md) |  | 

## Example

```python
from lxmfclient.models.sdk_negotiate_v2_response import SdkNegotiateV2Response

# TODO update the JSON string below
json = "{}"
# create an instance of SdkNegotiateV2Response from a JSON string
sdk_negotiate_v2_response_instance = SdkNegotiateV2Response.from_json(json)
# print the JSON string representation of the object
print(SdkNegotiateV2Response.to_json())

# convert the object into a dict
sdk_negotiate_v2_response_dict = sdk_negotiate_v2_response_instance.to_dict()
# create an instance of SdkNegotiateV2Response from a dict
sdk_negotiate_v2_response_from_dict = SdkNegotiateV2Response.from_dict(sdk_negotiate_v2_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


