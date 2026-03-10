# SdkPollEventsV2Request


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**method** | **str** |  | 
**params** | [**SdkPollEventsV2Params**](SdkPollEventsV2Params.md) |  | 

## Example

```python
from lxmfclient.models.sdk_poll_events_v2_request import SdkPollEventsV2Request

# TODO update the JSON string below
json = "{}"
# create an instance of SdkPollEventsV2Request from a JSON string
sdk_poll_events_v2_request_instance = SdkPollEventsV2Request.from_json(json)
# print the JSON string representation of the object
print(SdkPollEventsV2Request.to_json())

# convert the object into a dict
sdk_poll_events_v2_request_dict = sdk_poll_events_v2_request_instance.to_dict()
# create an instance of SdkPollEventsV2Request from a dict
sdk_poll_events_v2_request_from_dict = SdkPollEventsV2Request.from_dict(sdk_poll_events_v2_request_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


