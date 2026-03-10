# SdkSnapshotV2Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**result** | [**SdkSnapshotV2Result**](SdkSnapshotV2Result.md) |  | 

## Example

```python
from lxmfclient.models.sdk_snapshot_v2_response import SdkSnapshotV2Response

# TODO update the JSON string below
json = "{}"
# create an instance of SdkSnapshotV2Response from a JSON string
sdk_snapshot_v2_response_instance = SdkSnapshotV2Response.from_json(json)
# print the JSON string representation of the object
print(SdkSnapshotV2Response.to_json())

# convert the object into a dict
sdk_snapshot_v2_response_dict = sdk_snapshot_v2_response_instance.to_dict()
# create an instance of SdkSnapshotV2Response from a dict
sdk_snapshot_v2_response_from_dict = SdkSnapshotV2Response.from_dict(sdk_snapshot_v2_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


