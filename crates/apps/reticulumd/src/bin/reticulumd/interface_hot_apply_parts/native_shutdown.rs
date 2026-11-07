use super::*;
use tokio::time::Duration;

pub(crate) async fn stop_owned_native(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: Option<&Arc<Transport>>,
    mut native: NativeInterfaceRuntime,
) -> io::Result<()> {
    if let Some(shutdown) = native.auto_shutdown.take() {
        shutdown.quiesce().await;
    }
    let address = native.address;
    let mut failure = stop_and_join(manager, transport, address).await.err();
    // Native receiver ownership must disappear before completed retirement is reported.
    let auxiliary = std::mem::take(&mut native.auxiliary);
    drop(native);
    for task in auxiliary {
        if let Err(error) =
            join_auxiliary(task, &format!("interface auxiliary address={address}")).await
        {
            failure.get_or_insert(error);
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub(super) async fn join_auxiliary(
    mut task: tokio::task::JoinHandle<()>,
    label: &str,
) -> io::Result<()> {
    match tokio::time::timeout(Duration::from_secs(10), &mut task).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(io::Error::other(format!("{label} failed: {error}"))),
        Err(error) => {
            task.abort();
            if let Err(error) = task.await {
                if !error.is_cancelled() {
                    log::warn!("[daemon] aborted {label} failed: {error}");
                }
            }
            Err(io::Error::new(io::ErrorKind::TimedOut, format!("{label} stop timed out: {error}")))
        }
    }
}

pub(super) async fn stop_and_join(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: Option<&Arc<Transport>>,
    address: AddressHash,
) -> io::Result<()> {
    let workers = manager.lock().await.retire_worker_tree(address);
    stop_hot_apply_interface(manager, transport, address).await;
    let mut failure = None;
    for mut worker in workers {
        match tokio::time::timeout(Duration::from_secs(10), &mut worker).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                failure.get_or_insert_with(|| io::Error::other(error));
            }
            Err(error) => {
                worker.abort();
                match worker.await {
                    Ok(()) => {}
                    Err(error) if error.is_cancelled() => {}
                    Err(error) => log::warn!(
                        "[daemon] aborted interface worker failed address={address}: {error}"
                    ),
                }
                failure.get_or_insert_with(|| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("interface worker stop timed out address={address}: {error}"),
                    )
                });
            }
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
