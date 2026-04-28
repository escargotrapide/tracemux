//! EOL / encoding helpers via `encoding_rs`.
//!
//! Supported encodings: UTF-8, `Shift_JIS`, `EUC-JP`, `ISO-2022-JP`, CP932.

use encoding_rs::Encoding;

/// EOL convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    /// `\n`
    Lf,
    /// `\r\n`
    Crlf,
    /// `\r`
    Cr,
    /// Auto-detect.
    Auto,
}

impl Eol {
    /// Bytes that terminate a line in this convention.
    #[must_use]
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf | Self::Auto => b"\n",
            Self::Crlf => b"\r\n",
            Self::Cr => b"\r",
        }
    }

    /// Inspect a sample and pick the most likely EOL. Counts are
    /// computed over the first 64 KiB only; ties favour `Lf`.
    #[must_use]
    pub fn detect(sample: &[u8]) -> Self {
        let limit = sample.len().min(64 * 1024);
        let buf = &sample[..limit];
        let mut crlf = 0usize;
        let mut cr = 0usize;
        let mut lf = 0usize;
        let mut i = 0;
        while i < buf.len() {
            match buf[i] {
                b'\r' if i + 1 < buf.len() && buf[i + 1] == b'\n' => {
                    crlf += 1;
                    i += 2;
                    continue;
                }
                b'\r' => cr += 1,
                b'\n' => lf += 1,
                _ => {}
            }
            i += 1;
        }
        if crlf >= lf && crlf >= cr && crlf > 0 {
            Self::Crlf
        } else if cr > lf && cr > 0 {
            Self::Cr
        } else {
            Self::Lf
        }
    }
}

/// Resolve an encoding label (case-insensitive). Falls back to UTF-8
/// for unknown labels.
#[must_use]
pub fn encoding_for_label(label: &str) -> &'static Encoding {
    Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8)
}

/// Decode `bytes` using the named encoding, replacing malformed
/// sequences. Returns the decoded `String` and a flag indicating
/// whether replacement characters were emitted.
#[must_use]
pub fn decode(bytes: &[u8], label: &str) -> (String, bool) {
    let enc = encoding_for_label(label);
    let (cow, _, had_errors) = enc.decode(bytes);
    (cow.into_owned(), had_errors)
}

/// Iterate over lines in `buf` according to `eol`. The terminator is
/// stripped from each yielded slice. Trailing data without a terminator
/// is yielded as the last slice.
pub fn split_lines<'a>(buf: &'a [u8], eol: Eol) -> impl Iterator<Item = &'a [u8]> {
    LineIter {
        buf,
        eol,
        done: false,
    }
}

struct LineIter<'a> {
    buf: &'a [u8],
    eol: Eol,
    done: bool,
}

impl<'a> Iterator for LineIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        // Auto resolves per-call to avoid mid-stream flips.
        let resolved = match self.eol {
            Eol::Auto => Eol::detect(self.buf),
            other => other,
        };
        let (term_pos, term_len): (Option<usize>, usize) = match resolved {
            Eol::Lf => (self.buf.iter().position(|&b| b == b'\n'), 1),
            Eol::Cr => (self.buf.iter().position(|&b| b == b'\r'), 1),
            Eol::Crlf => (self.buf.windows(2).position(|w| w == b"\r\n"), 2),
            Eol::Auto => unreachable!(),
        };
        match term_pos {
            Some(idx) => {
                let (line, rest) = self.buf.split_at(idx);
                self.buf = &rest[term_len..];
                Some(line)
            }
            None => {
                self.done = true;
                if self.buf.is_empty() {
                    None
                } else {
                    let line = self.buf;
                    self.buf = &[];
                    Some(line)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_lf() {
        assert_eq!(Eol::detect(b"a\nb\nc\n"), Eol::Lf);
    }

    #[test]
    fn detect_crlf() {
        assert_eq!(Eol::detect(b"a\r\nb\r\n"), Eol::Crlf);
    }

    #[test]
    fn detect_cr_only() {
        assert_eq!(Eol::detect(b"a\rb\rc\r"), Eol::Cr);
    }

    #[test]
    fn split_lf() {
        let lines: Vec<&[u8]> = split_lines(b"a\nbb\nccc", Eol::Lf).collect();
        assert_eq!(lines, vec![b"a".as_ref(), b"bb".as_ref(), b"ccc".as_ref()]);
    }

    #[test]
    fn split_crlf() {
        let lines: Vec<&[u8]> = split_lines(b"x\r\nyy\r\n", Eol::Crlf).collect();
        assert_eq!(lines, vec![b"x".as_ref(), b"yy".as_ref()]);
    }

    #[test]
    fn decode_utf8_passthrough() {
        let (s, err) = decode("hello".as_bytes(), "utf-8");
        assert_eq!(s, "hello");
        assert!(!err);
    }

    #[test]
    fn decode_shift_jis() {
        // "あ" in Shift_JIS = 0x82 0xA0
        let (s, err) = decode(&[0x82, 0xA0], "shift_jis");
        assert_eq!(s, "あ");
        assert!(!err);
    }

    #[test]
    fn decode_unknown_label_falls_back_to_utf8() {
        let (s, _) = decode(b"hi", "no-such-encoding");
        assert_eq!(s, "hi");
    }
}
