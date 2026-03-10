# SdkCancelMessageV2Params


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**message_id** | **str** |  | 

## Example

```python
from lxmfclient.models.sdk_cancel_message_v2_params import SdkCancelMessageV2Params

# TODO update the JSON string below
json = "{}"
# create an instance of SdkCancelMessageV2Params from a JSON string
sdk_cancel_message_v2_params_instance = SdkCancelMessageV2Params.from_json(json)
# print the JSON string representation of the object
print(SdkCancelMessageV2Params.to_json())

# convert the object into a dict
sdk_cancel_message_v2_params_dict = sdk_cancel_message_v2_params_instance.to_dict()
# create an instance of SdkCancelMessageV2Params from a dict
sdk_cancel_message_v2_params_from_dict = SdkCancelMessageV2Params.from_dict(sdk_cancel_message_v2_params_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


