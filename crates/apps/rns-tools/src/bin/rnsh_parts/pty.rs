use portable_pty::{
    Child, ChildKiller, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem,
};
use std::io::{self, Read, Write};
use std::path::Path;

pub(crate) struct PtyCommand {
    pub(crate) child: Box<dyn Child + Send + Sync>,
    pub(crate) killer: Box<dyn ChildKiller + Send + Sync>,
    pub(crate) master: Box<dyn MasterPty + Send>,
    pub(crate) reader: Box<dyn Read + Send>,
    pub(crate) writer: Box<dyn Write + Send>,
}

impl PtyCommand {
    pub(crate) fn spawn(
        command: &[String],
        root: &Path,
        term: &str,
        remote_identity: Option<&str>,
        size: PtySize,
    ) -> io::Result<Self> {
        let executable = command.first().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "remote command is empty")
        })?;
        let system = NativePtySystem::default();
        let pair = system.openpty(size).map_err(pty_error)?;
        let mut builder = CommandBuilder::new(executable);
        builder.args(&command[1..]);
        builder.cwd(root);
        builder.env("TERM", term);
        if let Some(remote_identity) = remote_identity {
            builder.env("RNS_REMOTE_IDENTITY", remote_identity);
        }
        let child = pair.slave.spawn_command(builder).map_err(pty_error)?;
        let killer = child.clone_killer();
        let reader = pair.master.try_clone_reader().map_err(pty_error)?;
        let writer = pair.master.take_writer().map_err(pty_error)?;
        Ok(Self { child, killer, master: pair.master, reader, writer })
    }
}

pub(crate) fn window_size(
    rows: Option<u64>,
    cols: Option<u64>,
    hpix: Option<u64>,
    vpix: Option<u64>,
) -> io::Result<PtySize> {
    fn dimension(name: &str, value: Option<u64>, fallback: u16) -> io::Result<u16> {
        value.map_or(Ok(fallback), |value| {
            u16::try_from(value).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, format!("PTY {name} exceeds 65535"))
            })
        })
    }
    Ok(PtySize {
        rows: dimension("rows", rows, 24)?,
        cols: dimension("columns", cols, 80)?,
        pixel_width: dimension("pixel width", hpix, 0)?,
        pixel_height: dimension("pixel height", vpix, 0)?,
    })
}

fn pty_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("rnsh PTY error: {error}"))
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use super::{window_size, PtyCommand};
    use portable_pty::PtySize;
    use std::io::{Read, Write};
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn child_observes_controlling_terminal_and_initial_and_updated_window_size() {
        let initial = PtySize { rows: 31, cols: 101, pixel_width: 808, pixel_height: 496 };
        let mut command = PtyCommand::spawn(
            &[
                "/bin/sh".to_string(),
                "-c".to_string(),
                "test -t 0 && test -t 1 && test -t 2 && test \"$(ps -o tty= -p $$ | tr -d ' ')\" != \"?\" && stty size; read _; stty size".to_string(),
            ],
            Path::new("/"),
            "xterm-test",
            None,
            initial,
        )
        .expect("spawn PTY child");
        assert_eq!(command.master.get_size().expect("initial PTY size"), initial);
        let mut output = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !output.contains(&b'\n') && Instant::now() < deadline {
            let mut chunk = [0u8; 64];
            match command.reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("read initial PTY output: {error}"),
            }
        }
        assert!(String::from_utf8_lossy(&output).contains("31 101"), "{output:?}");

        command
            .master
            .resize(PtySize { rows: 42, cols: 120, pixel_width: 960, pixel_height: 672 })
            .expect("resize PTY");
        assert_eq!(
            command.master.get_size().expect("updated PTY size"),
            PtySize { rows: 42, cols: 120, pixel_width: 960, pixel_height: 672 }
        );
        command.writer.write_all(b"continue\n").expect("write child input");
        command.writer.flush().expect("flush child input");
        output.clear();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !String::from_utf8_lossy(&output).contains("42 120") && Instant::now() < deadline {
            let mut chunk = [0u8; 64];
            match command.reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("read resized PTY output: {error}"),
            }
        }
        assert!(String::from_utf8_lossy(&output).contains("42 120"), "{output:?}");
        assert!(command.child.wait().expect("wait PTY child").success());
    }

    #[test]
    fn window_size_uses_terminal_defaults_and_rejects_truncated_protocol_values() {
        assert_eq!(window_size(None, None, None, None).expect("defaults"), PtySize::default());
        assert_eq!(
            window_size(Some(31), Some(101), Some(808), Some(496)).expect("wire dimensions"),
            PtySize { rows: 31, cols: 101, pixel_width: 808, pixel_height: 496 }
        );
        assert!(window_size(Some(u64::from(u16::MAX) + 1), None, None, None).is_err());
    }
}
