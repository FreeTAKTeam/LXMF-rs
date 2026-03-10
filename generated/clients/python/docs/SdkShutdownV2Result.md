# SdkShutdownV2Result


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**ack** | **str** |  | 

## Example

```python
from lxmfclient.models.sdk_shutdown_v2_result import SdkShutdownV2Result

# TODO update the JSON string below
json = "{}"
# create an instance of SdkShutdownV2Result from a JSON string
sdk_shutdown_v2_result_instance = SdkShutdownV2Result.from_json(json)
# print the JSON string representation of the object
print(SdkShutdownV2Result.to_json())

# convert the object into a dict
sdk_shutdown_v2_result_dict = sdk_shutdown_v2_result_instance.to_dict()
# create an instance of SdkShutdownV2Result from a dict
sdk_shutdown_v2_result_from_dict = SdkShutdownV2Result.from_dict(sdk_shutdown_v2_result_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


