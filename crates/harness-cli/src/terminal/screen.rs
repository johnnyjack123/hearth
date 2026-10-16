//! Reading the agent's terminal stream just enough to take the screen over
//! for a moment: which keys were typed, and whether the alternate screen is
//! active.

/// Longer unfinished escape sequences are no key and are dropped.
const MAX_PENDING: usize = 64;

/// Keys from console input that may arrive in pieces. Plain characters come
/// as they are; keys in win32-input-mode (`ESC [ Vk ; Sc ; Uc ; Kd ; Cs ; Rc
/// _`, which a pseudo console asks the terminal for) are decoded to their
/// character. Other escape sequences (arrows, focus reports) are skipped.
#[derive(Debug, Default)]
pub struct KeyReader {
    /// The start of an escape sequence whose rest has not arrived yet.
    pending: String,
}

impl KeyReader {
    /// Keys in the next piece of input.
    pub fn feed(&mut self, text: &str) -> Vec<char> {
        let mut input = std::mem::take(&mut self.pending);
        input.push_str(text);
        let chars: Vec<char> = input.chars().collect();
        let mut keys = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != '\x1b' {
                keys.push(chars[i]);
                i += 1;
                continue;
            }
            match chars.get(i + 1) {
                // Maybe the start of a sequence; `idle` decides.
                None => {
                    self.pending = "\x1b".to_owned();
                    break;
                }
                Some('[') => {}
                Some(_) => {
                    keys.push('\x1b');
                    i += 1;
                    continue;
                }
            }
            // CSI: parameters and intermediates up to a final byte.
            let start = i + 2;
            let mut end = start;
            while end < chars.len() && !('\x40'..='\x7e').contains(&chars[end]) {
                end += 1;
            }
            if end == chars.len() {
                if end - i <= MAX_PENDING {
                    self.pending = chars[i..].iter().collect();
                }
                break;
            }
            if chars[end] == '_' {
                let params: String = chars[start..end].iter().collect();
                let fields: Vec<u32> = params.split(';').map(|f| f.parse().unwrap_or(0)).collect();
                // Uc is the third field, Kd (key down) the fourth.
                if fields.len() >= 4
                    && fields[3] == 1
                    && let Some(c) = char::from_u32(fields[2]).filter(|&c| c != '\0')
                {
                    keys.push(c);
                }
            }
            i = end + 1;
        }
        keys
    }

    /// No further input came: an ESC on its own was the Esc key.
    pub fn idle(&mut self) -> Vec<char> {
        if self.pending == "\x1b" {
            self.pending.clear();
            return vec!['\x1b'];
        }
        Vec::new()
    }

    /// Forgets a half-read sequence, e.g. when the question is gone.
    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

/// Follows `ESC [ ? … h/l` for the alternate screen (modes 1049, 1047, 47)
/// across chunk boundaries.
#[derive(Debug, Default)]
pub struct AltScreen {
    active: bool,
    /// Start of an escape sequence cut off at the end of the last chunk.
    tail: Vec<u8>,
}

/// Longer unfinished sequences are no mode switch and are dropped.
const MAX_TAIL: usize = 32;

impl AltScreen {
    /// Whether the alternate screen is active after everything fed so far.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Takes the next chunk of output.
    pub fn feed(&mut self, chunk: &[u8]) {
        let mut data = std::mem::take(&mut self.tail);
        data.extend_from_slice(chunk);
        let mut i = 0;
        while i < data.len() {
            if data[i] != 0x1b {
                i += 1;
                continue;
            }
            let rest = &data[i..];
            if rest.len() < 3 {
                if b"\x1b[?".starts_with(rest) {
                    self.tail = rest.to_vec();
                }
                return;
            }
            if &rest[..3] != b"\x1b[?" {
                i += 1;
                continue;
            }
            let params_end = rest[3..]
                .iter()
                .position(|b| !(b.is_ascii_digit() || *b == b';'))
                .map(|p| p + 3);
            let Some(end) = params_end else {
                if rest.len() <= MAX_TAIL {
                    self.tail = rest.to_vec();
                }
                return;
            };
            let set = match rest[end] {
                b'h' => Some(true),
                b'l' => Some(false),
                _ => None,
            };
            if let Some(set) = set {
                let params = std::str::from_utf8(&rest[3..end]).unwrap_or_default();
                if params
                    .split(';')
                    .any(|p| matches!(p, "1049" | "1047" | "47"))
                {
                    self.active = set;
                }
            }
            i += end + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<char> {
        let mut reader = KeyReader::default();
        let mut keys = reader.feed(text);
        keys.extend(reader.idle());
        keys
    }

    #[test]
    fn keys_split_across_reads() {
        let mut reader = KeyReader::default();
        assert_eq!(reader.feed("\x1b[69;18;10"), Vec::<char>::new());
        assert_eq!(reader.feed("1;1;0;1_\x1b"), ['e']);
        // The ESC above was the start of the next key, not the Esc key.
        assert_eq!(reader.feed("[78;49;110;1;0;1_"), ['n']);
        assert_eq!(reader.feed("\x1b"), Vec::<char>::new());
        assert_eq!(reader.idle(), ['\x1b']);
        assert_eq!(reader.idle(), Vec::<char>::new());
        reader.feed("\x1b[69;");
        reader.reset();
        assert_eq!(reader.feed("s"), ['s']);
    }

    #[test]
    fn plain_and_win32_input_keys() {
        assert_eq!(keys("e"), ['e']);
        assert_eq!(keys("\x1b"), ['\x1b']);
        assert_eq!(keys("\x03"), ['\x03']);
        // 'e' pressed and released, then Esc pressed in win32-input-mode.
        assert_eq!(
            keys("\x1b[69;18;101;1;0;1_\x1b[69;18;101;0;0;1_\x1b[27;1;27;1;0;1_"),
            ['e', '\x1b']
        );
        // Shift alone has no character; arrows and focus reports are no keys.
        assert_eq!(keys("\x1b[16;42;0;1;16;1_\x1b[A\x1b[I"), Vec::<char>::new());
        assert_eq!(keys("n\x1b[1;5A"), ['n']);
    }

    #[test]
    fn alternate_screen_across_chunks() {
        let mut alt = AltScreen::default();
        alt.feed(b"hello \x1b[?25l\x1b[?1049h");
        assert!(alt.active());
        alt.feed(b"text\x1b[?10");
        assert!(alt.active());
        alt.feed(b"49l more");
        assert!(!alt.active());
        alt.feed(b"\x1b");
        alt.feed(b"[");
        alt.feed(b"?1;1049h");
        assert!(alt.active());
        alt.feed(b"\x1b[?2004l\x1b[?47l");
        assert!(!alt.active());
        // Other private modes leave it alone.
        alt.feed(b"\x1b[?1000h\x1b[?25h");
        assert!(!alt.active());
    }
}
