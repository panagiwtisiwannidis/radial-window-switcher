use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Space,
    Tab,
    Enter,
    Escape,
    Shift,
}

pub struct Keymap {
    keys: HashMap<u32, Key>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            keys: HashMap::from([
                (57, Key::Space),
                (15, Key::Tab),
                (28, Key::Enter),
                (96, Key::Enter),
                (1, Key::Escape),
                (42, Key::Shift),
                (54, Key::Shift),
            ]),
        }
    }
}

fn key_for(symbol: &str) -> Option<Key> {
    match symbol {
        "space" => Some(Key::Space),
        "Tab" | "ISO_Left_Tab" => Some(Key::Tab),
        "Return" | "KP_Enter" => Some(Key::Enter),
        "Escape" => Some(Key::Escape),
        "Shift_L" | "Shift_R" => Some(Key::Shift),
        _ => None,
    }
}

fn section<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(name)?;
    let open = start + text[start..].find('{')?;
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn first_symbol(block: &str) -> Option<&str> {
    let mut rest = block;
    while let Some(open) = rest.find('[') {
        let close = open + rest[open..].find(']')?;
        let inside = rest[open + 1..close].trim();
        let is_index = inside.chars().all(|c| c.is_ascii_digit())
            || inside.to_ascii_lowercase().starts_with("group");
        if !is_index {
            return inside.split(',').next().map(str::trim);
        }
        rest = &rest[close + 1..];
    }
    None
}

impl Keymap {
    pub fn parse(text: &str) -> Option<Self> {
        let codes_text = section(text, "xkb_keycodes")?;
        let symbols_text = section(text, "xkb_symbols")?;
        let mut codes: HashMap<String, u32> = HashMap::new();
        let mut aliases: Vec<(String, String)> = Vec::new();
        for statement in codes_text.split(';') {
            let statement = statement.trim();
            if let Some(rest) = statement.strip_prefix("alias") {
                if let Some((alias, real)) = rest.split_once('=') {
                    aliases.push((alias.trim().to_string(), real.trim().to_string()));
                }
            } else if let Some((name, code)) = statement.split_once('=') {
                let name = name.trim();
                if name.starts_with('<') {
                    if let Ok(code) = code.trim().parse::<u32>() {
                        codes.insert(name.to_string(), code);
                    }
                }
            }
        }
        for (alias, real) in aliases {
            if let Some(code) = codes.get(&real).copied() {
                codes.entry(alias).or_insert(code);
            }
        }

        let mut keys = HashMap::new();
        let mut rest = symbols_text;
        while let Some(at) = rest.find("key <") {
            rest = &rest[at + 4..];
            let Some(name_end) = rest.find('>') else {
                break;
            };
            let name = &rest[..=name_end];
            let block = &rest[name_end..rest.find("};").unwrap_or(rest.len())];
            if let (Some(key), Some(code)) =
                (first_symbol(block).and_then(key_for), codes.get(name))
            {
                keys.insert(code.saturating_sub(8), key);
            }
            rest = &rest[name_end..];
        }
        (!keys.is_empty()).then_some(Self { keys })
    }

    pub fn key(&self, code: u32) -> Option<Key> {
        self.keys.get(&code).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_compositor_keymap() {
        let text = r#"xkb_keymap {
            xkb_keycodes "(unnamed)" {
                minimum = 8;
                <ESC> = 9;
                <RTRN> = 36;
                <SPCE> = 65;
                <K1> = 10;
                alias <AC12> = <RTRN>;
            };
            xkb_types "(unnamed)" { include "complete" };
            xkb_symbols "(unnamed)" {
                key <ESC> { [ Escape ] };
                key <RTRN> { type= "ONE_LEVEL", symbols[1]= [ Return ] };
                key <SPCE> { [ space ] };
                key <K1> { [ Tab, ISO_Left_Tab ] };
            };
        };"#;
        let map = Keymap::parse(text).unwrap();
        assert_eq!(map.key(1), Some(Key::Escape));
        assert_eq!(map.key(28), Some(Key::Enter));
        assert_eq!(map.key(57), Some(Key::Space));
        assert_eq!(map.key(2), Some(Key::Tab));
    }
}
