# SdkShutdownV2Request

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Method** | **string** |  | 
**Params** | [**SdkShutdownV2Params**](SdkShutdownV2Params.md) |  | 

## Methods

### NewSdkShutdownV2Request

`func NewSdkShutdownV2Request(id RpcId, method string, params SdkShutdownV2Params, ) *SdkShutdownV2Request`

NewSdkShutdownV2Request instantiates a new SdkShutdownV2Request object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkShutdownV2RequestWithDefaults

`func NewSdkShutdownV2RequestWithDefaults() *SdkShutdownV2Request`

NewSdkShutdownV2RequestWithDefaults instantiates a new SdkShutdownV2Request object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetId

`func (o *SdkShutdownV2Request) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *SdkShutdownV2Request) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *SdkShutdownV2Request) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *SdkShutdownV2Request) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *SdkShutdownV2Request) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *SdkShutdownV2Request) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *SdkShutdownV2Request) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetMethod

`func (o *SdkShutdownV2Request) GetMethod() string`

GetMethod returns the Method field if non-nil, zero value otherwise.

### GetMethodOk

`func (o *SdkShutdownV2Request) GetMethodOk() (*string, bool)`

GetMethodOk returns a tuple with the Method field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetMethod

`func (o *SdkShutdownV2Request) SetMethod(v string)`

SetMethod sets Method field to given value.


### GetParams

`func (o *SdkShutdownV2Request) GetParams() SdkShutdownV2Params`

GetParams returns the Params field if non-nil, zero value otherwise.

### GetParamsOk

`func (o *SdkShutdownV2Request) GetParamsOk() (*SdkShutdownV2Params, bool)`

GetParamsOk returns a tuple with the Params field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetParams

`func (o *SdkShutdownV2Request) SetParams(v SdkShutdownV2Params)`

SetParams sets Params field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


