# DefaultApi

All URIs are relative to *http://localhost*

|Method | HTTP request | Description|
|------------- | ------------- | -------------|
|[**rpc**](#rpc) | **POST** /rpc | |

# **rpc**
> RPCResponseUnion rpc(rPCRequestUnion)


### Example

```typescript
import {
    DefaultApi,
    Configuration,
    RPCRequestUnion
} from 'lxmfclient';

const configuration = new Configuration();
const apiInstance = new DefaultApi(configuration);

let rPCRequestUnion: RPCRequestUnion; //

const { status, data } = await apiInstance.rpc(
    rPCRequestUnion
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **rPCRequestUnion** | **RPCRequestUnion**|  | |


### Return type

**RPCResponseUnion**

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**200** | RPC response |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

