# SdkNegotiateV2Params

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Config** | [**SdkNegotiateV2ParamsConfig**](SdkNegotiateV2ParamsConfig.md) |  | 
**RequestedCapabilities** | **[]string** |  | 
**SupportedContractVersions** | **[]int32** |  | 

## Methods

### NewSdkNegotiateV2Params

`func NewSdkNegotiateV2Params(config SdkNegotiateV2ParamsConfig, requestedCapabilities []string, supportedContractVersions []int32, ) *SdkNegotiateV2Params`

NewSdkNegotiateV2Params instantiates a new SdkNegotiateV2Params object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkNegotiateV2ParamsWithDefaults

`func NewSdkNegotiateV2ParamsWithDefaults() *SdkNegotiateV2Params`

NewSdkNegotiateV2ParamsWithDefaults instantiates a new SdkNegotiateV2Params object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetConfig

`func (o *SdkNegotiateV2Params) GetConfig() SdkNegotiateV2ParamsConfig`

GetConfig returns the Config field if non-nil, zero value otherwise.

### GetConfigOk

`func (o *SdkNegotiateV2Params) GetConfigOk() (*SdkNegotiateV2ParamsConfig, bool)`

GetConfigOk returns a tuple with the Config field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetConfig

`func (o *SdkNegotiateV2Params) SetConfig(v SdkNegotiateV2ParamsConfig)`

SetConfig sets Config field to given value.


### GetRequestedCapabilities

`func (o *SdkNegotiateV2Params) GetRequestedCapabilities() []string`

GetRequestedCapabilities returns the RequestedCapabilities field if non-nil, zero value otherwise.

### GetRequestedCapabilitiesOk

`func (o *SdkNegotiateV2Params) GetRequestedCapabilitiesOk() (*[]string, bool)`

GetRequestedCapabilitiesOk returns a tuple with the RequestedCapabilities field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetRequestedCapabilities

`func (o *SdkNegotiateV2Params) SetRequestedCapabilities(v []string)`

SetRequestedCapabilities sets RequestedCapabilities field to given value.


### GetSupportedContractVersions

`func (o *SdkNegotiateV2Params) GetSupportedContractVersions() []int32`

GetSupportedContractVersions returns the SupportedContractVersions field if non-nil, zero value otherwise.

### GetSupportedContractVersionsOk

`func (o *SdkNegotiateV2Params) GetSupportedContractVersionsOk() (*[]int32, bool)`

GetSupportedContractVersionsOk returns a tuple with the SupportedContractVersions field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetSupportedContractVersions

`func (o *SdkNegotiateV2Params) SetSupportedContractVersions(v []int32)`

SetSupportedContractVersions sets SupportedContractVersions field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


