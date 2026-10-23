use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{timeout, Duration};

const PTY_RELAY: &str = r#"
import errno, os, pty, select, time, tty
master, slave = pty.openpty()
tty.setraw(master)
tty.setraw(slave)
def write_all(fd, data):
    while data:
        written = os.write(fd, data)
        if written == 0:
            raise RuntimeError("relay made no progress")
        data = data[written:]
write_all(1, (os.ttyname(slave) + "\n").encode())
os.close(slave)
while True:
    for source in select.select([0, master], [], [])[0]:
        try:
            data = os.read(source, 4096)
        except OSError as error:
            if source == master and error.errno == errno.EIO:
                time.sleep(0.01)
                continue
            raise
        if not data:
            raise SystemExit(0)
        write_all(master if source == 0 else 1, data)
"#;

pub(super) struct PtyPeer {
    child: Child,
    pub(super) input: ChildStdin,
    pub(super) output: BufReader<ChildStdout>,
    pub(super) slave: PathBuf,
}

impl PtyPeer {
    pub(super) async fn start() -> Self {
        let mut child = Command::new("python3")
            .args(["-u", "-c", PTY_RELAY])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("owned raw PTY observer");
        let input = child.stdin.take().expect("PTY observer input");
        let mut output = BufReader::new(child.stdout.take().expect("PTY observer output"));
        let mut greeting = Vec::new();
        timeout(Duration::from_secs(3), output.read_until(b'\n', &mut greeting))
            .await
            .expect("PTY path greeting deadline")
            .expect("complete PTY path greeting");
        assert!(greeting.len() <= 128 && greeting.last() == Some(&b'\n'));
        let slave =
            PathBuf::from(std::str::from_utf8(&greeting).expect("PTY path UTF-8").trim_end());
        assert!(slave.starts_with("/dev/pts/"), "only the owned Linux PTY may be used");
        Self { child, input, output, slave }
    }

    pub(super) fn publish_alias(&self, alias: &Path) {
        let staging = alias.with_extension("ready");
        symlink(&self.slave, &staging).expect("owned ready PTY alias");
        std::fs::rename(staging, alias).expect("publish owned PTY alias atomically");
    }

    pub(super) async fn stop(mut self) {
        self.input.shutdown().await.expect("close owned observer input");
        drop(self.input);
        timeout(Duration::from_secs(2), self.output.read_to_end(&mut Vec::new()))
            .await
            .expect("owned observer output EOF deadline")
            .expect("owned observer output EOF");
        let exit = timeout(Duration::from_secs(3), self.child.wait())
            .await
            .expect("owned PTY child wait deadline")
            .expect("owned PTY child reaped");
        assert!(exit.success(), "PTY observer must exit successfully: {exit}");
    }
}
