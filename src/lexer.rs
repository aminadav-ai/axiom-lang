use crate::diag::{AxiomDiagnostic, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Export,
    Fn,
    Let,
    Mut,
    While,
    Max,
    If,
    Else,
    Do,
    Req,
    Ens,
    True,
    False,
    SecretKw,
    RevealKw,
    MemLoadKw,
    MemStoreKw,

    // Identifiers & Literals
    Ident(String),
    HashRef(String),
    Int(i64),
    Str(String),

    // Punctuation & Arrows
    LParen,
    RParen,
    LBrace,
    RBrace,
    EffectOpen,  // -[
    EffectClose, // ]->
    Arrow,       // ->
    Colon,
    Comma,
    Semi,
    Dot,
    Assign, // =

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Shl,
    Shr,
    Bang,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,

    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, AxiomDiagnostic> {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut pos = 0usize;
    let mut line = 1usize;
    let mut col = 1usize;
    let mut tokens = Vec::new();

    while pos < len {
        let ch = src[pos..].chars().next().unwrap();
        let ch_len = ch.len_utf8();

        if ch.is_whitespace() {
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            pos += ch_len;
            continue;
        }

        if src[pos..].starts_with("//") {
            while pos < len && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }

        let start = pos;
        let start_line = line;
        let start_col = col;

        // 3-char tokens
        if src[pos..].starts_with("]->") {
            pos += 3;
            col += 3;
            tokens.push(Token {
                kind: TokenKind::EffectClose,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }

        // 2-char tokens
        let two_char = if src[pos..].starts_with("-[") {
            Some(TokenKind::EffectOpen)
        } else if src[pos..].starts_with("->") {
            Some(TokenKind::Arrow)
        } else if src[pos..].starts_with("==") {
            Some(TokenKind::EqEq)
        } else if src[pos..].starts_with("!=") {
            Some(TokenKind::NotEq)
        } else if src[pos..].starts_with("<<") {
            Some(TokenKind::Shl)
        } else if src[pos..].starts_with(">>") {
            Some(TokenKind::Shr)
        } else if src[pos..].starts_with("<=") {
            Some(TokenKind::Le)
        } else if src[pos..].starts_with(">=") {
            Some(TokenKind::Ge)
        } else if src[pos..].starts_with("&&") {
            Some(TokenKind::AndAnd)
        } else if src[pos..].starts_with("||") {
            Some(TokenKind::OrOr)
        } else {
            None
        };

        if let Some(kind) = two_char {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }

        // Hash literal: #<hex>
        if ch == '#' {
            pos += 1;
            col += 1;
            let hex_start = pos;
            while pos < len && bytes[pos].is_ascii_hexdigit() {
                pos += 1;
                col += 1;
            }
            let hex = &src[hex_start..pos];
            if hex.len() < 6 {
                return Err(AxiomDiagnostic::new(
                    "E001_INVALID_HASH_REF",
                    "lex",
                    format!("Hash reference '#{hex}' must have at least 6 hex digits"),
                    Span {
                        start,
                        end: pos,
                        line: start_line,
                        col: start_col,
                    },
                    "Provide a valid node hash prefix of 6..64 lowercase hex digits (e.g. #8f3a12)",
                ));
            }
            tokens.push(Token {
                kind: TokenKind::HashRef(hex.to_ascii_lowercase()),
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }

        // String literal
        if ch == '"' {
            pos += 1;
            col += 1;
            let mut buf = String::new();
            let mut closed = false;
            while pos < len {
                let c = src[pos..].chars().next().unwrap();
                let clen = c.len_utf8();
                pos += clen;
                if c == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                if c == '"' {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    if pos >= len {
                        break;
                    }
                    let esc = src[pos..].chars().next().unwrap();
                    pos += esc.len_utf8();
                    col += 1;
                    match esc {
                        'n' => buf.push('\n'),
                        't' => buf.push('\t'),
                        'r' => buf.push('\r'),
                        '"' => buf.push('"'),
                        '\\' => buf.push('\\'),
                        other => buf.push(other),
                    }
                } else {
                    buf.push(c);
                }
            }
            if !closed {
                return Err(AxiomDiagnostic::new(
                    "E002_UNTERMINATED_STRING",
                    "lex",
                    "Unterminated string literal",
                    Span {
                        start,
                        end: pos,
                        line: start_line,
                        col: start_col,
                    },
                    "Close the string literal with a matching double quote '\"'",
                ));
            }
            tokens.push(Token {
                kind: TokenKind::Str(buf),
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }

        // Hex or decimal integer literal (supports 0xFF228833 and underscore separators 10_000)
        if ch.is_ascii_digit() {
            if src[pos..].starts_with("0x") || src[pos..].starts_with("0X") {
                pos += 2;
                col += 2;
                let hex_start = pos;
                while pos < len && (bytes[pos].is_ascii_hexdigit() || bytes[pos] == b'_') {
                    pos += 1;
                    col += 1;
                }
                let clean: String = src[hex_start..pos]
                    .chars()
                    .filter(|&c| c != '_')
                    .collect();
                let uval = u64::from_str_radix(&clean, 16).map_err(|_| {
                    AxiomDiagnostic::new(
                        "E003_INT_OVERFLOW",
                        "lex",
                        format!("Invalid or overflowing hex literal '0x{clean}'"),
                        Span {
                            start,
                            end: pos,
                            line: start_line,
                            col: start_col,
                        },
                        "Use a valid 64-bit hexadecimal literal (e.g. 0xFF228844)",
                    )
                })?;
                tokens.push(Token {
                    kind: TokenKind::Int(uval as i64),
                    span: Span {
                        start,
                        end: pos,
                        line: start_line,
                        col: start_col,
                    },
                });
                continue;
            }

            while pos < len && (bytes[pos].is_ascii_digit() || bytes[pos] == b'_') {
                pos += 1;
                col += 1;
            }
            let clean: String = src[start..pos].chars().filter(|&c| c != '_').collect();
            let val = clean.parse::<i64>().map_err(|_| {
                AxiomDiagnostic::new(
                    "E003_INT_OVERFLOW",
                    "lex",
                    format!("Integer literal '{}' overflows i64", &src[start..pos]),
                    Span {
                        start,
                        end: pos,
                        line: start_line,
                        col: start_col,
                    },
                    "Use an integer within the signed 64-bit range",
                )
            })?;
            tokens.push(Token {
                kind: TokenKind::Int(val),
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }

        // Identifiers & Keywords
        if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
            pos += 1;
            col += 1;
            while pos < len && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
                pos += 1;
                col += 1;
            }
            let word = &src[start..pos];
            let kind = match word {
                "export" => TokenKind::Export,
                "fn" => TokenKind::Fn,
                "let" => TokenKind::Let,
                "mut" => TokenKind::Mut,
                "while" => TokenKind::While,
                "max" => TokenKind::Max,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "do" => TokenKind::Do,
                "req" => TokenKind::Req,
                "ens" => TokenKind::Ens,
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "secret" => TokenKind::SecretKw,
                "reveal" => TokenKind::RevealKw,
                "mem_load" => TokenKind::MemLoadKw,
                "mem_store" => TokenKind::MemStoreKw,
                _ => TokenKind::Ident(word.to_string()),
            };
            tokens.push(Token {
                kind,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }

        // Single-char punctuation
        let single = match ch {
            '(' => Some(TokenKind::LParen),
            ')' => Some(TokenKind::RParen),
            '{' => Some(TokenKind::LBrace),
            '}' => Some(TokenKind::RBrace),
            ':' => Some(TokenKind::Colon),
            ',' => Some(TokenKind::Comma),
            ';' => Some(TokenKind::Semi),
            '.' => Some(TokenKind::Dot),
            '=' => Some(TokenKind::Assign),
            '+' => Some(TokenKind::Plus),
            '-' => Some(TokenKind::Minus),
            '*' => Some(TokenKind::Star),
            '/' => Some(TokenKind::Slash),
            '%' => Some(TokenKind::Percent),
            '&' => Some(TokenKind::Amp),
            '|' => Some(TokenKind::Pipe),
            '^' => Some(TokenKind::Caret),
            '!' => Some(TokenKind::Bang),
            '<' => Some(TokenKind::Lt),
            '>' => Some(TokenKind::Gt),
            _ => None,
        };

        if let Some(kind) = single {
            pos += ch_len;
            col += 1;
            tokens.push(Token {
                kind,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }

        return Err(AxiomDiagnostic::new(
            "E004_UNEXPECTED_CHAR",
            "lex",
            format!("Unexpected character '{ch}'"),
            Span {
                start,
                end: pos + ch_len,
                line: start_line,
                col: start_col,
            },
            "Remove invalid character or check Axiom token syntax",
        ));
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span {
            start: len,
            end: len,
            line,
            col,
        },
    });
    Ok(tokens)
}
