# SdkSnapshotV2Result

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**ActiveContractVersion** | **int32** |  | 
**ConfigRevision** | **int32** |  | 
**CountsIncluded** | **bool** |  | 
**EffectiveCapabilities** | **[]string** |  | 
**EventStreamPosition** | **int32** |  | 
**InFlightMessages** | **int32** |  | 
**Profile** | **string** |  | 
**QueuedMessages** | **int32** |  | 
**RuntimeId** | **string** |  | 
**State** | **string** |  | 

## Methods

### NewSdkSnapshotV2Result

`func NewSdkSnapshotV2Result(activeContractVersion int32, configRevision int32, countsIncluded bool, effectiveCapabilities []string, eventStreamPosition int32, inFlightMessages int32, profile string, queuedMessages int32, runtimeId string, state string, ) *SdkSnapshotV2Result`

NewSdkSnapshotV2Result instantiates a new SdkSnapshotV2Result object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewSdkSnapshotV2ResultWithDefaults

`func NewSdkSnapshotV2ResultWithDefaults() *SdkSnapshotV2Result`

NewSdkSnapshotV2ResultWithDefaults instantiates a new SdkSnapshotV2Result object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetActiveContractVersion

`func (o *SdkSnapshotV2Result) GetActiveContractVersion() int32`

GetActiveContractVersion returns the ActiveContractVersion field if non-nil, zero value otherwise.

### GetActiveContractVersionOk

`func (o *SdkSnapshotV2Result) GetActiveContractVersionOk() (*int32, bool)`

GetActiveContractVersionOk returns a tuple with the ActiveContractVersion field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetActiveContractVersion

`func (o *SdkSnapshotV2Result) SetActiveContractVersion(v int32)`

SetActiveContractVersion sets ActiveContractVersion field to given value.


### GetConfigRevision

`func (o *SdkSnapshotV2Result) GetConfigRevision() int32`

GetConfigRevision returns the ConfigRevision field if non-nil, zero value otherwise.

### GetConfigRevisionOk

`func (o *SdkSnapshotV2Result) GetConfigRevisionOk() (*int32, bool)`

GetConfigRevisionOk returns a tuple with the ConfigRevision field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetConfigRevision

`func (o *SdkSnapshotV2Result) SetConfigRevision(v int32)`

SetConfigRevision sets ConfigRevision field to given value.


### GetCountsIncluded

`func (o *SdkSnapshotV2Result) GetCountsIncluded() bool`

GetCountsIncluded returns the CountsIncluded field if non-nil, zero value otherwise.

### GetCountsIncludedOk

`func (o *SdkSnapshotV2Result) GetCountsIncludedOk() (*bool, bool)`

GetCountsIncludedOk returns a tuple with the CountsIncluded field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetCountsIncluded

`func (o *SdkSnapshotV2Result) SetCountsIncluded(v bool)`

SetCountsIncluded sets CountsIncluded field to given value.


### GetEffectiveCapabilities

`func (o *SdkSnapshotV2Result) GetEffectiveCapabilities() []string`

GetEffectiveCapabilities returns the EffectiveCapabilities field if non-nil, zero value otherwise.

### GetEffectiveCapabilitiesOk

`func (o *SdkSnapshotV2Result) GetEffectiveCapabilitiesOk() (*[]string, bool)`

GetEffectiveCapabilitiesOk returns a tuple with the EffectiveCapabilities field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetEffectiveCapabilities

`func (o *SdkSnapshotV2Result) SetEffectiveCapabilities(v []string)`

SetEffectiveCapabilities sets EffectiveCapabilities field to given value.


### GetEventStreamPosition

`func (o *SdkSnapshotV2Result) GetEventStreamPosition() int32`

GetEventStreamPosition returns the EventStreamPosition field if non-nil, zero value otherwise.

### GetEventStreamPositionOk

`func (o *SdkSnapshotV2Result) GetEventStreamPositionOk() (*int32, bool)`

GetEventStreamPositionOk returns a tuple with the EventStreamPosition field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetEventStreamPosition

`func (o *SdkSnapshotV2Result) SetEventStreamPosition(v int32)`

SetEventStreamPosition sets EventStreamPosition field to given value.


### GetInFlightMessages

`func (o *SdkSnapshotV2Result) GetInFlightMessages() int32`

GetInFlightMessages returns the InFlightMessages field if non-nil, zero value otherwise.

### GetInFlightMessagesOk

`func (o *SdkSnapshotV2Result) GetInFlightMessagesOk() (*int32, bool)`

GetInFlightMessagesOk returns a tuple with the InFlightMessages field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetInFlightMessages

`func (o *SdkSnapshotV2Result) SetInFlightMessages(v int32)`

SetInFlightMessages sets InFlightMessages field to given value.


### GetProfile

`func (o *SdkSnapshotV2Result) GetProfile() string`

GetProfile returns the Profile field if non-nil, zero value otherwise.

### GetProfileOk

`func (o *SdkSnapshotV2Result) GetProfileOk() (*string, bool)`

GetProfileOk returns a tuple with the Profile field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetProfile

`func (o *SdkSnapshotV2Result) SetProfile(v string)`

SetProfile sets Profile field to given value.


### GetQueuedMessages

`func (o *SdkSnapshotV2Result) GetQueuedMessages() int32`

GetQueuedMessages returns the QueuedMessages field if non-nil, zero value otherwise.

### GetQueuedMessagesOk

`func (o *SdkSnapshotV2Result) GetQueuedMessagesOk() (*int32, bool)`

GetQueuedMessagesOk returns a tuple with the QueuedMessages field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetQueuedMessages

`func (o *SdkSnapshotV2Result) SetQueuedMessages(v int32)`

SetQueuedMessages sets QueuedMessages field to given value.


### GetRuntimeId

`func (o *SdkSnapshotV2Result) GetRuntimeId() string`

GetRuntimeId returns the RuntimeId field if non-nil, zero value otherwise.

### GetRuntimeIdOk

`func (o *SdkSnapshotV2Result) GetRuntimeIdOk() (*string, bool)`

GetRuntimeIdOk returns a tuple with the RuntimeId field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetRuntimeId

`func (o *SdkSnapshotV2Result) SetRuntimeId(v string)`

SetRuntimeId sets RuntimeId field to given value.


### GetState

`func (o *SdkSnapshotV2Result) GetState() string`

GetState returns the State field if non-nil, zero value otherwise.

### GetStateOk

`func (o *SdkSnapshotV2Result) GetStateOk() (*string, bool)`

GetStateOk returns a tuple with the State field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetState

`func (o *SdkSnapshotV2Result) SetState(v string)`

SetState sets State field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


