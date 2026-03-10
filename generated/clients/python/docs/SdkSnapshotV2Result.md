# SdkSnapshotV2Result


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**active_contract_version** | **int** |  | 
**config_revision** | **int** |  | 
**counts_included** | **bool** |  | 
**effective_capabilities** | **List[str]** |  | 
**event_stream_position** | **int** |  | 
**in_flight_messages** | **int** |  | 
**profile** | **str** |  | 
**queued_messages** | **int** |  | 
**runtime_id** | **str** |  | 
**state** | **str** |  | 

## Example

```python
from lxmfclient.models.sdk_snapshot_v2_result import SdkSnapshotV2Result

# TODO update the JSON string below
json = "{}"
# create an instance of SdkSnapshotV2Result from a JSON string
sdk_snapshot_v2_result_instance = SdkSnapshotV2Result.from_json(json)
# print the JSON string representation of the object
print(SdkSnapshotV2Result.to_json())

# convert the object into a dict
sdk_snapshot_v2_result_dict = sdk_snapshot_v2_result_instance.to_dict()
# create an instance of SdkSnapshotV2Result from a dict
sdk_snapshot_v2_result_from_dict = SdkSnapshotV2Result.from_dict(sdk_snapshot_v2_result_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


