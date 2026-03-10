## lxmfclient@0.1.0

This generator creates TypeScript/JavaScript client that utilizes [axios](https://github.com/axios/axios). The generated Node module can be used in the following environments:

Environment
* Node.js
* Webpack
* Browserify

Language level
* ES5 - you must have a Promises/A+ library installed
* ES6

Module system
* CommonJS
* ES6 module system

It can be used in both TypeScript and JavaScript. In TypeScript, the definition will be automatically resolved via `package.json`. ([Reference](https://www.typescriptlang.org/docs/handbook/declaration-files/consumption.html))

### Building

To build and compile the typescript sources to javascript use:
```
npm install
npm run build
```

### Publishing

First build the package then run `npm publish`

### Consuming

navigate to the folder of your consuming project and run one of the following commands.

_published:_

```
npm install lxmfclient@0.1.0 --save
```

_unPublished (not recommended):_

```
npm install PATH_TO_GENERATED_PACKAGE --save
```

### Documentation for API Endpoints

All URIs are relative to *http://localhost*

Class | Method | HTTP request | Description
------------ | ------------- | ------------- | -------------
*DefaultApi* | [**rpc**](docs/DefaultApi.md#rpc) | **POST** /rpc | 


### Documentation For Models

 - [ErrorJsonValue](docs/ErrorJsonValue.md)
 - [RPCError](docs/RPCError.md)
 - [RPCErrorPayload](docs/RPCErrorPayload.md)
 - [RPCRequest](docs/RPCRequest.md)
 - [RPCRequestUnion](docs/RPCRequestUnion.md)
 - [RPCResponseUnion](docs/RPCResponseUnion.md)
 - [RPCSuccess](docs/RPCSuccess.md)
 - [RpcId](docs/RpcId.md)
 - [SdkCancelMessageV2Params](docs/SdkCancelMessageV2Params.md)
 - [SdkCancelMessageV2Request](docs/SdkCancelMessageV2Request.md)
 - [SdkCancelMessageV2Response](docs/SdkCancelMessageV2Response.md)
 - [SdkCancelMessageV2Result](docs/SdkCancelMessageV2Result.md)
 - [SdkConfigureV2Params](docs/SdkConfigureV2Params.md)
 - [SdkConfigureV2Request](docs/SdkConfigureV2Request.md)
 - [SdkConfigureV2Response](docs/SdkConfigureV2Response.md)
 - [SdkConfigureV2Result](docs/SdkConfigureV2Result.md)
 - [SdkNegotiateV2Params](docs/SdkNegotiateV2Params.md)
 - [SdkNegotiateV2ParamsConfig](docs/SdkNegotiateV2ParamsConfig.md)
 - [SdkNegotiateV2Request](docs/SdkNegotiateV2Request.md)
 - [SdkNegotiateV2Response](docs/SdkNegotiateV2Response.md)
 - [SdkNegotiateV2Result](docs/SdkNegotiateV2Result.md)
 - [SdkPollEventsV2Params](docs/SdkPollEventsV2Params.md)
 - [SdkPollEventsV2Request](docs/SdkPollEventsV2Request.md)
 - [SdkPollEventsV2Response](docs/SdkPollEventsV2Response.md)
 - [SdkPollEventsV2Result](docs/SdkPollEventsV2Result.md)
 - [SdkSendV2Params](docs/SdkSendV2Params.md)
 - [SdkSendV2Request](docs/SdkSendV2Request.md)
 - [SdkSendV2Response](docs/SdkSendV2Response.md)
 - [SdkSendV2Result](docs/SdkSendV2Result.md)
 - [SdkShutdownV2Params](docs/SdkShutdownV2Params.md)
 - [SdkShutdownV2Request](docs/SdkShutdownV2Request.md)
 - [SdkShutdownV2Response](docs/SdkShutdownV2Response.md)
 - [SdkShutdownV2Result](docs/SdkShutdownV2Result.md)
 - [SdkSnapshotV2Params](docs/SdkSnapshotV2Params.md)
 - [SdkSnapshotV2Request](docs/SdkSnapshotV2Request.md)
 - [SdkSnapshotV2Response](docs/SdkSnapshotV2Response.md)
 - [SdkSnapshotV2Result](docs/SdkSnapshotV2Result.md)
 - [SdkStatusV2Params](docs/SdkStatusV2Params.md)
 - [SdkStatusV2Request](docs/SdkStatusV2Request.md)
 - [SdkStatusV2Response](docs/SdkStatusV2Response.md)
 - [SdkStatusV2Result](docs/SdkStatusV2Result.md)


<a id="documentation-for-authorization"></a>
## Documentation For Authorization

Endpoints do not require authorization.

