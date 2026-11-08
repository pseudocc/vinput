//! A deliberately small, deterministic grammar for standalone dictation.
//!
//! This is not a shell parser or an intent classifier. Unrecognized text is left
//! byte-for-byte unchanged. Only a complete spoken HTTP(S) URL, a recognized
//! command with spoken options, a spoken symbol expression, or an utterance
//! starting with a spoken option is formatted. Ambiguous phrases can still look
//! like technical text; callers must
//! let the user review the result, especially before using it in a terminal.
//!
//! Words are separated by ASCII spaces; other whitespace/control characters
//! cause a verbatim return. Outer and unrelated internal spaces are preserved.
//! In a formatted URL or expression, or at the end of an edited command span,
//! a final period may be treated as Whisper sentence punctuation. A repeated
//! `!` or `?` after a dictated symbol is treated as transcription punctuation.
//! These cases are ambiguous. No rule creates a newline, tab, or control character.

#[derive(Clone, Copy)]
struct Word<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn is(word: Word<'_>, expected: &str) -> bool {
    word.text.eq_ignore_ascii_case(expected)
}

fn atom(word: Word<'_>) -> bool {
    !word.text.is_empty() && word.text.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn separator(word: Word<'_>) -> bool {
    is(word, "dot") || is(word, "dash") || is(word, "slash")
}

// Whole-word names only; "dash" is handled separately by the option grammar.
fn symbol(words: &[Word<'_>], i: usize) -> Option<(usize, char)> {
    let word = *words.get(i)?;
    let next = words.get(i + 1).copied();
    if is(word, "question") && next.is_some_and(|w| is(w, "mark")) {
        return Some((i + 2, '?'));
    }
    if is(word, "exclamation") && next.is_some_and(|w| is(w, "mark") || is(w, "point")) {
        return Some((i + 2, '!'));
    }
    if is(word, "at") && next.is_some_and(|w| is(w, "sign")) {
        return Some((i + 2, '@'));
    }
    if is(word, "dollar") && next.is_some_and(|w| is(w, "sign")) {
        return Some((i + 2, '$'));
    }
    if is(word, "back") && next.is_some_and(|w| is(w, "slash")) {
        return Some((i + 2, '\\'));
    }
    if is(word, "vertical") && next.is_some_and(|w| is(w, "bar")) {
        return Some((i + 2, '|'));
    }
    if (is(word, "open") || is(word, "close")) && next.is_some_and(|w| is(w, "parenthesis")) {
        return Some((i + 2, if is(word, "open") { '(' } else { ')' }));
    }
    if next.is_some_and(|w| is(w, "sign")) {
        let ch = if is(word, "hash") {
            '#'
        } else if is(word, "percent") {
            '%'
        } else if is(word, "plus") {
            '+'
        } else if is(word, "equal") || is(word, "equals") {
            '='
        } else {
            return None;
        };
        return Some((i + 2, ch));
    }
    let ch = if is(word, "backslash") {
        '\\'
    } else if is(word, "underscore") {
        '_'
    } else if is(word, "equals") {
        '='
    } else if is(word, "tilde") {
        '~'
    } else if is(word, "hash") {
        '#'
    } else if is(word, "ampersand") {
        '&'
    } else if is(word, "plus") {
        '+'
    } else if is(word, "asterisk") {
        '*'
    } else if is(word, "percent") {
        '%'
    } else if is(word, "pipe") {
        '|'
    } else if is(word, "caret") {
        '^'
    } else if is(word, "slash") {
        '/'
    } else if is(word, "dot") {
        '.'
    } else if is(word, "colon") {
        ':'
    } else {
        return None;
    };
    Some((i + 1, ch))
}

// A literal expression alternates word components and spoken symbols. Adjacent
// ordinary words terminate the expression instead of being silently joined.
fn literal(words: &[Word<'_>], start: usize, allow_existing: bool) -> Option<(usize, String)> {
    let mut result = String::new();
    let mut i = start;
    let mut previous_atom = false;
    let mut has_symbol = false;
    let mut has_new_symbol = false;
    let mut symbol_count = 0;
    let mut atom_count = 0;
    while i < words.len() {
        if let Some((end, ch)) = symbol(words, i) {
            result.push(ch);
            i = end;
            previous_atom = false;
            has_symbol = true;
            symbol_count += 1;
            has_new_symbol |= !matches!(ch, '/' | '.' | ':');
        } else if atom(words[i]) && !previous_atom {
            result.push_str(words[i].text);
            i += 1;
            previous_atom = true;
            atom_count += 1;
        } else {
            break;
        }
    }
    (has_new_symbol || (allow_existing && has_symbol) || (symbol_count == 1 && atom_count == 0))
        .then_some((i, result))
}

// HTTP(S) ':' '/' '/' host (('.' | '-') host)* [ '/' path ... ]
// The scheme is the explicit signal; require at least one host dot, and
// consume the entire utterance so prose after a URL cannot become a path.
// Host/path components are ASCII alphanumeric words. Reserved separator words
// cannot be components; more elaborate URLs are intentionally left alone.
fn url(words: &[Word<'_>]) -> Option<String> {
    let scheme = *words.first()?;
    if !(is(scheme, "http") || is(scheme, "https"))
        || words.len() < 7
        || !is(words[1], "colon")
        || !is(words[2], "slash")
        || !is(words[3], "slash")
        || !atom(words[4])
        || separator(words[4])
    {
        return None;
    }

    let mut result = format!("{}://{}", scheme.text, words[4].text);
    let mut dots = 0;
    let mut in_path = false;
    let mut i = 5;
    while i < words.len() {
        let sep = words[i];
        let ch = if is(sep, "dot") {
            if !in_path {
                dots += 1;
            }
            '.'
        } else if is(sep, "dash") {
            '-'
        } else if is(sep, "slash") && dots > 0 {
            in_path = true;
            '/'
        } else {
            return None;
        };
        result.push(ch);
        i += 1;
        // A final spoken slash is a legitimate URL suffix.
        if i == words.len() && ch == '/' {
            break;
        }
        let component = *words.get(i)?;
        if !atom(component) || separator(component) {
            return None;
        }
        result.push_str(component.text);
        i += 1;
    }
    (dots > 0).then_some(result)
}

// Extend a complete URL only after a valid host. Explicit query/fragment
// delimiters are literal; no URL encoding, shell escaping, or host inference.
fn extended_url(words: &[Word<'_>]) -> Option<String> {
    if words.len() < 7 {
        return None;
    }
    let mut host_end = 5;
    while let (Some(&sep), Some(&part)) = (words.get(host_end), words.get(host_end + 1)) {
        if !(is(sep, "dot") || is(sep, "dash")) || !atom(part) || separator(part) {
            break;
        }
        host_end += 2;
    }
    let mut result = url(&words[..host_end])?;
    let mut i = host_end;
    let mut first = true;
    while i < words.len() {
        let (end, ch) =
            symbol(words, i).or_else(|| is(words[i], "dash").then_some((i + 1, '-')))?;
        if (first && !matches!(ch, '/' | '?' | '#')) || ch == '\\' || ch == ':' {
            return None;
        }
        first = false;
        result.push(ch);
        i = end;
        if i == words.len() && ch == '/' {
            break;
        }
        // A query or fragment can follow an empty path.
        if ch == '/' && symbol(words, i).is_some_and(|(_, next)| matches!(next, '?' | '#')) {
            continue;
        }
        // A path may start with a literal tilde, as in /~user.
        if ch == '/' && words.get(i).is_some_and(|&w| is(w, "tilde")) {
            result.push('~');
            i += 1;
            if i == words.len()
                || symbol(words, i).is_some_and(|(_, next)| matches!(next, '/' | '?' | '#'))
            {
                continue;
            }
        } else if ch == '='
            && let Some((end, value)) = symbol(words, i)
            && !matches!(value, '\\' | ':')
        {
            result.push(value);
            i = end;
            continue;
        }
        let component = *words.get(i)?;
        if !atom(component) || separator(component) {
            return None;
        }
        result.push_str(component.text);
        i += 1;
    }
    (!first).then_some(result)
}

// Only known executable contexts opt into command formatting. Git and cargo
// also require a known subcommand followed immediately by an option, or an
// option immediately after the name. Other executables require an option
// immediately after the name. This avoids treating prose after a command-like
// word as an option. Unknown programs/subcommands remain verbatim.
fn command_start(words: &[Word<'_>]) -> bool {
    let Some((&name, rest)) = words.split_first() else {
        return false;
    };
    let Some(&next) = rest.first() else {
        return false;
    };
    let name = name.text.to_ascii_lowercase();
    let next_name = next.text.to_ascii_lowercase();
    let option_first = is(next, "dash") || is(next, "double");
    let option_after_subcommand = words
        .get(2)
        .is_some_and(|&word| is(word, "dash") || is(word, "double"));
    match name.as_str() {
        "git" => {
            option_first
                || option_after_subcommand
                    && matches!(
                        next_name.as_str(),
                        "add"
                            | "branch"
                            | "checkout"
                            | "clean"
                            | "clone"
                            | "commit"
                            | "diff"
                            | "fetch"
                            | "log"
                            | "merge"
                            | "pull"
                            | "push"
                            | "rebase"
                            | "reset"
                            | "restore"
                            | "show"
                            | "status"
                            | "switch"
                            | "tag"
                    )
        }
        "cargo" => {
            option_first
                || option_after_subcommand
                    && matches!(
                        next_name.as_str(),
                        "build" | "check" | "clippy" | "doc" | "fmt" | "run" | "test"
                    )
        }
        "curl" | "ls" | "grep" | "find" | "docker" => option_first,
        _ => false,
    }
}

// dash dash WORD (dash WORD)* => --WORD(-WORD)*
// dash LETTER => -LETTER; dash capital LETTER => -LETTER (uppercase).
// double dash => -- (the following word remains a separate argument).
// A single dash followed by a whole word is not interpreted: too easy to
// confuse with ordinary English. Non-ASCII and punctuated option components
// are left intact rather than guessing how to spell or quote them.
fn option(words: &[Word<'_>], i: usize) -> Option<(usize, String)> {
    let word = *words.get(i)?;
    let next = *words.get(i + 1)?;
    if is(word, "double") && is(next, "dash") {
        return Some((i + 2, "--".into()));
    }
    if !is(word, "dash") {
        return None;
    }
    if is(next, "dash") {
        let first = *words.get(i + 2)?;
        if !atom(first) || separator(first) || is(first, "capital") {
            return None;
        }
        let mut result = format!("--{}", first.text);
        let mut end = i + 3;
        while let (Some(&joiner), Some(&part)) = (words.get(end), words.get(end + 1)) {
            if !(is(joiner, "dash") || is(joiner, "underscore"))
                || !atom(part)
                || separator(part)
                || is(part, "capital")
            {
                break;
            }
            result.push(if is(joiner, "dash") { '-' } else { '_' });
            result.push_str(part.text);
            end += 2;
        }
        if let Some((value_index, '=')) = symbol(words, end) {
            if let Some((value_end, value)) = literal(words, value_index, true) {
                result.push('=');
                result.push_str(&value);
                end = value_end;
            } else if let Some(&value) = words.get(value_index)
                && atom(value)
                && !separator(value)
                && symbol(words, value_index).is_none()
            {
                result.push('=');
                result.push_str(value.text);
                end = value_index + 1;
            }
        }
        return Some((end, result));
    }
    let (end, letter, uppercase) = if is(next, "capital") {
        (i + 3, *words.get(i + 2)?, true)
    } else {
        (i + 2, next, false)
    };
    if letter.text.len() != 1 || !letter.text.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let letter = if uppercase {
        letter.text.to_ascii_uppercase()
    } else {
        letter.text.to_owned()
    };
    Some((end, format!("-{letter}")))
}

/// Format recognizable spoken URL and command notation without executing it.
/// Unsupported forms are returned unchanged; see the module grammar and its
/// limitations. Even a recognized form may have been intended as ordinary text.
pub fn format(text: &str) -> String {
    if text
        .chars()
        .any(|c| c.is_control() || (c.is_whitespace() && c != ' '))
    {
        return text.to_owned();
    }
    let mut words = Vec::new();
    let mut pos = 0;
    while pos < text.len() {
        if text.as_bytes()[pos] == b' ' {
            pos += 1;
            continue;
        }
        let start = pos;
        while pos < text.len() && text.as_bytes()[pos] != b' ' {
            pos += 1;
        }
        words.push(Word {
            text: &text[start..pos],
            start,
            end: pos,
        });
    }
    let Some(last) = words.last_mut() else {
        return text.to_owned();
    };
    // Only peel terminal punctuation from a plain word, not e.g. a filename.
    let mut final_period = None;
    if let Some(base) = last.text.strip_suffix(['.', '!', '?']) {
        if !base.is_empty() && base.bytes().all(|b| b.is_ascii_alphanumeric()) {
            if last.text.ends_with('.') {
                final_period = Some(last.end - 1);
            }
            last.text = base;
            last.end -= 1;
        }
    }

    let spoken_url = words.len() >= 4
        && (is(words[0], "http") || is(words[0], "https"))
        && is(words[1], "colon")
        && is(words[2], "slash")
        && is(words[3], "slash");
    let formatted = extended_url(&words).or_else(|| url(&words)).or_else(|| {
        (!spoken_url)
            .then(|| literal(&words, 0, false))
            .flatten()
            .and_then(|(end, text)| (end == words.len()).then_some(text))
    });
    if let Some(formatted) = formatted {
        let mut result = String::new();
        result.push_str(&text[..words[0].start]);
        result.push_str(&formatted);
        let end = words.last().unwrap().end;
        // Whisper may repeat a dictated terminal question or exclamation mark.
        let duplicate = matches!(
            (formatted.as_bytes().last(), text.as_bytes().get(end)),
            (Some(b'?'), Some(b'?')) | (Some(b'!'), Some(b'!'))
        );
        let suffix_start = end + usize::from(final_period.is_some() || duplicate);
        result.push_str(&text[suffix_start..]);
        return result;
    }

    // A standalone single-dash phrase such as "dash a little salt" is
    // ordinary English; only the explicit two-dash forms are standalone.
    let standalone = words.get(1).is_some_and(|&second| {
        (is(words[0], "dash") && is(second, "dash"))
            || (is(words[0], "double") && is(second, "dash"))
    });
    if !standalone && !command_start(&words) {
        return text.to_owned();
    }
    // A standalone option must actually start at word zero. Do not find an
    // option later in an otherwise unrecognized phrase.
    if standalone && option(&words, 0).is_none() {
        return text.to_owned();
    }
    let mut edits = Vec::new();
    let mut i = usize::from(!standalone);
    let mut after_option = false;
    while i < words.len() {
        if let Some((end, replacement)) = option(&words, i) {
            edits.push((words[i].start, words[end - 1].end, replacement));
            i = end;
            after_option = true;
        } else if after_option
            && let Some((end, replacement)) = literal(&words, i, false)
            && (symbol(&words, i).is_none() || end == words.len())
        {
            edits.push((words[i].start, words[end - 1].end, replacement));
            i = end;
            after_option = false;
        } else {
            i += 1;
            after_option = false;
        }
    }
    if edits.is_empty() {
        return text.to_owned();
    }
    if let Some(period) = final_period
        && edits.last().is_some_and(|(_, end, _)| *end == period)
    {
        edits.push((period, period + 1, String::new()));
    }
    let mut result = String::new();
    let mut end = 0;
    for (start, next_end, replacement) in edits {
        result.push_str(&text[end..start]);
        result.push_str(&replacement);
        end = next_end;
    }
    result.push_str(&text[end..]);
    result
}

#[cfg(test)]
mod tests {
    use super::format;

    #[test]
    fn requested_examples() {
        for (input, expected) in [
            (
                "https colon slash slash oem dash share dot canonical dot com",
                "https://oem-share.canonical.com",
            ),
            ("git clean dash dash dry dash run", "git clean --dry-run"),
            ("git dash capital C double dash status", "git -C -- status"),
            ("double dash status", "-- status"),
            ("dash dash status", "--status"),
        ] {
            assert_eq!(format(input), expected, "{input:?}");
        }
    }

    #[test]
    fn whisper_case_punctuation_and_spaces() {
        for (input, expected) in [
            (
                "Https colon slash slash oem dash share dot canonical dot com.",
                "Https://oem-share.canonical.com",
            ),
            (
                "  HTTP  colon slash slash Example dot com!  ",
                "  HTTP://Example.com!  ",
            ),
            (
                "https colon slash slash example dot com?",
                "https://example.com?",
            ),
            (
                "Git  clean dash dash dry dash run.  ",
                "Git  clean --dry-run  ",
            ),
            (
                "Git dash capital c double dash status.",
                "Git -C -- status.",
            ),
            ("Dash dash status.", "--status"),
            ("Double dash status!", "-- status!"),
            ("git status", "git status"),
        ] {
            assert_eq!(format(input), expected, "{input:?}");
        }
    }

    #[test]
    fn urls_are_complete_and_explicit() {
        assert_eq!(
            format("https colon slash slash example dot com slash docs dash v2 dot html"),
            "https://example.com/docs-v2.html"
        );
        assert_eq!(
            format("https colon slash slash example dot com slash"),
            "https://example.com/"
        );
        for text in [
            "example dot com",
            "visit https colon slash slash example dot com",
            "https colon slash slash example dot com and more",
            "https colon slash slash example",
            "https colon slash slash example dot",
            "https colon slash slash example dot com slash café",
            "https colon slash slash example dot com slash foo.bar",
        ] {
            assert_eq!(format(text), text);
        }
    }

    #[test]
    fn test_spoken_symbols_and_literal_expressions() {
        for (input, expected) in [
            ("  Question mark.  ", "  ?  "),
            ("question mark?", "?"),
            ("exclamation mark!", "!"),
            ("question mark question mark?", "??"),
            ("slash", "/"),
            ("dot", "."),
            ("colon", ":"),
            ("exclamation mark", "!"),
            ("exclamation point", "!"),
            ("backslash", "\\"),
            ("back slash", "\\"),
            ("underscore", "_"),
            ("equals", "="),
            ("equal sign", "="),
            ("equals sign", "="),
            ("tilde", "~"),
            ("at sign", "@"),
            ("hash", "#"),
            ("hash sign", "#"),
            ("ampersand", "&"),
            ("plus", "+"),
            ("plus sign", "+"),
            ("asterisk", "*"),
            ("percent sign", "%"),
            ("pipe", "|"),
            ("vertical bar", "|"),
            ("dollar sign", "$"),
            ("caret", "^"),
            ("open parenthesis x close parenthesis", "(x)"),
            ("hello question mark", "hello?"),
            ("foo underscore bar", "foo_bar"),
            ("foo equals bar", "foo=bar"),
            ("tilde slash config", "~/config"),
            ("name at sign example dot org", "name@example.org"),
            ("foo backslash bar", "foo\\bar"),
            ("a exclamation mark b", "a!b"),
            ("a hash b ampersand c", "a#b&c"),
            ("dollar sign HOME", "$HOME"),
        ] {
            assert_eq!(format(input), expected, "{input:?}");
        }
        for text in [
            "I have a question mark",
            "an at sign is useful",
            "example dot com",
        ] {
            assert_eq!(format(text), text);
        }
    }

    #[test]
    fn test_extended_urls_and_invalid_urls() {
        for (input, expected) in [
            (
                "https colon slash slash example dot com slash search question mark q equals rust ampersand page equals two",
                "https://example.com/search?q=rust&page=two",
            ),
            (
                "https colon slash slash example dot com slash foo underscore bar",
                "https://example.com/foo_bar",
            ),
            (
                "https colon slash slash example dot com question mark hash equals value hash top",
                "https://example.com?hash=value#top",
            ),
            (
                "https colon slash slash example dot com slash tilde user slash name exclamation mark end",
                "https://example.com/~user/name!end",
            ),
            (
                "https colon slash slash example dot com slash tilde slash config",
                "https://example.com/~/config",
            ),
            (
                "https colon slash slash example dot com slash question mark q equals rust",
                "https://example.com/?q=rust",
            ),
            (
                "https colon slash slash example dot com question mark q equals tilde",
                "https://example.com?q=~",
            ),
        ] {
            assert_eq!(format(input), expected, "{input:?}");
        }
        for text in [
            "https colon slash slash example dot com question mark q equals",
            "https colon slash slash example dot com slash foo backslash bar",
            "https colon slash slash example dot com at sign other dot com",
            "https colon slash slash example",
        ] {
            assert_eq!(format(text), text);
        }
    }

    #[test]
    fn test_symbols_in_recognized_command_options() {
        for (input, expected) in [
            (
                "cargo test dash dash test underscore name",
                "cargo test --test_name",
            ),
            (
                "git clean dash dash name equals foo underscore bar",
                "git clean --name=foo_bar",
            ),
            (
                "git clean dash dash name equals sign foo",
                "git clean --name=foo",
            ),
            (
                "git clean dash dash exclude equals foo slash bar",
                "git clean --exclude=foo/bar",
            ),
            (
                "cargo test dash dash pattern equals question mark",
                "cargo test --pattern=?",
            ),
            (
                "git status dash dash verbose foo underscore bar",
                "git status --verbose foo_bar",
            ),
            (
                "git clean dash dash force hash remove everything",
                "git clean --force hash remove everything",
            ),
            (
                "git clean dash dash force not a question mark",
                "git clean --force not a question mark",
            ),
            ("git clean dash dash force file.", "git clean --force file."),
            (
                "git clean dash dash force question mark",
                "git clean --force ?",
            ),
            (
                "git clean dash dash force tilde slash config",
                "git clean --force ~/config",
            ),
        ] {
            assert_eq!(format(input), expected, "{input:?}");
        }
    }

    #[test]
    fn options_and_unknown_words() {
        assert_eq!(
            format("cargo test dash dash release double dash test_name"),
            "cargo test --release -- test_name"
        );
        assert_eq!(format("ls dash a"), "ls -a");
        assert_eq!(
            format("git clean dash dash dry dash run unusual_token.txt"),
            "git clean --dry-run unusual_token.txt"
        );
        assert_eq!(
            format("git status dash dash verbose 雪"),
            "git status --verbose 雪"
        );
        for text in [
            "git frobnicate dash dash force",
            "unknown dash dash status",
            "git clean dash",
            "git clean dash dash",
            "git clean dash capital city",
            "git clean dash dash café",
            "dash dash",
            "double trouble dash status",
        ] {
            assert_eq!(format(text), text);
        }
    }

    #[test]
    fn prose_and_control_characters_pass_through() {
        for text in [
            "",
            "  ",
            "Hello, world.",
            "Use a dash and a dot, then capital C.",
            "Dash a little salt on it.",
            "Find a dash a child drew.",
            "Git status shows dash a character.",
            "I saw a dash dash status message.",
            "The capital city is nice.",
            "Git is a word dash and capital A.",
            "git status\ndash dash verbose",
            "git\tclean dash dash dry dash run",
            "https colon slash slash example dot com\0",
            "git clean\u{a0}dash dash dry dash run",
        ] {
            assert_eq!(format(text), text);
        }
    }
}
