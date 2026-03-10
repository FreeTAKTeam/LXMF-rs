# RPCResponseUnion

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Error** | [**RPCErrorPayload**](RPCErrorPayload.md) |  | 
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Result** | [**SdkStatusV2Result**](SdkStatusV2Result.md) |  | 

## Methods

### NewRPCResponseUnion

`func NewRPCResponseUnion(error_ RPCErrorPayload, id RpcId, result SdkStatusV2Result, ) *RPCResponseUnion`

NewRPCResponseUnion instantiates a new RPCResponseUnion object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewRPCResponseUnionWithDefaults

`func NewRPCResponseUnionWithDefaults() *RPCResponseUnion`

NewRPCResponseUnionWithDefaults instantiates a new RPCResponseUnion object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetError

`func (o *RPCResponseUnion) GetError() RPCErrorPayload`

GetError returns the Error field if non-nil, zero value otherwise.

### GetErrorOk

`func (o *RPCResponseUnion) GetErrorOk() (*RPCErrorPayload, bool)`

GetErrorOk returns a tuple with the Error field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetError

`func (o *RPCResponseUnion) SetError(v RPCErrorPayload)`

SetError sets Error field to given value.


### GetId

`func (o *RPCResponseUnion) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *RPCResponseUnion) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *RPCResponseUnion) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *RPCResponseUnion) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *RPCResponseUnion) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *RPCResponseUnion) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *RPCResponseUnion) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetResult

`func (o *RPCResponseUnion) GetResult() SdkStatusV2Result`

GetResult returns the Result field if non-nil, zero value otherwise.

### GetResultOk

`func (o *RPCResponseUnion) GetResultOk() (*SdkStatusV2Result, bool)`

GetResultOk returns a tuple with the Result field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetResult

`func (o *RPCResponseUnion) SetResult(v SdkStatusV2Result)`

SetResult sets Result field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


