# SdkNegotiateV2Result


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**active_contract_version** | **int** |  | 
**contract_release** | **str** |  | 
**effective_capabilities** | **List[str]** |  | 
**effective_limits** | **Dict[str, object]** |  | 
**runtime_id** | **str** |  | 
**schema_namespace** | **str** |  | 

## Example

```python
from lxmfclient.models.sdk_negotiate_v2_result import SdkNegotiateV2Result

# TODO update the JSON string below
json = "{}"
# create an instance of SdkNegotiateV2Result from a JSON string
sdk_negotiate_v2_result_instance = SdkNegotiateV2Result.from_json(json)
# print the JSON string representation of the object
print(SdkNegotiateV2Result.to_json())

# convert the object into a dict
sdk_negotiate_v2_result_dict = sdk_negotiate_v2_result_instance.to_dict()
# create an instance of SdkNegotiateV2Result from a dict
sdk_negotiate_v2_result_from_dict = SdkNegotiateV2Result.from_dict(sdk_negotiate_v2_result_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


