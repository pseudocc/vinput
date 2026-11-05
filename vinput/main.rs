mod audio;
mod transcribe;

use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

const HELP: &str = "vinput: push-to-talk voice input\n\nUSAGE: vinput <listen|record|write|stop|toggle|status>\n\nRun listen in the background, then bind toggle (or record and write) to shortcuts.";
static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}

fn install_signal_handlers() -> io::Result<()> {
    unsafe extern "C" {
        fn signal(signal: i32, handler: usize) -> usize;
    }
    // The handler only updates a lock-free atomic; cleanup happens in the event loop.
    for sig in [2, 15] {
        if unsafe { signal(sig, stop as *const () as usize) } == usize::MAX {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    Record,
    Write,
    Toggle,
    Status,
}

impl Request {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "record" => Some(Self::Record),
            "write" | "stop" => Some(Self::Write),
            "toggle" => Some(Self::Toggle),
            "status" => Some(Self::Status),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Recording,
    Processing,
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Start,
    Finish,
    Status,
    Reject(&'static str),
}

fn resolve(phase: Phase, request: Request) -> Action {
    match (phase, request) {
        (_, Request::Status) => Action::Status,
        (Phase::Idle, Request::Record | Request::Toggle) => Action::Start,
        (Phase::Recording, Request::Write | Request::Toggle) => Action::Finish,
        (Phase::Idle, Request::Write) => Action::Reject("not recording"),
        (Phase::Recording, Request::Record) => Action::Reject("already recording"),
        (Phase::Processing, _) => Action::Reject("transcription in progress"),
    }
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::Processing => "processing",
        }
    }
}

enum State {
    Idle,
    Recording(audio::Recording),
    Processing,
}

impl State {
    fn phase(&self) -> Phase {
        match self {
            Self::Idle => Phase::Idle,
            Self::Recording(_) => Phase::Recording,
            Self::Processing => Phase::Processing,
        }
    }
}

struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn socket_path() -> io::Result<PathBuf> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR must be set"))?;
    Ok(PathBuf::from(runtime).join("vinput.sock"))
}

fn bind(path: &Path) -> io::Result<(UnixListener, SocketGuard)> {
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            match UnixStream::connect(path) {
                Ok(_) => return Err(io::Error::other("vinput is already listening")),
                Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                    // Only remove stale sockets, never a regular file or symlink.
                    use std::os::unix::fs::FileTypeExt;
                    if !fs::symlink_metadata(path)?.file_type().is_socket() {
                        return Err(io::Error::other("socket path is not a socket"));
                    }
                    fs::remove_file(path)?;
                    UnixListener::bind(path)?
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    let guard = SocketGuard(path.to_path_buf());
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok((listener, guard))
}

fn setting(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

fn listen(path: &Path) -> io::Result<()> {
    install_signal_handlers()?;
    let runtime = path
        .parent()
        .ok_or_else(|| io::Error::other("invalid runtime directory"))?;
    let settings = transcribe::Settings {
        url: setting("VINPUT_ASR_URL")
            .unwrap_or_else(|| "http://127.0.0.1:18080/v1/audio/transcriptions".into()),
        model: setting("VINPUT_MODEL").unwrap_or_else(|| "whisper-large-v3-turbo".into()),
        language: setting("VINPUT_LANGUAGE"),
    };
    let device = setting("VINPUT_CAPTURE_DEVICE");
    let (listener, _guard) = bind(path)?;
    let (send, receive) = mpsc::channel::<io::Result<()>>();
    let mut state = State::Idle;
    eprintln!("vinput: listening on {}", path.display());
    loop {
        if STOP.load(Ordering::Relaxed) {
            return Ok(());
        }
        if let Ok(result) = receive.try_recv() {
            if let Err(error) = result {
                eprintln!("vinput: {error}");
            }
            state = State::Idle;
        }
        match listener.accept() {
            Ok((mut client, _)) => {
                client.set_read_timeout(Some(Duration::from_secs(2)))?;
                client.set_write_timeout(Some(Duration::from_secs(2)))?;
                let result = handle(
                    &mut client,
                    &mut state,
                    runtime,
                    device.as_deref(),
                    &settings,
                    &send,
                );
                let response = match result {
                    Ok(message) => format!("ok {message}\n"),
                    Err(error) => {
                        format!("error {}\n", error.to_string().replace(['\n', '\r'], " "))
                    }
                };
                let _ = client.write_all(response.as_bytes());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(error) => return Err(error),
        }
    }
}

fn handle(
    client: &mut UnixStream,
    state: &mut State,
    runtime: &Path,
    device: Option<&str>,
    settings: &transcribe::Settings,
    send: &mpsc::Sender<io::Result<()>>,
) -> io::Result<&'static str> {
    let mut input = String::new();
    BufReader::new(client).take(128).read_line(&mut input)?;
    let request =
        Request::parse(input.trim()).ok_or_else(|| io::Error::other("unknown command"))?;
    match resolve(state.phase(), request) {
        Action::Status => Ok(state.phase().label()),
        Action::Reject(message) => Err(io::Error::other(message)),
        Action::Start => {
            *state = State::Recording(audio::Recording::start(runtime, device)?);
            Ok("recording")
        }
        Action::Finish => {
            let State::Recording(recording) = std::mem::replace(state, State::Idle) else {
                unreachable!()
            };
            let wav = recording.finish(runtime)?;
            let settings = settings.clone();
            let send = send.clone();
            std::thread::spawn(move || {
                let _ = send.send(transcribe::run(wav, &settings));
            });
            *state = State::Processing;
            Ok("processing")
        }
    }
}

fn client(path: &Path, command: &str) -> io::Result<()> {
    let mut socket = UnixStream::connect(path)
        .map_err(|error| io::Error::other(format!("cannot connect to vinput listener: {error}")))?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    writeln!(socket, "{command}")?;
    let mut response = String::new();
    BufReader::new(socket).take(4096).read_line(&mut response)?;
    if let Some(message) = response.trim_end().strip_prefix("ok ") {
        println!("{message}");
        Ok(())
    } else {
        Err(io::Error::other(
            response
                .trim_end()
                .strip_prefix("error ")
                .unwrap_or("invalid listener response"),
        ))
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("vinput: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => {
            println!("{HELP}");
            Ok(())
        }
        [arg] if matches!(arg.as_str(), "help" | "--help" | "-h") => {
            println!("{HELP}");
            Ok(())
        }
        [arg] if arg == "--version" => {
            println!("vinput {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        [arg] if arg == "listen" => listen(&socket_path()?),
        [arg] if Request::parse(arg).is_some() => client(&socket_path()?, arg),
        _ => Err(io::Error::other("unknown command; use vinput --help")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_state_transitions() {
        for (phase, request, expected) in [
            (Phase::Idle, Request::Record, Action::Start),
            (Phase::Idle, Request::Toggle, Action::Start),
            (Phase::Recording, Request::Write, Action::Finish),
            (Phase::Recording, Request::Toggle, Action::Finish),
            (Phase::Idle, Request::Write, Action::Reject("not recording")),
            (
                Phase::Recording,
                Request::Record,
                Action::Reject("already recording"),
            ),
            (
                Phase::Processing,
                Request::Toggle,
                Action::Reject("transcription in progress"),
            ),
            (Phase::Processing, Request::Status, Action::Status),
        ] {
            assert_eq!(resolve(phase, request), expected);
        }
        assert_eq!(Request::parse("stop"), Some(Request::Write));
        assert_eq!(Request::parse("bogus"), None);
    }
}
