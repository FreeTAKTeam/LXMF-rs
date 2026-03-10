# SdkStatusV2Result


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**message** | **Dict[str, object]** |  | 

## Example

```python
from lxmfclient.models.sdk_status_v2_result import SdkStatusV2Result

# TODO update the JSON string below
json = "{}"
# create an instance of SdkStatusV2Result from a JSON string
sdk_status_v2_result_instance = SdkStatusV2Result.from_json(json)
# print the JSON string representation of the object
print(SdkStatusV2Result.to_json())

# convert the object into a dict
sdk_status_v2_result_dict = sdk_status_v2_result_instance.to_dict()
# create an instance of SdkStatusV2Result from a dict
sdk_status_v2_result_from_dict = SdkStatusV2Result.from_dict(sdk_status_v2_result_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


