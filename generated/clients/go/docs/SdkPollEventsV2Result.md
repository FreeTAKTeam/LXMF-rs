# SdkPollEventsV2Result

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**DroppedCount** | **int32** |  | 
**Events** | **[]map[string]interface{}** |  | 
**NextCursor** | **string** |  | 

## Methods

### NewSdkPollEventsV2Result

`func NewSdkPollEventsV2Result(droppedCount int32, events []map[string]interface{}, nextCursor string, ) *SdkPollEventsV2Result`

NewSdkPollEventsV2Result instantiates a new SdkPollEventsV2Result object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkPollEventsV2ResultWithDefaults

`func NewSdkPollEventsV2ResultWithDefaults() *SdkPollEventsV2Result`

NewSdkPollEventsV2ResultWithDefaults instantiates a new SdkPollEventsV2Result object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetDroppedCount

`func (o *SdkPollEventsV2Result) GetDroppedCount() int32`

GetDroppedCount returns the DroppedCount field if non-nil, zero value otherwise.

### GetDroppedCountOk

`func (o *SdkPollEventsV2Result) GetDroppedCountOk() (*int32, bool)`

GetDroppedCountOk returns a tuple with the DroppedCount field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetDroppedCount

`func (o *SdkPollEventsV2Result) SetDroppedCount(v int32)`

SetDroppedCount sets DroppedCount field to given value.


### GetEvents

`func (o *SdkPollEventsV2Result) GetEvents() []map[string]interface{}`

GetEvents returns the Events field if non-nil, zero value otherwise.

### GetEventsOk

`func (o *SdkPollEventsV2Result) GetEventsOk() (*[]map[string]interface{}, bool)`

GetEventsOk returns a tuple with the Events field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetEvents

`func (o *SdkPollEventsV2Result) SetEvents(v []map[string]interface{})`

SetEvents sets Events field to given value.


### GetNextCursor

`func (o *SdkPollEventsV2Result) GetNextCursor() string`

GetNextCursor returns the NextCursor field if non-nil, zero value otherwise.

### GetNextCursorOk

`func (o *SdkPollEventsV2Result) GetNextCursorOk() (*string, bool)`

GetNextCursorOk returns a tuple with the NextCursor field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetNextCursor

`func (o *SdkPollEventsV2Result) SetNextCursor(v string)`

SetNextCursor sets NextCursor field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


