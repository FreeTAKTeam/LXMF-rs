use clap_complete::{generate, Shell};

use lxmf_sdk::{
    error_code, AuthMode, BindMode, Client, ConfigPatch, ErrorCategory, EventCursor, LxmfSdk,
    LxmfSdkManualTick, LxmfSdkPaper, MessageId, OverflowPolicy, PaperMessageEnvelope,
    RpcBackendClient, SdkConfig, SdkError, SendRequest, ShutdownMode, StartRequest, TickBudget,
};

use serde_json::{json, Value as JsonValue};

use std::process::ExitCode;
