# SdkConfigureV2Params


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**expected_revision** | **int** |  | 
**patch** | **Dict[str, object]** |  | 

## Example

```python
from lxmfclient.models.sdk_configure_v2_params import SdkConfigureV2Params

# TODO update the JSON string below
json = "{}"
# create an instance of SdkConfigureV2Params from a JSON string
sdk_configure_v2_params_instance = SdkConfigureV2Params.from_json(json)
# print the JSON string representation of the object
print(SdkConfigureV2Params.to_json())

# convert the object into a dict
sdk_configure_v2_params_dict = sdk_configure_v2_params_instance.to_dict()
# create an instance of SdkConfigureV2Params from a dict
sdk_configure_v2_params_from_dict = SdkConfigureV2Params.from_dict(sdk_configure_v2_params_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


