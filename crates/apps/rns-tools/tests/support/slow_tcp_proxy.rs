use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const SLOW_PATH_CHUNK_SIZE: usize = 256;
const SLOW_PATH_CHUNK_DELAY: Duration = Duration::from_millis(25);
pub(super) const SLOW_PATH_FIRST_RESPONSE_DELAY: Duration = Duration::from_secs(2);

pub(super) struct SlowTcpProxy {
    pub(super) port: u16,
    worker: thread::JoinHandle<io::Result<(usize, usize)>>,
}

impl SlowTcpProxy {
    pub(super) fn start(upstream: SocketAddr) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let worker = thread::spawn(move || {
            let (client, _) = listener.accept()?;
            let server = TcpStream::connect(upstream)?;
            let client_reader = client.try_clone()?;
            let server_writer = server.try_clone()?;
            let downstream = thread::spawn(move || {
                relay_with_delay(server, client, Some(SLOW_PATH_FIRST_RESPONSE_DELAY))
            });
            let upstream_bytes = relay_with_delay(client_reader, server_writer, None)?;
            let downstream_bytes = downstream
                .join()
                .map_err(|_| io::Error::other("slow TCP proxy downstream thread panicked"))??;
            Ok((upstream_bytes, downstream_bytes))
        });
        Ok(Self { port, worker })
    }

    pub(super) fn join(self) -> io::Result<(usize, usize)> {
        self.worker.join().map_err(|_| io::Error::other("slow TCP proxy worker panicked"))?
    }
}

fn relay_with_delay(
    mut reader: TcpStream,
    mut writer: TcpStream,
    first_chunk_delay: Option<Duration>,
) -> io::Result<usize> {
    let mut first_chunk_delay = first_chunk_delay;
    let mut forwarded_bytes = 0;
    let mut buffer = [0; SLOW_PATH_CHUNK_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if let Some(delay) = first_chunk_delay.take() {
                    thread::sleep(delay);
                }
                thread::sleep(SLOW_PATH_CHUNK_DELAY);
                writer.write_all(&buffer[..read])?;
                forwarded_bytes += read;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    writer.shutdown(Shutdown::Write)?;
    Ok(forwarded_bytes)
}
