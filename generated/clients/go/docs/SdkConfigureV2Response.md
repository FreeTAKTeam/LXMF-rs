# SdkConfigureV2Response

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Result** | [**SdkConfigureV2Result**](SdkConfigureV2Result.md) |  | 

## Methods

### NewSdkConfigureV2Response

`func NewSdkConfigureV2Response(id RpcId, result SdkConfigureV2Result, ) *SdkConfigureV2Response`

NewSdkConfigureV2Response instantiates a new SdkConfigureV2Response object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkConfigureV2ResponseWithDefaults

`func NewSdkConfigureV2ResponseWithDefaults() *SdkConfigureV2Response`

NewSdkConfigureV2ResponseWithDefaults instantiates a new SdkConfigureV2Response object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetId

`func (o *SdkConfigureV2Response) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *SdkConfigureV2Response) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *SdkConfigureV2Response) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *SdkConfigureV2Response) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *SdkConfigureV2Response) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *SdkConfigureV2Response) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *SdkConfigureV2Response) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetResult

`func (o *SdkConfigureV2Response) GetResult() SdkConfigureV2Result`

GetResult returns the Result field if non-nil, zero value otherwise.

### GetResultOk

`func (o *SdkConfigureV2Response) GetResultOk() (*SdkConfigureV2Result, bool)`

GetResultOk returns a tuple with the Result field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetResult

`func (o *SdkConfigureV2Response) SetResult(v SdkConfigureV2Result)`

SetResult sets Result field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


