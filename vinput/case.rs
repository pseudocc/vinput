/// Case controls for one rule-mode recording.
///
/// Directives are reserved whole words, even in otherwise unrecognized prose.
/// A state can be reused across segments of a recording, but directive words
/// (including the two words of `caps lock`) never match across calls.
#[derive(Default)]
pub struct CaseState {
    caps_lock: bool,
    capitalize_next: bool,
}

impl CaseState {
    /// Remove spoken case controls and apply them to the remaining text.
    ///
    /// The caller supplies validated ASCII text. Whitespace following a
    /// directive is its separator and is removed; other whitespace and
    /// punctuation are preserved. A terminal period immediately following a
    /// directive is treated as ASR sentence punctuation and removed as well.
    pub fn apply(&mut self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut output = String::with_capacity(text.len());
        let mut i = 0;
        let mut ends_with_control = false;

        while i < chars.len() {
            let directive = if let Some(end) = word_end(&chars, i, "capital") {
                self.capitalize_next = true;
                Some(end)
            } else if let Some(end) = word_end(&chars, i, "capslock") {
                self.caps_lock = !self.caps_lock;
                Some(end)
            } else if let Some(caps_end) = word_end(&chars, i, "caps") {
                let mut lock_start = caps_end;
                while chars.get(lock_start).is_some_and(char::is_ascii_whitespace) {
                    lock_start += 1;
                }
                if lock_start > caps_end {
                    if let Some(end) = word_end(&chars, lock_start, "lock") {
                        self.caps_lock = !self.caps_lock;
                        Some(end)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(mut end) = directive {
                ends_with_control = true;
                if chars.get(end) == Some(&'.')
                    && chars[end + 1..].iter().all(char::is_ascii_whitespace)
                {
                    end += 1;
                }
                while chars.get(end).is_some_and(char::is_ascii_whitespace) {
                    end += 1;
                }
                i = end;
                continue;
            }

            let ch = chars[i];
            ends_with_control = false;
            if ch.is_ascii_whitespace() {
                output.push(ch);
            } else if self.capitalize_next {
                output.push(ch.to_ascii_uppercase());
                self.capitalize_next = false;
            } else if self.caps_lock {
                output.push(ch.to_ascii_uppercase());
            } else {
                output.push(ch.to_ascii_lowercase());
            }
            i += 1;
        }

        // The separator before a terminal control is not text. Dropping one
        // separator lets a caller join separately processed lines with one space.
        if ends_with_control
            && output
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_whitespace())
        {
            output.pop();
        }
        output
    }
}

fn word_end(chars: &[char], start: usize, word: &str) -> Option<usize> {
    // Require whitespace-delimited tokens; words attached to punctuation,
    // paths, or flags are data. A terminal ASR period is allowed. Standalone
    // controls inside a quoted string still apply.
    if start > 0 && !chars[start - 1].is_ascii_whitespace() {
        return None;
    }
    let end = start + word.len();
    let candidate = chars.get(start..end)?;
    let is_boundary = match chars.get(end) {
        None => true,
        Some(ch) if ch.is_ascii_whitespace() => true,
        Some('.') => chars[end + 1..].iter().all(char::is_ascii_whitespace),
        _ => false,
    };
    if !is_boundary
        || !candidate
            .iter()
            .zip(word.bytes())
            .all(|(ch, byte)| ch.eq_ignore_ascii_case(&(byte as char)))
    {
        return None;
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::CaseState;

    #[test]
    fn lowercases_by_default_and_capital_only_affects_one_character() {
        let mut state = CaseState::default();
        assert_eq!(state.apply("HELLO capital WORLD"), "hello World");
        assert_eq!(state.apply("CAPITAL aBC"), "Abc");
    }

    #[test]
    fn caps_toggles_with_both_spellings_and_ignores_control_case() {
        let mut state = CaseState::default();
        assert_eq!(
            state.apply("one CAPS LOCK TWO CapsLock THREE"),
            "one TWO three"
        );
        assert_eq!(state.apply("capslock MiXeD caps lock MiXeD"), "MIXED mixed");
    }

    #[test]
    fn new_state_starts_with_caps_off() {
        let mut first = CaseState::default();
        assert_eq!(first.apply("capslock HELLO"), "HELLO");
        assert_eq!(CaseState::default().apply("HELLO"), "hello");
    }

    #[test]
    fn capital_is_consumed_by_any_non_whitespace_character() {
        let mut state = CaseState::default();
        assert_eq!(state.apply("capital 3ABC capital !HELLO"), "3abc !hello");
        assert_eq!(
            state.apply("capital _HELLO capital .HELLO"),
            "_hello .hello"
        );
    }

    #[test]
    fn capital_and_caps_can_be_combined_and_controls_can_be_consecutive() {
        let mut state = CaseState::default();
        assert_eq!(
            state.apply("capital capslock capital HELLO caps lock WORLD"),
            "HELLO world"
        );
        assert_eq!(
            state.apply("capslock capslock capital capital NAME"),
            "Name"
        );
    }

    #[test]
    fn preserves_unrelated_spacing_and_punctuation() {
        let mut state = CaseState::default();
        assert_eq!(state.apply("foo  capital   BAR"), "foo  Bar");
        assert_eq!(state.apply("  FOO,\tBAR!  "), "  foo,\tbar!  ");
        assert_eq!(state.apply("capital   hello"), "Hello");
        assert_eq!(state.apply("foo\tCAPS\t LOCK  bar"), "foo\tBAR");
    }

    #[test]
    fn recognizes_only_whole_words() {
        let mut state = CaseState::default();
        assert_eq!(
            state.apply("CAPITALIZE precapital capital2 _capital capslocker caps locked"),
            "capitalize precapital capital2 _capital capslocker caps locked"
        );
        assert_eq!(
            state.apply("(capital hello), caps-lock"),
            "(capital hello), caps-lock"
        );
        assert_eq!(
            state.apply("/tmp/capital/file capital_file capital-letter"),
            "/tmp/capital/file capital_file capital-letter"
        );
    }

    #[test]
    fn ignores_terminal_period_on_directives() {
        let mut state = CaseState::default();
        assert_eq!(state.apply("CAPITAL."), "");
        assert_eq!(state.apply("hello"), "Hello");
        assert_eq!(state.apply("CAPS LOCK.  "), "");
        assert_eq!(state.apply("hello"), "HELLO");
        assert_eq!(state.apply("capslock."), "");
        assert_eq!(state.apply("HELLO"), "hello");
    }

    #[test]
    fn nonterminal_period_does_not_create_a_control() {
        let mut state = CaseState::default();
        assert_eq!(state.apply("capital. HELLO"), "capital. hello");
        assert_eq!(state.apply("capslock. hello"), "capslock. hello");
    }

    #[test]
    fn shell_like_text_stays_literal_and_no_control_characters_are_added() {
        let mut state = CaseState::default();
        let input = "capital ; $(ECHO HELLO) && `RM FILE`";
        let output = state.apply(input);
        assert_eq!(output, "; $(echo hello) && `rm file`");
        assert!(!output.chars().any(char::is_control));
    }

    #[test]
    fn state_survives_segments_but_words_do_not_join_across_calls() {
        let mut state = CaseState::default();
        assert_eq!(state.apply("capital"), "");
        assert_eq!(state.apply("  "), "  ");
        assert_eq!(state.apply("hello capslock"), "Hello");
        assert_eq!(state.apply("WORLD caps lock"), "WORLD");
        assert_eq!(state.apply("AGAIN"), "again");

        let mut state = CaseState::default();
        assert_eq!(state.apply("cap"), "cap");
        assert_eq!(state.apply("ital TEXT caps "), "ital text caps ");
        assert_eq!(state.apply("lock MORE"), "lock more");
    }

    #[test]
    fn a_dangling_capital_does_not_affect_a_new_recording() {
        let mut first = CaseState::default();
        assert_eq!(first.apply("capital"), "");
        assert_eq!(CaseState::default().apply("HELLO"), "hello");
    }
}
