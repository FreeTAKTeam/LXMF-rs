# SdkPollEventsV2Params


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**cursor** | **str** |  | 
**max** | **int** |  | 

## Example

```python
from lxmfclient.models.sdk_poll_events_v2_params import SdkPollEventsV2Params

# TODO update the JSON string below
json = "{}"
# create an instance of SdkPollEventsV2Params from a JSON string
sdk_poll_events_v2_params_instance = SdkPollEventsV2Params.from_json(json)
# print the JSON string representation of the object
print(SdkPollEventsV2Params.to_json())

# convert the object into a dict
sdk_poll_events_v2_params_dict = sdk_poll_events_v2_params_instance.to_dict()
# create an instance of SdkPollEventsV2Params from a dict
sdk_poll_events_v2_params_from_dict = SdkPollEventsV2Params.from_dict(sdk_poll_events_v2_params_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


