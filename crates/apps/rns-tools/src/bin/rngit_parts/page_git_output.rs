use std::io::Read;
use std::process::Child;

pub(super) fn read_bounded_child_stdout(child: &mut Child, limit: usize) -> Option<Vec<u8>> {
    let (status, output) = read_bounded_child_stdout_with_status(child, limit)?;
    status.success().then_some(output)
}

pub(super) fn read_bounded_child_stdout_with_status(
    child: &mut Child,
    limit: usize,
) -> Option<(std::process::ExitStatus, Vec<u8>)> {
    let Some(stdout) = child.stdout.take() else {
        terminate_and_reap(child);
        return None;
    };
    let Some(read_limit) = u64::try_from(limit).ok().map(|limit| limit.saturating_add(1)) else {
        terminate_and_reap(child);
        return None;
    };
    let mut output = Vec::with_capacity(limit.min(8192));
    if stdout.take(read_limit).read_to_end(&mut output).is_err() {
        terminate_and_reap(child);
        return None;
    }
    if output.len() > limit {
        terminate_and_reap(child);
        return None;
    }
    let status = match child.wait() {
        Ok(status) => status,
        Err(_) => {
            terminate_and_reap(child);
            return None;
        }
    };
    Some((status, output))
}

fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
