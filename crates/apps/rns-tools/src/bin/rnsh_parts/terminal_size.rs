#[cfg(unix)]
use rns_transport::transport::TransportChannel;
#[cfg(unix)]
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalWindowSize {
    pub(crate) rows: u64,
    pub(crate) cols: u64,
    pub(crate) hpix: u64,
    pub(crate) vpix: u64,
}

#[cfg(any(unix, test))]
fn select_terminal_window_size(
    candidates: [Option<TerminalWindowSize>; 3],
) -> Option<TerminalWindowSize> {
    candidates.into_iter().flatten().next()
}

#[cfg(unix)]
pub(crate) fn current_terminal_window_size() -> Option<TerminalWindowSize> {
    use std::os::fd::AsFd;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let candidates = [stdin.as_fd(), stdout.as_fd(), stderr.as_fd()].map(|fd| {
        rustix::termios::tcgetwinsize(fd).ok().map(|size| TerminalWindowSize {
            rows: u64::from(size.ws_row),
            cols: u64::from(size.ws_col),
            hpix: u64::from(size.ws_xpixel),
            vpix: u64::from(size.ws_ypixel),
        })
    });
    select_terminal_window_size(candidates)
}

#[cfg(not(unix))]
pub(crate) fn current_terminal_window_size() -> Option<TerminalWindowSize> {
    None
}

#[cfg(unix)]
pub(crate) async fn send_window_size_changes(channel: TransportChannel) -> io::Result<()> {
    use crate::rnsh_parts::protocol::WindowSizeMessage;

    let mut changes =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())?;
    while changes.recv().await.is_some() {
        let Some(size) = current_terminal_window_size() else {
            continue;
        };
        channel
            .send_typed(&WindowSizeMessage {
                rows: Some(size.rows),
                cols: Some(size.cols),
                hpix: Some(size.hpix),
                vpix: Some(size.vpix),
            })
            .await
            .map_err(|error| io::Error::other(format!("remote shell channel error: {error:?}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{select_terminal_window_size, TerminalWindowSize};
    use crate::rnsh_parts::protocol::{ExecuteCommandMessage, WindowSizeMessage};
    use rns_transport::channel::TypedMessage;

    #[test]
    fn terminal_size_uses_first_available_standard_stream() {
        let stdin = TerminalWindowSize { rows: 24, cols: 80, hpix: 0, vpix: 0 };
        let stdout = TerminalWindowSize { rows: 40, cols: 120, hpix: 10, vpix: 20 };
        assert_eq!(select_terminal_window_size([None, Some(stdout), Some(stdin)]), Some(stdout));
    }

    #[test]
    fn terminal_size_is_absent_when_no_standard_stream_is_a_tty() {
        assert_eq!(select_terminal_window_size([None, None, None]), None);
    }

    #[test]
    fn terminal_dimensions_round_trip_in_execute_and_resize_messages() {
        let size = TerminalWindowSize { rows: 31, cols: 101, hpix: 808, vpix: 496 };
        let execute = ExecuteCommandMessage {
            command: Some(vec!["shell".to_owned()]),
            pipe_stdin: false,
            pipe_stdout: false,
            pipe_stderr: false,
            term: Some("xterm-256color".to_owned()),
            rows: Some(size.rows),
            cols: Some(size.cols),
            hpix: Some(size.hpix),
            vpix: Some(size.vpix),
        };
        let decoded = ExecuteCommandMessage::decode(&execute.encode()).expect("decode execute");
        assert_eq!(
            (decoded.rows, decoded.cols, decoded.hpix, decoded.vpix),
            (Some(31), Some(101), Some(808), Some(496))
        );

        let resize = WindowSizeMessage {
            rows: Some(size.rows),
            cols: Some(size.cols),
            hpix: Some(size.hpix),
            vpix: Some(size.vpix),
        };
        let decoded = WindowSizeMessage::decode(&resize.encode()).expect("decode resize");
        assert_eq!(
            (decoded.rows, decoded.cols, decoded.hpix, decoded.vpix),
            (Some(31), Some(101), Some(808), Some(496))
        );
    }
}
