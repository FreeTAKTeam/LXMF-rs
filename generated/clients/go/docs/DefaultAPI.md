# \DefaultAPI

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**Rpc**](DefaultAPI.md#Rpc) | **Post** /rpc | 



## Rpc

> RPCResponseUnion Rpc(ctx).RPCRequestUnion(rPCRequestUnion).Execute()



### Example

```go
package main

import (
	"context"
	"fmt"
	"os"
	openapiclient "github.com/GIT_USER_ID/GIT_REPO_ID"
)

func main() {
	rPCRequestUnion := openapiclient.RPCRequestUnion{SdkCancelMessageV2Request: openapiclient.NewSdkCancelMessageV2Request(openapiclient.rpcId{Int32: new(int32)}, "Method_example", *openapiclient.NewSdkCancelMessageV2Params("MessageId_example"))} // RPCRequestUnion | 

	configuration := openapiclient.NewConfiguration()
	apiClient := openapiclient.NewAPIClient(configuration)
	resp, r, err := apiClient.DefaultAPI.Rpc(context.Background()).RPCRequestUnion(rPCRequestUnion).Execute()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error when calling `DefaultAPI.Rpc``: %v\n", err)
		fmt.Fprintf(os.Stderr, "Full HTTP response: %v\n", r)
	}
	// response from `Rpc`: RPCResponseUnion
	fmt.Fprintf(os.Stdout, "Response from `DefaultAPI.Rpc`: %v\n", resp)
}
```

### Path Parameters



### Other Parameters

Other parameters are passed through a pointer to a apiRpcRequest struct via the builder pattern


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **rPCRequestUnion** | [**RPCRequestUnion**](RPCRequestUnion.md) |  | 

### Return type

[**RPCResponseUnion**](RPCResponseUnion.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models)
[[Back to README]](../README.md)

