# RPCRequestUnion

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Method** | **string** |  | 
**Params** | [**SdkStatusV2Params**](SdkStatusV2Params.md) |  | 

## Methods

### NewRPCRequestUnion

`func NewRPCRequestUnion(id RpcId, method string, params SdkStatusV2Params, ) *RPCRequestUnion`

NewRPCRequestUnion instantiates a new RPCRequestUnion object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewRPCRequestUnionWithDefaults

`func NewRPCRequestUnionWithDefaults() *RPCRequestUnion`

NewRPCRequestUnionWithDefaults instantiates a new RPCRequestUnion object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetId

`func (o *RPCRequestUnion) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *RPCRequestUnion) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *RPCRequestUnion) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *RPCRequestUnion) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *RPCRequestUnion) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *RPCRequestUnion) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *RPCRequestUnion) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetMethod

`func (o *RPCRequestUnion) GetMethod() string`

GetMethod returns the Method field if non-nil, zero value otherwise.

### GetMethodOk

`func (o *RPCRequestUnion) GetMethodOk() (*string, bool)`

GetMethodOk returns a tuple with the Method field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetMethod

`func (o *RPCRequestUnion) SetMethod(v string)`

SetMethod sets Method field to given value.


### GetParams

`func (o *RPCRequestUnion) GetParams() SdkStatusV2Params`

GetParams returns the Params field if non-nil, zero value otherwise.

### GetParamsOk

`func (o *RPCRequestUnion) GetParamsOk() (*SdkStatusV2Params, bool)`

GetParamsOk returns a tuple with the Params field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetParams

`func (o *RPCRequestUnion) SetParams(v SdkStatusV2Params)`

SetParams sets Params field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


