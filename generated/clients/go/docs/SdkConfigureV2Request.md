# SdkConfigureV2Request

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Method** | **string** |  | 
**Params** | [**SdkConfigureV2Params**](SdkConfigureV2Params.md) |  | 

## Methods

### NewSdkConfigureV2Request

`func NewSdkConfigureV2Request(id RpcId, method string, params SdkConfigureV2Params, ) *SdkConfigureV2Request`

NewSdkConfigureV2Request instantiates a new SdkConfigureV2Request object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkConfigureV2RequestWithDefaults

`func NewSdkConfigureV2RequestWithDefaults() *SdkConfigureV2Request`

NewSdkConfigureV2RequestWithDefaults instantiates a new SdkConfigureV2Request object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetId

`func (o *SdkConfigureV2Request) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *SdkConfigureV2Request) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *SdkConfigureV2Request) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *SdkConfigureV2Request) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *SdkConfigureV2Request) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *SdkConfigureV2Request) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *SdkConfigureV2Request) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetMethod

`func (o *SdkConfigureV2Request) GetMethod() string`

GetMethod returns the Method field if non-nil, zero value otherwise.

### GetMethodOk

`func (o *SdkConfigureV2Request) GetMethodOk() (*string, bool)`

GetMethodOk returns a tuple with the Method field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetMethod

`func (o *SdkConfigureV2Request) SetMethod(v string)`

SetMethod sets Method field to given value.


### GetParams

`func (o *SdkConfigureV2Request) GetParams() SdkConfigureV2Params`

GetParams returns the Params field if non-nil, zero value otherwise.

### GetParamsOk

`func (o *SdkConfigureV2Request) GetParamsOk() (*SdkConfigureV2Params, bool)`

GetParamsOk returns a tuple with the Params field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetParams

`func (o *SdkConfigureV2Request) SetParams(v SdkConfigureV2Params)`

SetParams sets Params field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


