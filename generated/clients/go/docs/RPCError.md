# RPCError

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Error** | [**RPCErrorPayload**](RPCErrorPayload.md) |  | 
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 

## Methods

### NewRPCError

`func NewRPCError(error_ RPCErrorPayload, id RpcId, ) *RPCError`

NewRPCError instantiates a new RPCError object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewRPCErrorWithDefaults

`func NewRPCErrorWithDefaults() *RPCError`

NewRPCErrorWithDefaults instantiates a new RPCError object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetError

`func (o *RPCError) GetError() RPCErrorPayload`

GetError returns the Error field if non-nil, zero value otherwise.

### GetErrorOk

`func (o *RPCError) GetErrorOk() (*RPCErrorPayload, bool)`

GetErrorOk returns a tuple with the Error field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetError

`func (o *RPCError) SetError(v RPCErrorPayload)`

SetError sets Error field to given value.


### GetId

`func (o *RPCError) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *RPCError) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *RPCError) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *RPCError) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *RPCError) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *RPCError) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *RPCError) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.


[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


