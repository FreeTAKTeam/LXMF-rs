# SdkNegotiateV2Response

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Id** | [**RpcId**](RpcId.md) |  | 
**Jsonrpc** | Pointer to **string** |  | [optional] 
**Result** | [**SdkNegotiateV2Result**](SdkNegotiateV2Result.md) |  | 

## Methods

### NewSdkNegotiateV2Response

`func NewSdkNegotiateV2Response(id RpcId, result SdkNegotiateV2Result, ) *SdkNegotiateV2Response`

NewSdkNegotiateV2Response instantiates a new SdkNegotiateV2Response object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkNegotiateV2ResponseWithDefaults

`func NewSdkNegotiateV2ResponseWithDefaults() *SdkNegotiateV2Response`

NewSdkNegotiateV2ResponseWithDefaults instantiates a new SdkNegotiateV2Response object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetId

`func (o *SdkNegotiateV2Response) GetId() RpcId`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *SdkNegotiateV2Response) GetIdOk() (*RpcId, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *SdkNegotiateV2Response) SetId(v RpcId)`

SetId sets Id field to given value.


### GetJsonrpc

`func (o *SdkNegotiateV2Response) GetJsonrpc() string`

GetJsonrpc returns the Jsonrpc field if non-nil, zero value otherwise.

### GetJsonrpcOk

`func (o *SdkNegotiateV2Response) GetJsonrpcOk() (*string, bool)`

GetJsonrpcOk returns a tuple with the Jsonrpc field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetJsonrpc

`func (o *SdkNegotiateV2Response) SetJsonrpc(v string)`

SetJsonrpc sets Jsonrpc field to given value.

### HasJsonrpc

`func (o *SdkNegotiateV2Response) HasJsonrpc() bool`

HasJsonrpc returns a boolean if a field has been set.

### GetResult

`func (o *SdkNegotiateV2Response) GetResult() SdkNegotiateV2Result`

GetResult returns the Result field if non-nil, zero value otherwise.

### GetResultOk

`func (o *SdkNegotiateV2Response) GetResultOk() (*SdkNegotiateV2Result, bool)`

GetResultOk returns a tuple with the Result field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetResult

`func (o *SdkNegotiateV2Response) SetResult(v SdkNegotiateV2Result)`

SetResult sets Result field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


