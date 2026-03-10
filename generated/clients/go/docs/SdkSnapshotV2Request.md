# SdkSnapshotV2Request

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Method** | **string** |  | 
**Params** | [**SdkSnapshotV2Params**](SdkSnapshotV2Params.md) |  | 

## Methods

### NewSdkSnapshotV2Request

`func NewSdkSnapshotV2Request(id RpcId, method string, params SdkSnapshotV2Params, ) *SdkSnapshotV2Request`

NewSdkSnapshotV2Request instantiates a new SdkSnapshotV2Request object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkSnapshotV2RequestWithDefaults

`func NewSdkSnapshotV2RequestWithDefaults() *SdkSnapshotV2Request`

NewSdkSnapshotV2RequestWithDefaults instantiates a new SdkSnapshotV2Request object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetId

`func (o *SdkSnapshotV2Request) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *SdkSnapshotV2Request) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *SdkSnapshotV2Request) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *SdkSnapshotV2Request) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *SdkSnapshotV2Request) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *SdkSnapshotV2Request) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *SdkSnapshotV2Request) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetMethod

`func (o *SdkSnapshotV2Request) GetMethod() string`

GetMethod returns the Method field if non-nil, zero value otherwise.

### GetMethodOk

`func (o *SdkSnapshotV2Request) GetMethodOk() (*string, bool)`

GetMethodOk returns a tuple with the Method field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetMethod

`func (o *SdkSnapshotV2Request) SetMethod(v string)`

SetMethod sets Method field to given value.


### GetParams

`func (o *SdkSnapshotV2Request) GetParams() SdkSnapshotV2Params`

GetParams returns the Params field if non-nil, zero value otherwise.

### GetParamsOk

`func (o *SdkSnapshotV2Request) GetParamsOk() (*SdkSnapshotV2Params, bool)`

GetParamsOk returns a tuple with the Params field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetParams

`func (o *SdkSnapshotV2Request) SetParams(v SdkSnapshotV2Params)`

SetParams sets Params field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


