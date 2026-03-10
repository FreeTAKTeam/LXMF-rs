# SdkShutdownV2Response

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Result** | [**SdkShutdownV2Result**](SdkShutdownV2Result.md) |  | 

## Methods

### NewSdkShutdownV2Response

`func NewSdkShutdownV2Response(id RpcId, result SdkShutdownV2Result, ) *SdkShutdownV2Response`

NewSdkShutdownV2Response instantiates a new SdkShutdownV2Response object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkShutdownV2ResponseWithDefaults

`func NewSdkShutdownV2ResponseWithDefaults() *SdkShutdownV2Response`

NewSdkShutdownV2ResponseWithDefaults instantiates a new SdkShutdownV2Response object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetId

`func (o *SdkShutdownV2Response) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *SdkShutdownV2Response) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *SdkShutdownV2Response) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *SdkShutdownV2Response) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *SdkShutdownV2Response) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *SdkShutdownV2Response) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *SdkShutdownV2Response) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetResult

`func (o *SdkShutdownV2Response) GetResult() SdkShutdownV2Result`

GetResult returns the Result field if non-nil, zero value otherwise.

### GetResultOk

`func (o *SdkShutdownV2Response) GetResultOk() (*SdkShutdownV2Result, bool)`

GetResultOk returns a tuple with the Result field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetResult

`func (o *SdkShutdownV2Response) SetResult(v SdkShutdownV2Result)`

SetResult sets Result field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


