use crate::diag::{AxiomDiagnostic, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Fn,
    Let,
    If,
    Else,
    Do,
    Req,
    Ens,
    True,
    False,
    SecretKw, // secret(...)
    RevealKw, // reveal(...)

    // Identifiers & Literals
    Ident(String),
    HashRef(String), // #8f3a12...
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

        // Whitespace
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

        // Single-line comments: // ...
        if src[pos..].starts_with("//") {
            while pos < len && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }

        let start = pos;
        let start_line = line;
        let start_col = col;

        // Multi-char tokens
        if src[pos..].starts_with("-[") {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind: TokenKind::EffectOpen,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }
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
        if src[pos..].starts_with("->") {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind: TokenKind::Arrow,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }
        if src[pos..].starts_with("==") {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind: TokenKind::EqEq,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }
        if src[pos..].starts_with("!=") {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind: TokenKind::NotEq,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }
        if src[pos..].starts_with("<=") {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind: TokenKind::Le,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }
        if src[pos..].starts_with(">=") {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind: TokenKind::Ge,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }
        if src[pos..].starts_with("&&") {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind: TokenKind::AndAnd,
                span: Span {
                    start,
                    end: pos,
                    line: start_line,
                    col: start_col,
                },
            });
            continue;
        }
        if src[pos..].starts_with("||") {
            pos += 2;
            col += 2;
            tokens.push(Token {
                kind: TokenKind::OrOr,
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

        // Integer literal
        if ch.is_ascii_digit() {
            while pos < len && bytes[pos].is_ascii_digit() {
                pos += 1;
                col += 1;
            }
            let num_str = &src[start..pos];
            let val = num_str.parse::<i64>().map_err(|_| {
                AxiomDiagnostic::new(
                    "E003_INT_OVERFLOW",
                    "lex",
                    format!("Integer literal '{num_str}' overflows i64"),
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

        // Identifiers & Keywords (also allows $result / $self in canonical round-trips)
        if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
            pos += 1;
            col += 1;
            while pos < len && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
                pos += 1;
                col += 1;
            }
            let word = &src[start..pos];
            let kind = match word {
                "fn" => TokenKind::Fn,
                "let" => TokenKind::Let,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "do" => TokenKind::Do,
                "req" => TokenKind::Req,
                "ens" => TokenKind::Ens,
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "secret" => TokenKind::SecretKw,
                "reveal" => TokenKind::RevealKw,
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
