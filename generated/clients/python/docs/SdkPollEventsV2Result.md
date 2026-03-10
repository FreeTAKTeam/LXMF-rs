# SdkPollEventsV2Result


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**dropped_count** | **int** |  | 
**events** | **List[Dict[str, object]]** |  | 
**next_cursor** | **str** |  | 

## Example

```python
from lxmfclient.models.sdk_poll_events_v2_result import SdkPollEventsV2Result

# TODO update the JSON string below
json = "{}"
# create an instance of SdkPollEventsV2Result from a JSON string
sdk_poll_events_v2_result_instance = SdkPollEventsV2Result.from_json(json)
# print the JSON string representation of the object
print(SdkPollEventsV2Result.to_json())

# convert the object into a dict
sdk_poll_events_v2_result_dict = sdk_poll_events_v2_result_instance.to_dict()
# create an instance of SdkPollEventsV2Result from a dict
sdk_poll_events_v2_result_from_dict = SdkPollEventsV2Result.from_dict(sdk_poll_events_v2_result_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


