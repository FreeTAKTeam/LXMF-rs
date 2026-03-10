# SdkShutdownV2Params


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**mode** | **str** |  | 

## Example

```python
from lxmfclient.models.sdk_shutdown_v2_params import SdkShutdownV2Params

# TODO update the JSON string below
json = "{}"
# create an instance of SdkShutdownV2Params from a JSON string
sdk_shutdown_v2_params_instance = SdkShutdownV2Params.from_json(json)
# print the JSON string representation of the object
print(SdkShutdownV2Params.to_json())

# convert the object into a dict
sdk_shutdown_v2_params_dict = sdk_shutdown_v2_params_instance.to_dict()
# create an instance of SdkShutdownV2Params from a dict
sdk_shutdown_v2_params_from_dict = SdkShutdownV2Params.from_dict(sdk_shutdown_v2_params_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


