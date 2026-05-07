#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHotkey {
    pub command: bool,
    pub shift: bool,
    pub option: bool,
    pub control: bool,
    pub key_code: u16,
}

pub const CARBON_CMD_KEY: u32 = 1 << 8;
pub const CARBON_SHIFT_KEY: u32 = 1 << 9;
pub const CARBON_OPTION_KEY: u32 = 1 << 11;
pub const CARBON_CONTROL_KEY: u32 = 1 << 12;

impl ParsedHotkey {
    pub fn is_modifier_only(&self) -> bool {
        self.key_code == u16::MAX
    }

    pub fn carbon_key_code(&self) -> u32 {
        self.key_code as u32
    }

    pub fn carbon_modifiers(&self) -> u32 {
        let mut modifiers = 0;
        if self.command {
            modifiers |= CARBON_CMD_KEY;
        }
        if self.shift {
            modifiers |= CARBON_SHIFT_KEY;
        }
        if self.option {
            modifiers |= CARBON_OPTION_KEY;
        }
        if self.control {
            modifiers |= CARBON_CONTROL_KEY;
        }
        modifiers
    }
}

pub fn display_hotkey_for_status(value: &str) -> String {
    match parse_hotkey(value) {
        Ok(parsed) => parsed.display_label(),
        Err(_) => value.trim().to_string(),
    }
}

pub fn parse_hotkey(value: &str) -> Result<ParsedHotkey, String> {
    let mut parsed = ParsedHotkey {
        command: false,
        shift: false,
        option: false,
        control: false,
        key_code: u16::MAX,
    };
    let mut saw_key = false;

    for token in value
        .split('+')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
    {
        match normalize_token(token).as_str() {
            "COMMAND" | "CMD" | "META" | "COMMANDORCONTROL" | "PRIMARY" => {
                parsed.command = true;
            }
            "SHIFT" => parsed.shift = true,
            "OPTION" | "OPT" | "ALT" => parsed.option = true,
            "CONTROL" | "CTRL" => parsed.control = true,
            key => {
                if saw_key {
                    return Err("hotkey can only contain one non-modifier key".to_string());
                }
                parsed.key_code = key_code_for_token(key)
                    .ok_or_else(|| format!("unsupported hotkey key: {token}"))?;
                saw_key = true;
            }
        }
    }

    let has_primary_modifier = parsed.command || parsed.option || parsed.control;
    let has_any_modifier = has_primary_modifier || parsed.shift;

    if !saw_key {
        if has_primary_modifier {
            return Ok(parsed);
        }
        return Err("modifier-only hotkey must include Command, Option, or Control".to_string());
    }
    if !has_any_modifier {
        return Err("hotkey must include at least one modifier".to_string());
    }
    Ok(parsed)
}

impl ParsedHotkey {
    fn display_label(&self) -> String {
        let mut label = String::new();
        if self.command {
            label.push('⌘');
        }
        if self.control {
            label.push('⌃');
        }
        if self.option {
            label.push('⌥');
        }
        if self.shift {
            label.push('⇧');
        }
        if !self.is_modifier_only() {
            label.push_str(key_label_for_code(self.key_code));
        }
        label
    }
}

fn normalize_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-' && *ch != '_')
        .flat_map(|ch| ch.to_uppercase())
        .collect()
}

fn key_code_for_token(token: &str) -> Option<u16> {
    Some(match token {
        "SPACE" => 49,
        "RETURN" | "ENTER" => 36,
        "TAB" => 48,
        "ESC" | "ESCAPE" => 53,
        "A" => 0,
        "S" => 1,
        "D" => 2,
        "F" => 3,
        "H" => 4,
        "G" => 5,
        "Z" => 6,
        "X" => 7,
        "C" => 8,
        "V" => 9,
        "B" => 11,
        "Q" => 12,
        "W" => 13,
        "E" => 14,
        "R" => 15,
        "Y" => 16,
        "T" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "EQUAL" | "=" => 24,
        "9" => 25,
        "7" => 26,
        "MINUS" | "-" => 27,
        "8" => 28,
        "0" => 29,
        "RIGHTBRACKET" | "]" => 30,
        "O" => 31,
        "U" => 32,
        "LEFTBRACKET" | "[" => 33,
        "I" => 34,
        "P" => 35,
        "L" => 37,
        "J" => 38,
        "QUOTE" | "'" => 39,
        "K" => 40,
        "SEMICOLON" | ";" => 41,
        "BACKSLASH" | "\\" => 42,
        "COMMA" | "," => 43,
        "SLASH" | "/" => 44,
        "N" => 45,
        "M" => 46,
        "PERIOD" | "." => 47,
        _ => return None,
    })
}

fn key_label_for_code(key_code: u16) -> &'static str {
    match key_code {
        49 => "Space",
        36 => "Enter",
        48 => "Tab",
        53 => "Esc",
        0 => "A",
        1 => "S",
        2 => "D",
        3 => "F",
        4 => "H",
        5 => "G",
        6 => "Z",
        7 => "X",
        8 => "C",
        9 => "V",
        11 => "B",
        12 => "Q",
        13 => "W",
        14 => "E",
        15 => "R",
        16 => "Y",
        17 => "T",
        18 => "1",
        19 => "2",
        20 => "3",
        21 => "4",
        22 => "6",
        23 => "5",
        24 => "=",
        25 => "9",
        26 => "7",
        27 => "-",
        28 => "8",
        29 => "0",
        30 => "]",
        31 => "O",
        32 => "U",
        33 => "[",
        34 => "I",
        35 => "P",
        37 => "L",
        38 => "J",
        39 => "'",
        40 => "K",
        41 => ";",
        42 => "\\",
        43 => ",",
        44 => "/",
        45 => "N",
        46 => "M",
        47 => ".",
        _ => "?",
    }
}
