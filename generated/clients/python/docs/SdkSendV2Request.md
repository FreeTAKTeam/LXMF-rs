# SdkSendV2Request


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**method** | **str** |  | 
**params** | [**SdkSendV2Params**](SdkSendV2Params.md) |  | 

## Example

```python
from lxmfclient.models.sdk_send_v2_request import SdkSendV2Request

# TODO update the JSON string below
json = "{}"
# create an instance of SdkSendV2Request from a JSON string
sdk_send_v2_request_instance = SdkSendV2Request.from_json(json)
# print the JSON string representation of the object
print(SdkSendV2Request.to_json())

# convert the object into a dict
sdk_send_v2_request_dict = sdk_send_v2_request_instance.to_dict()
# create an instance of SdkSendV2Request from a dict
sdk_send_v2_request_from_dict = SdkSendV2Request.from_dict(sdk_send_v2_request_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


