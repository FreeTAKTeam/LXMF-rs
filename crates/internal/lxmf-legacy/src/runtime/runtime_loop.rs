use super::{
    handle_runtime_request, RuntimeCommand, RuntimeRequest, RuntimeResponse, WorkerInit,
    WorkerState,
};
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::LocalSet;

pub(super) fn runtime_thread(
    init: WorkerInit,
    command_rx: UnboundedReceiver<RuntimeRequest>,
    startup_tx: std_mpsc::Sender<Result<(), String>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = startup_tx.send(Err(format!("failed to build tokio runtime: {err}")));
            return;
        }
    };

    let local = LocalSet::new();
    local.block_on(&runtime, async move {
        runtime_main(init, command_rx, startup_tx).await;
    });
}

async fn runtime_main(
    init: WorkerInit,
    mut command_rx: UnboundedReceiver<RuntimeRequest>,
    startup_tx: std_mpsc::Sender<Result<(), String>>,
) {
    let mut state = match WorkerState::initialize(init).await {
        Ok(state) => state,
        Err(err) => {
            let _ = startup_tx.send(Err(err.to_string()));
            return;
        }
    };

    let _ = startup_tx.send(Ok(()));

    let mut stopped = false;
    while let Some(request) = command_rx.recv().await {
        let stop_requested = matches!(&request.command, RuntimeCommand::Stop);
        let response = handle_runtime_request(&mut state, request.command).await;
        let should_exit = matches!(response, Ok(RuntimeResponse::Ack)) && stop_requested;
        if should_exit {
            stopped = true;
        }
        let _ = request.respond_to.send(response);
        if should_exit {
            break;
        }
    }

    if !stopped {
        state.shutdown();
    }
}
