# SdkSendV2Params


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**content** | **str** |  | 
**destination** | **str** |  | 
**fields** | **Dict[str, object]** |  | [optional] 
**id** | **str** |  | 
**include_ticket** | **bool** |  | [optional] 
**method** | **str** |  | [optional] 
**source** | **str** |  | 
**stamp_cost** | **int** |  | [optional] 
**title** | **str** |  | [optional] 
**try_propagation_on_fail** | **bool** |  | [optional] 

## Example

```python
from lxmfclient.models.sdk_send_v2_params import SdkSendV2Params

# TODO update the JSON string below
json = "{}"
# create an instance of SdkSendV2Params from a JSON string
sdk_send_v2_params_instance = SdkSendV2Params.from_json(json)
# print the JSON string representation of the object
print(SdkSendV2Params.to_json())

# convert the object into a dict
sdk_send_v2_params_dict = sdk_send_v2_params_instance.to_dict()
# create an instance of SdkSendV2Params from a dict
sdk_send_v2_params_from_dict = SdkSendV2Params.from_dict(sdk_send_v2_params_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


