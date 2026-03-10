# SdkConfigureV2Result


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**ack** | **str** |  | 
**revision** | **int** |  | 

## Example

```python
from lxmfclient.models.sdk_configure_v2_result import SdkConfigureV2Result

# TODO update the JSON string below
json = "{}"
# create an instance of SdkConfigureV2Result from a JSON string
sdk_configure_v2_result_instance = SdkConfigureV2Result.from_json(json)
# print the JSON string representation of the object
print(SdkConfigureV2Result.to_json())

# convert the object into a dict
sdk_configure_v2_result_dict = sdk_configure_v2_result_instance.to_dict()
# create an instance of SdkConfigureV2Result from a dict
sdk_configure_v2_result_from_dict = SdkConfigureV2Result.from_dict(sdk_configure_v2_result_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


