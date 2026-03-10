# ErrorJsonValue


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------

## Example

```python
from lxmfclient.models.error_json_value import ErrorJsonValue

# TODO update the JSON string below
json = "{}"
# create an instance of ErrorJsonValue from a JSON string
error_json_value_instance = ErrorJsonValue.from_json(json)
# print the JSON string representation of the object
print(ErrorJsonValue.to_json())

# convert the object into a dict
error_json_value_dict = error_json_value_instance.to_dict()
# create an instance of ErrorJsonValue from a dict
error_json_value_from_dict = ErrorJsonValue.from_dict(error_json_value_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


