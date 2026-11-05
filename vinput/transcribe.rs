use std::fs::File;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

const MAX_TRANSCRIPT: u64 = 65_536;

#[derive(Clone)]
pub struct Settings {
    pub url: String,
    pub model: String,
    pub language: Option<String>,
}

pub fn run(wav: File, settings: &Settings) -> io::Result<()> {
    let mut command = Command::new("curl");
    command.args([
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        "120",
        "--connect-timeout",
        "5",
        "--form",
        "file=@-;filename=audio.wav;type=audio/wav",
        "--form-string",
        &format!("model={}", settings.model),
        "--form-string",
        "response_format=text",
    ]);
    if let Some(language) = &settings.language {
        command.args(["--form-string", &format!("language={language}")]);
    }
    let mut child = command
        .arg("--")
        .arg(&settings.url)
        .stdin(wav)
        .stdout(Stdio::piped())
        .spawn()?;
    let mut response = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take(MAX_TRANSCRIPT + 1)
        .read_to_end(&mut response);
    if read.is_err() || response.len() as u64 > MAX_TRANSCRIPT {
        let _ = child.kill();
        let _ = child.wait();
        read?;
        return Err(io::Error::other("transcription response is too large"));
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "transcription request failed: {status}"
        )));
    }
    let text =
        String::from_utf8(response).map_err(|_| io::Error::other("transcription is not UTF-8"))?;
    let text = prepare_text(&text)?;
    if text.is_empty() {
        return Ok(());
    }
    let mut child = Command::new("ydotool")
        .args(["type", "--file", "-"])
        .stdin(Stdio::piped())
        .spawn()?;
    let write = child.stdin.take().unwrap().write_all(text.as_bytes());
    let status = child.wait()?;
    write?;
    if !status.success() {
        return Err(io::Error::other(format!("ydotool failed: {status}")));
    }
    Ok(())
}

fn prepare_text(text: &str) -> io::Result<String> {
    let text = text.trim().replace(['\n', '\r', '\t'], " ");
    if !text.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Err(io::Error::other(
            "ydotool cannot reliably type non-ASCII or control characters; transcript was not typed",
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_text_sanitizes_line_breaks_and_preserves_backslashes() {
        assert_eq!(
            prepare_text(" hello\nworld\\n\t ").unwrap(),
            "hello world\\n"
        );
    }

    #[test]
    fn test_prepare_text_rejects_unsupported_characters() {
        for text in ["hello\0world", "你好", "hello\u{7f}"] {
            assert!(prepare_text(text).is_err());
        }
    }
}
