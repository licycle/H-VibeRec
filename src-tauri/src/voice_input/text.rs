pub fn count_inserted_chars(text: &str) -> i64 {
    text.trim().chars().count() as i64
}

pub fn debug_text_summary(label: &str, text: &str) -> String {
    format!(
        "{label}: chars={} bytes={} lines={} hash={} preview=\"{}\"",
        text.chars().count(),
        text.len(),
        text.lines().count(),
        stable_text_hash(text),
        preview_text(text, 120)
    )
}

pub fn plain_transcript_for_voice_input(text: &str) -> String {
    text.lines()
        .filter_map(|line| {
            let stripped = strip_workflow_label(line.trim());
            if stripped.is_empty() {
                None
            } else {
                Some(compact_cjk_spacing(stripped))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn compact_cjk_spacing(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut pending_space = String::new();
    let mut previous_non_space: Option<char> = None;

    for character in text.chars() {
        if is_horizontal_space(character) {
            pending_space.push(character);
            continue;
        }

        if !pending_space.is_empty() {
            if !previous_non_space
                .map(|previous| should_compact_space_between(previous, character))
                .unwrap_or(false)
            {
                output.push_str(&pending_space);
            }
            pending_space.clear();
        }

        output.push(character);
        previous_non_space = Some(character);
    }

    output.push_str(&pending_space);
    output
}

fn should_compact_space_between(left: char, right: char) -> bool {
    (is_cjk_text_char(left) || is_cjk_punctuation(left))
        && (is_cjk_text_char(right) || is_cjk_punctuation(right))
}

fn is_horizontal_space(character: char) -> bool {
    matches!(
        character,
        ' ' | '\t' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

fn is_cjk_text_char(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

fn is_cjk_punctuation(character: char) -> bool {
    matches!(
        character,
        '\u{3000}'
            ..='\u{303F}'
                | '，'
                | '；'
                | '：'
                | '？'
                | '！'
                | '（'
                | '）'
                | '“'
                | '”'
                | '‘'
                | '’'
    )
}

fn strip_workflow_label(line: &str) -> &str {
    let after_timestamp = if line.starts_with('[') {
        line.find(']')
            .map(|index| line[index + 1..].trim_start())
            .unwrap_or(line)
    } else {
        line
    };

    let Some((speaker, transcript)) = after_timestamp.split_once(':') else {
        return after_timestamp.trim();
    };
    if speaker.trim_start().starts_with("Speaker ") {
        transcript.trim()
    } else {
        after_timestamp.trim()
    }
}

fn stable_text_hash(text: &str) -> String {
    let mut hash = 0x811c_9dc5_u32;
    for byte in text.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut preview = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
