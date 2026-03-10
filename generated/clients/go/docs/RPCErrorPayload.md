# RPCErrorPayload

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**Category** | **string** |  | 
**CauseCode** | Pointer to **string** |  | [optional] 
**Details** | [**map[string]ErrorJsonValue**](ErrorJsonValue.md) |  | 
**Extensions** | Pointer to **map[string]interface{}** |  | [optional] 
**IsUserActionable** | **bool** |  | 
**MachineCode** | **string** |  | 
**Message** | **string** |  | 
**Retryable** | **bool** |  | 

## Methods

### NewRPCErrorPayload

`func NewRPCErrorPayload(category string, details map[string]ErrorJsonValue, isUserActionable bool, machineCode string, message string, retryable bool, ) *RPCErrorPayload`

NewRPCErrorPayload instantiates a new RPCErrorPayload object
This constructor will assign default values to properties that have it defined,
and makes sure properties required by API are set, but the set of arguments
will change when the set of required properties is changed

### NewRPCErrorPayloadWithDefaults

`func NewRPCErrorPayloadWithDefaults() *RPCErrorPayload`

NewRPCErrorPayloadWithDefaults instantiates a new RPCErrorPayload object
This constructor will only assign default values to properties that have it defined,
but it doesn't guarantee that properties required by API are set

### GetCategory

`func (o *RPCErrorPayload) GetCategory() string`

GetCategory returns the Category field if non-nil, zero value otherwise.

### GetCategoryOk

`func (o *RPCErrorPayload) GetCategoryOk() (*string, bool)`

GetCategoryOk returns a tuple with the Category field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetCategory

`func (o *RPCErrorPayload) SetCategory(v string)`

SetCategory sets Category field to given value.


### GetCauseCode

`func (o *RPCErrorPayload) GetCauseCode() string`

GetCauseCode returns the CauseCode field if non-nil, zero value otherwise.

### GetCauseCodeOk

`func (o *RPCErrorPayload) GetCauseCodeOk() (*string, bool)`

GetCauseCodeOk returns a tuple with the CauseCode field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetCauseCode

`func (o *RPCErrorPayload) SetCauseCode(v string)`

SetCauseCode sets CauseCode field to given value.

### HasCauseCode

`func (o *RPCErrorPayload) HasCauseCode() bool`

HasCauseCode returns a boolean if a field has been set.

### GetDetails

`func (o *RPCErrorPayload) GetDetails() map[string]ErrorJsonValue`

GetDetails returns the Details field if non-nil, zero value otherwise.

### GetDetailsOk

`func (o *RPCErrorPayload) GetDetailsOk() (*map[string]ErrorJsonValue, bool)`

GetDetailsOk returns a tuple with the Details field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetDetails

`func (o *RPCErrorPayload) SetDetails(v map[string]ErrorJsonValue)`

SetDetails sets Details field to given value.


### GetExtensions

`func (o *RPCErrorPayload) GetExtensions() map[string]interface{}`

GetExtensions returns the Extensions field if non-nil, zero value otherwise.

### GetExtensionsOk

`func (o *RPCErrorPayload) GetExtensionsOk() (*map[string]interface{}, bool)`

GetExtensionsOk returns a tuple with the Extensions field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetExtensions

`func (o *RPCErrorPayload) SetExtensions(v map[string]interface{})`

SetExtensions sets Extensions field to given value.

### HasExtensions

`func (o *RPCErrorPayload) HasExtensions() bool`

HasExtensions returns a boolean if a field has been set.

### GetIsUserActionable

`func (o *RPCErrorPayload) GetIsUserActionable() bool`

GetIsUserActionable returns the IsUserActionable field if non-nil, zero value otherwise.

### GetIsUserActionableOk

`func (o *RPCErrorPayload) GetIsUserActionableOk() (*bool, bool)`

GetIsUserActionableOk returns a tuple with the IsUserActionable field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetIsUserActionable

`func (o *RPCErrorPayload) SetIsUserActionable(v bool)`

SetIsUserActionable sets IsUserActionable field to given value.


### GetMachineCode

`func (o *RPCErrorPayload) GetMachineCode() string`

GetMachineCode returns the MachineCode field if non-nil, zero value otherwise.

### GetMachineCodeOk

`func (o *RPCErrorPayload) GetMachineCodeOk() (*string, bool)`

GetMachineCodeOk returns a tuple with the MachineCode field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetMachineCode

`func (o *RPCErrorPayload) SetMachineCode(v string)`

SetMachineCode sets MachineCode field to given value.


### GetMessage

`func (o *RPCErrorPayload) GetMessage() string`

GetMessage returns the Message field if non-nil, zero value otherwise.

### GetMessageOk

`func (o *RPCErrorPayload) GetMessageOk() (*string, bool)`

GetMessageOk returns a tuple with the Message field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetMessage

`func (o *RPCErrorPayload) SetMessage(v string)`

SetMessage sets Message field to given value.


### GetRetryable

`func (o *RPCErrorPayload) GetRetryable() bool`

GetRetryable returns the Retryable field if non-nil, zero value otherwise.

### GetRetryableOk

`func (o *RPCErrorPayload) GetRetryableOk() (*bool, bool)`

GetRetryableOk returns a tuple with the Retryable field if it's non-nil, zero value otherwise
and a boolean to check if the value has been set.

### SetRetryable

`func (o *RPCErrorPayload) SetRetryable(v bool)`

SetRetryable sets Retryable field to given value.



[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


