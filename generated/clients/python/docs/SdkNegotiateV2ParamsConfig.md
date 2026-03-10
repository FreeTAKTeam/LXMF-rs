# SdkNegotiateV2ParamsConfig


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**auth_mode** | **str** |  | 
**bind_mode** | **str** |  | 
**block_timeout_ms** | **int** |  | [optional] 
**overflow_policy** | **str** |  | 
**profile** | **str** |  | 
**rpc_backend** | **object** |  | [optional] 

## Example

```python
from lxmfclient.models.sdk_negotiate_v2_params_config import SdkNegotiateV2ParamsConfig

# TODO update the JSON string below
json = "{}"
# create an instance of SdkNegotiateV2ParamsConfig from a JSON string
sdk_negotiate_v2_params_config_instance = SdkNegotiateV2ParamsConfig.from_json(json)
# print the JSON string representation of the object
print(SdkNegotiateV2ParamsConfig.to_json())

# convert the object into a dict
sdk_negotiate_v2_params_config_dict = sdk_negotiate_v2_params_config_instance.to_dict()
# create an instance of SdkNegotiateV2ParamsConfig from a dict
sdk_negotiate_v2_params_config_from_dict = SdkNegotiateV2ParamsConfig.from_dict(sdk_negotiate_v2_params_config_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


