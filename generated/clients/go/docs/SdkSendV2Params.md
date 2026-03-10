# SdkSendV2Params

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Content** | **string** |  | 
**Destination** | **string** |  | 
**Fields** | Pointer to **map[string]interface{}** |  | [optional] 
**Id** | **string** |  | 
**IncludeTicket** | Pointer to **NullableBool** |  | [optional] 
**Method** | Pointer to **NullableString** |  | [optional] 
**Source** | **string** |  | 
**StampCost** | Pointer to **NullableInt32** |  | [optional] 
**Title** | Pointer to **string** |  | [optional] 
**TryPropagationOnFail** | Pointer to **NullableBool** |  | [optional] 

## Methods

### NewSdkSendV2Params

`func NewSdkSendV2Params(content string, destination string, id string, source string, ) *SdkSendV2Params`

NewSdkSendV2Params instantiates a new SdkSendV2Params object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkSendV2ParamsWithDefaults

`func NewSdkSendV2ParamsWithDefaults() *SdkSendV2Params`

NewSdkSendV2ParamsWithDefaults instantiates a new SdkSendV2Params object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetContent

`func (o *SdkSendV2Params) GetContent() string`

GetContent returns the Content field if non-nil, zero value otherwise.

### GetContentOk

`func (o *SdkSendV2Params) GetContentOk() (*string, bool)`

GetContentOk returns a tuple with the Content field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetContent

`func (o *SdkSendV2Params) SetContent(v string)`

SetContent sets Content field to given value.


### GetDestination

`func (o *SdkSendV2Params) GetDestination() string`

GetDestination returns the Destination field if non-nil, zero value otherwise.

### GetDestinationOk

`func (o *SdkSendV2Params) GetDestinationOk() (*string, bool)`

GetDestinationOk returns a tuple with the Destination field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetDestination

`func (o *SdkSendV2Params) SetDestination(v string)`

SetDestination sets Destination field to given value.


### GetFields

`func (o *SdkSendV2Params) GetFields() map[string]interface{}`

GetFields returns the Fields field if non-nil, zero value otherwise.

### GetFieldsOk

`func (o *SdkSendV2Params) GetFieldsOk() (*map[string]interface{}, bool)`

GetFieldsOk returns a tuple with the Fields field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetFields

`func (o *SdkSendV2Params) SetFields(v map[string]interface{})`

SetFields sets Fields field to given value.

### HasFields

`func (o *SdkSendV2Params) HasFields() bool`

HasFields returns a boolean if a field has been set.

### SetFieldsNil

`func (o *SdkSendV2Params) SetFieldsNil(b bool)`

 SetFieldsNil sets the value for Fields to be an explicit nil

### UnsetFields
`func (o *SdkSendV2Params) UnsetFields()`

UnsetFields ensures that no value is present for Fields, not even an explicit nil
### GetId

`func (o *SdkSendV2Params) GetId() string`

GetId returns the Id field if non-nil, zero value otherwise.

### GetIdOk

`func (o *SdkSendV2Params) GetIdOk() (*string, bool)`

GetIdOk returns a tuple with the Id field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetId

`func (o *SdkSendV2Params) SetId(v string)`

SetId sets Id field to given value.


### GetIncludeTicket

`func (o *SdkSendV2Params) GetIncludeTicket() bool`

GetIncludeTicket returns the IncludeTicket field if non-nil, zero value otherwise.

### GetIncludeTicketOk

`func (o *SdkSendV2Params) GetIncludeTicketOk() (*bool, bool)`

GetIncludeTicketOk returns a tuple with the IncludeTicket field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetIncludeTicket

`func (o *SdkSendV2Params) SetIncludeTicket(v bool)`

SetIncludeTicket sets IncludeTicket field to given value.

### HasIncludeTicket

`func (o *SdkSendV2Params) HasIncludeTicket() bool`

HasIncludeTicket returns a boolean if a field has been set.

### SetIncludeTicketNil

`func (o *SdkSendV2Params) SetIncludeTicketNil(b bool)`

 SetIncludeTicketNil sets the value for IncludeTicket to be an explicit nil

### UnsetIncludeTicket
`func (o *SdkSendV2Params) UnsetIncludeTicket()`

UnsetIncludeTicket ensures that no value is present for IncludeTicket, not even an explicit nil
### GetMethod

`func (o *SdkSendV2Params) GetMethod() string`

GetMethod returns the Method field if non-nil, zero value otherwise.

### GetMethodOk

`func (o *SdkSendV2Params) GetMethodOk() (*string, bool)`

GetMethodOk returns a tuple with the Method field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetMethod

`func (o *SdkSendV2Params) SetMethod(v string)`

SetMethod sets Method field to given value.

### HasMethod

`func (o *SdkSendV2Params) HasMethod() bool`

HasMethod returns a boolean if a field has been set.

### SetMethodNil

`func (o *SdkSendV2Params) SetMethodNil(b bool)`

 SetMethodNil sets the value for Method to be an explicit nil

### UnsetMethod
`func (o *SdkSendV2Params) UnsetMethod()`

UnsetMethod ensures that no value is present for Method, not even an explicit nil
### GetSource

`func (o *SdkSendV2Params) GetSource() string`

GetSource returns the Source field if non-nil, zero value otherwise.

### GetSourceOk

`func (o *SdkSendV2Params) GetSourceOk() (*string, bool)`

GetSourceOk returns a tuple with the Source field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetSource

`func (o *SdkSendV2Params) SetSource(v string)`

SetSource sets Source field to given value.


### GetStampCost

`func (o *SdkSendV2Params) GetStampCost() int32`

GetStampCost returns the StampCost field if non-nil, zero value otherwise.

### GetStampCostOk

`func (o *SdkSendV2Params) GetStampCostOk() (*int32, bool)`

GetStampCostOk returns a tuple with the StampCost field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetStampCost

`func (o *SdkSendV2Params) SetStampCost(v int32)`

SetStampCost sets StampCost field to given value.

### HasStampCost

`func (o *SdkSendV2Params) HasStampCost() bool`

HasStampCost returns a boolean if a field has been set.

### SetStampCostNil

`func (o *SdkSendV2Params) SetStampCostNil(b bool)`

 SetStampCostNil sets the value for StampCost to be an explicit nil

### UnsetStampCost
`func (o *SdkSendV2Params) UnsetStampCost()`

UnsetStampCost ensures that no value is present for StampCost, not even an explicit nil
### GetTitle

`func (o *SdkSendV2Params) GetTitle() string`

GetTitle returns the Title field if non-nil, zero value otherwise.

### GetTitleOk

`func (o *SdkSendV2Params) GetTitleOk() (*string, bool)`

GetTitleOk returns a tuple with the Title field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetTitle

`func (o *SdkSendV2Params) SetTitle(v string)`

SetTitle sets Title field to given value.

### HasTitle

`func (o *SdkSendV2Params) HasTitle() bool`

HasTitle returns a boolean if a field has been set.

### GetTryPropagationOnFail

`func (o *SdkSendV2Params) GetTryPropagationOnFail() bool`

GetTryPropagationOnFail returns the TryPropagationOnFail field if non-nil, zero value otherwise.

### GetTryPropagationOnFailOk

`func (o *SdkSendV2Params) GetTryPropagationOnFailOk() (*bool, bool)`

GetTryPropagationOnFailOk returns a tuple with the TryPropagationOnFail field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetTryPropagationOnFail

`func (o *SdkSendV2Params) SetTryPropagationOnFail(v bool)`

SetTryPropagationOnFail sets TryPropagationOnFail field to given value.

### HasTryPropagationOnFail

`func (o *SdkSendV2Params) HasTryPropagationOnFail() bool`

HasTryPropagationOnFail returns a boolean if a field has been set.

### SetTryPropagationOnFailNil

`func (o *SdkSendV2Params) SetTryPropagationOnFailNil(b bool)`

 SetTryPropagationOnFailNil sets the value for TryPropagationOnFail to be an explicit nil

### UnsetTryPropagationOnFail
`func (o *SdkSendV2Params) UnsetTryPropagationOnFail()`

UnsetTryPropagationOnFail ensures that no value is present for TryPropagationOnFail, not even an explicit nil

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


