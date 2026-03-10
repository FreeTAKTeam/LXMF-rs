# SdkSnapshotV2Request


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | [**RpcId**](RpcId.md) |  | 
**jsonrpc** | **str** |  | [optional] 
**method** | **str** |  | 
**params** | [**SdkSnapshotV2Params**](SdkSnapshotV2Params.md) |  | 

## Example

```python
from lxmfclient.models.sdk_snapshot_v2_request import SdkSnapshotV2Request

# TODO update the JSON string below
json = "{}"
# create an instance of SdkSnapshotV2Request from a JSON string
sdk_snapshot_v2_request_instance = SdkSnapshotV2Request.from_json(json)
# print the JSON string representation of the object
print(SdkSnapshotV2Request.to_json())

# convert the object into a dict
sdk_snapshot_v2_request_dict = sdk_snapshot_v2_request_instance.to_dict()
# create an instance of SdkSnapshotV2Request from a dict
sdk_snapshot_v2_request_from_dict = SdkSnapshotV2Request.from_dict(sdk_snapshot_v2_request_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


