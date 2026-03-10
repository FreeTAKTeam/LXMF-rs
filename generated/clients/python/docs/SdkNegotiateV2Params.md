# SdkNegotiateV2Params


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**config** | [**SdkNegotiateV2ParamsConfig**](SdkNegotiateV2ParamsConfig.md) |  | 
**requested_capabilities** | **List[str]** |  | 
**supported_contract_versions** | **List[int]** |  | 

## Example

```python
from lxmfclient.models.sdk_negotiate_v2_params import SdkNegotiateV2Params

# TODO update the JSON string below
json = "{}"
# create an instance of SdkNegotiateV2Params from a JSON string
sdk_negotiate_v2_params_instance = SdkNegotiateV2Params.from_json(json)
# print the JSON string representation of the object
print(SdkNegotiateV2Params.to_json())

# convert the object into a dict
sdk_negotiate_v2_params_dict = sdk_negotiate_v2_params_instance.to_dict()
# create an instance of SdkNegotiateV2Params from a dict
sdk_negotiate_v2_params_from_dict = SdkNegotiateV2Params.from_dict(sdk_negotiate_v2_params_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


