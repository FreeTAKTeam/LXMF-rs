use tokio::process::Child;

pub(super) async fn terminate_pipe_child(child: &mut Child, command: &str) -> Result<(), String> {
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => child
            .kill()
            .await
            .map_err(|err| format!("failed to terminate pipe command {command}: {err}"))?,
        Err(err) => return Err(format!("failed to inspect pipe command {command}: {err}")),
    }
    child.wait().await.map_err(|err| format!("failed to reap pipe command {command}: {err}"))?;
    Ok(())
}
