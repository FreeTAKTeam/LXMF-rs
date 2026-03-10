# lxmfclient.DefaultApi

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**rpc**](DefaultApi.md#rpc) | **POST** /rpc | 


# **rpc**
> RPCResponseUnion rpc(rpc_request_union)

### Example


```python
import lxmfclient
from lxmfclient.models.rpc_request_union import RPCRequestUnion
from lxmfclient.models.rpc_response_union import RPCResponseUnion
from lxmfclient.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to http://localhost
# See configuration.py for a list of all supported configuration parameters.
configuration = lxmfclient.Configuration(
    host = "http://localhost"
)


# Enter a context with an instance of the API client
with lxmfclient.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = lxmfclient.DefaultApi(api_client)
    rpc_request_union = lxmfclient.RPCRequestUnion() # RPCRequestUnion | 

    try:
        api_response = api_instance.rpc(rpc_request_union)
        print("The response of DefaultApi->rpc:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling DefaultApi->rpc: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **rpc_request_union** | [**RPCRequestUnion**](RPCRequestUnion.md)|  | 

### Return type

[**RPCResponseUnion**](RPCResponseUnion.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | RPC response |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

