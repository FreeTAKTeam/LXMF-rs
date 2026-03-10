# SdkNegotiateV2ParamsConfig

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**AuthMode** | **string** |  | 
**BindMode** | **string** |  | 
**BlockTimeoutMs** | Pointer to **NullableInt32** |  | [optional] 
**OverflowPolicy** | **string** |  | 
**Profile** | **string** |  | 
**RpcBackend** | Pointer to **map[string]interface{}** |  | [optional] 

## Methods

### NewSdkNegotiateV2ParamsConfig

`func NewSdkNegotiateV2ParamsConfig(authMode string, bindMode string, overflowPolicy string, profile string, ) *SdkNegotiateV2ParamsConfig`

NewSdkNegotiateV2ParamsConfig instantiates a new SdkNegotiateV2ParamsConfig object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkNegotiateV2ParamsConfigWithDefaults

`func NewSdkNegotiateV2ParamsConfigWithDefaults() *SdkNegotiateV2ParamsConfig`

NewSdkNegotiateV2ParamsConfigWithDefaults instantiates a new SdkNegotiateV2ParamsConfig object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetAuthMode

`func (o *SdkNegotiateV2ParamsConfig) GetAuthMode() string`

GetAuthMode returns the AuthMode field if non-nil, zero value otherwise.

### GetAuthModeOk

`func (o *SdkNegotiateV2ParamsConfig) GetAuthModeOk() (*string, bool)`

GetAuthModeOk returns a tuple with the AuthMode field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetAuthMode

`func (o *SdkNegotiateV2ParamsConfig) SetAuthMode(v string)`

SetAuthMode sets AuthMode field to given value.


### GetBindMode

`func (o *SdkNegotiateV2ParamsConfig) GetBindMode() string`

GetBindMode returns the BindMode field if non-nil, zero value otherwise.

### GetBindModeOk

`func (o *SdkNegotiateV2ParamsConfig) GetBindModeOk() (*string, bool)`

GetBindModeOk returns a tuple with the BindMode field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetBindMode

`func (o *SdkNegotiateV2ParamsConfig) SetBindMode(v string)`

SetBindMode sets BindMode field to given value.


### GetBlockTimeoutMs

`func (o *SdkNegotiateV2ParamsConfig) GetBlockTimeoutMs() int32`

GetBlockTimeoutMs returns the BlockTimeoutMs field if non-nil, zero value otherwise.

### GetBlockTimeoutMsOk

`func (o *SdkNegotiateV2ParamsConfig) GetBlockTimeoutMsOk() (*int32, bool)`

GetBlockTimeoutMsOk returns a tuple with the BlockTimeoutMs field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetBlockTimeoutMs

`func (o *SdkNegotiateV2ParamsConfig) SetBlockTimeoutMs(v int32)`

SetBlockTimeoutMs sets BlockTimeoutMs field to given value.

### HasBlockTimeoutMs

`func (o *SdkNegotiateV2ParamsConfig) HasBlockTimeoutMs() bool`

HasBlockTimeoutMs returns a boolean if a field has been set.

### SetBlockTimeoutMsNil

`func (o *SdkNegotiateV2ParamsConfig) SetBlockTimeoutMsNil(b bool)`

 SetBlockTimeoutMsNil sets the value for BlockTimeoutMs to be an explicit nil

### UnsetBlockTimeoutMs
`func (o *SdkNegotiateV2ParamsConfig) UnsetBlockTimeoutMs()`

UnsetBlockTimeoutMs ensures that no value is present for BlockTimeoutMs, not even an explicit nil
### GetOverflowPolicy

`func (o *SdkNegotiateV2ParamsConfig) GetOverflowPolicy() string`

GetOverflowPolicy returns the OverflowPolicy field if non-nil, zero value otherwise.

### GetOverflowPolicyOk

`func (o *SdkNegotiateV2ParamsConfig) GetOverflowPolicyOk() (*string, bool)`

GetOverflowPolicyOk returns a tuple with the OverflowPolicy field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetOverflowPolicy

`func (o *SdkNegotiateV2ParamsConfig) SetOverflowPolicy(v string)`

SetOverflowPolicy sets OverflowPolicy field to given value.


### GetProfile

`func (o *SdkNegotiateV2ParamsConfig) GetProfile() string`

GetProfile returns the Profile field if non-nil, zero value otherwise.

### GetProfileOk

`func (o *SdkNegotiateV2ParamsConfig) GetProfileOk() (*string, bool)`

GetProfileOk returns a tuple with the Profile field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetProfile

`func (o *SdkNegotiateV2ParamsConfig) SetProfile(v string)`

SetProfile sets Profile field to given value.


### GetRpcBackend

`func (o *SdkNegotiateV2ParamsConfig) GetRpcBackend() map[string]interface{}`

GetRpcBackend returns the RpcBackend field if non-nil, zero value otherwise.

### GetRpcBackendOk

`func (o *SdkNegotiateV2ParamsConfig) GetRpcBackendOk() (*map[string]interface{}, bool)`

GetRpcBackendOk returns a tuple with the RpcBackend field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetRpcBackend

`func (o *SdkNegotiateV2ParamsConfig) SetRpcBackend(v map[string]interface{})`

SetRpcBackend sets RpcBackend field to given value.

### HasRpcBackend

`func (o *SdkNegotiateV2ParamsConfig) HasRpcBackend() bool`

HasRpcBackend returns a boolean if a field has been set.

### SetRpcBackendNil

`func (o *SdkNegotiateV2ParamsConfig) SetRpcBackendNil(b bool)`

 SetRpcBackendNil sets the value for RpcBackend to be an explicit nil

### UnsetRpcBackend
`func (o *SdkNegotiateV2ParamsConfig) UnsetRpcBackend()`

UnsetRpcBackend ensures that no value is present for RpcBackend, not even an explicit nil

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


