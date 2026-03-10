# SdkSendV2Result


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**message_id** | **str** |  | 

## Example

```python
from lxmfclient.models.sdk_send_v2_result import SdkSendV2Result

# TODO update the JSON string below
json = "{}"
# create an instance of SdkSendV2Result from a JSON string
sdk_send_v2_result_instance = SdkSendV2Result.from_json(json)
# print the JSON string representation of the object
print(SdkSendV2Result.to_json())

# convert the object into a dict
sdk_send_v2_result_dict = sdk_send_v2_result_instance.to_dict()
# create an instance of SdkSendV2Result from a dict
sdk_send_v2_result_from_dict = SdkSendV2Result.from_dict(sdk_send_v2_result_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


