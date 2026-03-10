# SdkSendV2Response

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Result** | [**SdkSendV2Result**](SdkSendV2Result.md) |  | 

## Methods

### NewSdkSendV2Response

`func NewSdkSendV2Response(id RpcId, result SdkSendV2Result, ) *SdkSendV2Response`

NewSdkSendV2Response instantiates a new SdkSendV2Response object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkSendV2ResponseWithDefaults

`func NewSdkSendV2ResponseWithDefaults() *SdkSendV2Response`

NewSdkSendV2ResponseWithDefaults instantiates a new SdkSendV2Response object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetId

`func (o *SdkSendV2Response) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *SdkSendV2Response) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *SdkSendV2Response) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *SdkSendV2Response) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *SdkSendV2Response) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *SdkSendV2Response) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *SdkSendV2Response) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetResult

`func (o *SdkSendV2Response) GetResult() SdkSendV2Result`

GetResult returns the Result field if non-nil, zero value otherwise.

### GetResultOk

`func (o *SdkSendV2Response) GetResultOk() (*SdkSendV2Result, bool)`

GetResultOk returns a tuple with the Result field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetResult

`func (o *SdkSendV2Response) SetResult(v SdkSendV2Result)`

SetResult sets Result field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


