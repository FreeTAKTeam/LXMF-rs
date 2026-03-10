# SdkPollEventsV2Params

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Cursor** | **NullableString** |  | 
**Max** | **int32** |  | 

## Methods

### NewSdkPollEventsV2Params

`func NewSdkPollEventsV2Params(cursor NullableString, max int32, ) *SdkPollEventsV2Params`

NewSdkPollEventsV2Params instantiates a new SdkPollEventsV2Params object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkPollEventsV2ParamsWithDefaults

`func NewSdkPollEventsV2ParamsWithDefaults() *SdkPollEventsV2Params`

NewSdkPollEventsV2ParamsWithDefaults instantiates a new SdkPollEventsV2Params object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetCursor

`func (o *SdkPollEventsV2Params) GetCursor() string`

GetCursor returns the Cursor field if non-nil, zero value otherwise.

### GetCursorOk

`func (o *SdkPollEventsV2Params) GetCursorOk() (*string, bool)`

GetCursorOk returns a tuple with the Cursor field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetCursor

`func (o *SdkPollEventsV2Params) SetCursor(v string)`

SetCursor sets Cursor field to given value.


### SetCursorNil

`func (o *SdkPollEventsV2Params) SetCursorNil(b bool)`

 SetCursorNil sets the value for Cursor to be an explicit nil

### UnsetCursor
`func (o *SdkPollEventsV2Params) UnsetCursor()`

UnsetCursor ensures that no value is present for Cursor, not even an explicit nil
### GetMax

`func (o *SdkPollEventsV2Params) GetMax() int32`

GetMax returns the Max field if non-nil, zero value otherwise.

### GetMaxOk

`func (o *SdkPollEventsV2Params) GetMaxOk() (*int32, bool)`

GetMaxOk returns a tuple with the Max field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetMax

`func (o *SdkPollEventsV2Params) SetMax(v int32)`

SetMax sets Max field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


