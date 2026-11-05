use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const SAMPLE_RATE: u32 = 16_000;
const MAX_BYTES: u64 = SAMPLE_RATE as u64 * 2 * 30;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

fn temporary_file(runtime: &Path) -> io::Result<File> {
    loop {
        let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let path = runtime.join(format!(".vinput-{}-{id}", std::process::id()));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => {
                std::fs::remove_file(path)?;
                return Ok(file);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

pub struct Recording {
    child: Child,
    raw: File,
}

impl Recording {
    pub fn start(runtime: &Path, device: Option<&str>) -> io::Result<Self> {
        let raw = temporary_file(runtime)?;
        let mut command = Command::new("arecord");
        command.args([
            "-q", "-t", "raw", "-f", "S16_LE", "-c", "1", "-r", "16000", "-d", "30",
        ]);
        if let Some(device) = device {
            command.args(["-D", device]);
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(raw.try_clone()?)
            .spawn()?;
        Ok(Self { child, raw })
    }

    pub fn finish(mut self, runtime: &Path) -> io::Result<File> {
        match self.child.try_wait()? {
            Some(status) if !status.success() => {
                return Err(io::Error::other(format!("arecord failed: {status}")));
            }
            Some(_) => {}
            None => {
                // Raw PCM has no header to corrupt when capture is stopped.
                if let Err(error) = self.child.kill()
                    && self.child.try_wait()?.is_none()
                {
                    return Err(error);
                }
                self.child.wait()?;
            }
        }
        let len = self.raw.metadata()?.len();
        if len == 0 || len > MAX_BYTES || len % 2 != 0 {
            return Err(io::Error::other(
                "recording is empty or invalid (maximum 30 seconds)",
            ));
        }
        self.raw.rewind()?;
        let mut wav = temporary_file(runtime)?;
        write_wav(&mut wav, &mut self.raw, len as u32)?;
        wav.rewind()?;
        Ok(wav)
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_wav(out: &mut impl Write, raw: &mut impl Read, len: u32) -> io::Result<()> {
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + len).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16_u32.to_le_bytes())?;
    out.write_all(&1_u16.to_le_bytes())?;
    out.write_all(&1_u16.to_le_bytes())?;
    out.write_all(&SAMPLE_RATE.to_le_bytes())?;
    out.write_all(&(SAMPLE_RATE * 2).to_le_bytes())?;
    out.write_all(&2_u16.to_le_bytes())?;
    out.write_all(&16_u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&len.to_le_bytes())?;
    if io::copy(raw, out)? != u64::from(len) {
        return Err(io::Error::other("recording changed while reading"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wav_header_and_samples() {
        let mut wav = Vec::new();
        write_wav(&mut wav, &mut &b"\x01\x02\x03\x04"[..], 4).unwrap();
        assert_eq!(&wav[..12], b"RIFF\x28\0\0\0WAVE");
        assert_eq!(&wav[20..24], b"\x01\0\x01\0");
        assert_eq!(&wav[24..28], &16_000_u32.to_le_bytes());
        assert_eq!(&wav[36..], b"data\x04\0\0\0\x01\x02\x03\x04");
    }

    #[test]
    fn test_wav_rejects_short_audio() {
        assert!(write_wav(&mut Vec::new(), &mut &b"a"[..], 2).is_err());
    }
}
