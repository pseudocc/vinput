use std::fs::File;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

pub const MAX_TRANSCRIPT: u64 = 65_536;

#[derive(Clone)]
pub struct Settings {
    pub url: String,
    pub model: String,
    pub language: Option<String>,
}

pub fn run(wav: File, settings: &Settings, use_rules: bool) -> io::Result<()> {
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
    let text = if use_rules {
        prepare_rules(&text)?
    } else {
        prepare_text(&text)?
    };
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

pub fn prepare_rules(text: &str) -> io::Result<String> {
    // Validate both the ASR response and the output before invoking ydotool.
    let original = text.trim_matches([' ', '\n', '\r', '\t']);
    let has_internal_break = original.contains(['\n', '\r', '\t']);
    let normalized = prepare_text(text)?;
    let mut case = crate::case::CaseState::default();
    if has_internal_break {
        // Keep case state within this recording, but never recognize a
        // multi-word control or a technical form across a line break.
        let text = original
            .split(['\n', '\r', '\t'])
            .map(|part| case.apply(part))
            .collect::<Vec<_>>()
            .join(" ");
        return prepare_text(&text);
    }
    prepare_text(&crate::format::format(&case.apply(&normalized)))
}

fn prepare_text(text: &str) -> io::Result<String> {
    let text = text
        .trim_matches([' ', '\n', '\r', '\t'])
        .replace(['\n', '\r', '\t'], " ");
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
        for text in [
            "hello\0world",
            "你好",
            "hello\u{7f}",
            "\u{b}hello",
            "\u{a0}hello",
        ] {
            assert!(prepare_text(text).is_err());
        }
    }

    #[test]
    fn test_rules_do_not_join_across_line_breaks() {
        assert_eq!(
            prepare_rules("git clean\ndash dash dry dash run").unwrap(),
            "git clean dash dash dry dash run"
        );
        assert_eq!(
            prepare_rules("git clean dash dash dry dash run\n").unwrap(),
            "git clean --dry-run"
        );
        assert_eq!(prepare_rules("HELLO\nWORLD").unwrap(), "hello world");
        assert_eq!(
            prepare_rules("caps lock HELLO\nWORLD capslock DONE").unwrap(),
            "HELLO WORLD done"
        );
        assert_eq!(prepare_rules("caps\nlock TEST").unwrap(), "caps lock test");
        assert_eq!(prepare_rules("one capital\nworld").unwrap(), "one World");
        assert_eq!(prepare_rules("one caps lock\nworld").unwrap(), "one WORLD");
    }

    #[test]
    fn test_case_controls_apply_to_rule_outputs() {
        for (input, expected) in [
            ("FOO_BAR", "foo_bar"),
            ("capital HELLO", "Hello"),
            (
                "one caps lock TWO Three capslock FOUR",
                "one TWO THREE four",
            ),
            ("git dash capital c double dash STATUS", "git -C -- status"),
            (
                "capslock GIT caps lock status dash dash dry dash run",
                "GIT status --dry-run",
            ),
            (
                "HTTPS colon slash slash caps lock API capslock dot EXAMPLE dot ORG slash capital HELLO",
                "https://API.example.org/Hello",
            ),
            (
                "capslock HTTP colon slash slash example dot com",
                "HTTP://EXAMPLE.COM",
            ),
            (
                "git clean dash dash name equals capital FILE underscore capslock FOO capslock",
                "git clean --name=File_FOO",
            ),
            ("dollar sign caps lock HOME capslock", "$HOME"),
            ("capital", ""),
        ] {
            assert_eq!(prepare_rules(input).unwrap(), expected, "{input:?}");
        }
        assert_eq!(prepare_rules("capslock HELLO").unwrap(), "HELLO");
        assert_eq!(prepare_rules("HELLO").unwrap(), "hello");
    }
}
