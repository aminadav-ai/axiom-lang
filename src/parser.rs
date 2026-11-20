use crate::ast::{
    BinOp, ConstDef, ConstValue, EffectCap, EnumDef, EnumVariant, Expr, FnDef, LayoutDef,
    LayoutField, Module, Param, Stmt, TargetRef, Type, UnaryOp,
};
use crate::diag::{AxiomDiagnostic, Span};
use crate::lexer::{tokenize, Token, TokenKind};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Internal Raw AST (pre-resolution / pre-desugaring)
// ---------------------------------------------------------------------------

struct RawUse {
    path: String,
    span: Span,
}

struct RawConst {
    name: String,
    ty: Type,
    expr: RawExpr,
    span: Span,
}

struct RawEnumVariant {
    name: String,
    value: Option<RawExpr>,
    span: Span,
}

struct RawEnum {
    name: String,
    variants: Vec<RawEnumVariant>,
    span: Span,
}

struct RawLayoutField {
    name: String,
    len_expr: Option<RawExpr>,
    is_array: bool,
    at_expr: Option<RawExpr>,
    span: Span,
}

struct RawLayout {
    name: String,
    base_expr: Option<RawExpr>,
    fields: Vec<RawLayoutField>,
    span: Span,
}

enum RawItem {
    Const(RawConst),
    Enum(RawEnum),
    Layout(RawLayout),
}

enum RawMatchPattern {
    Wildcard(Span),
    Int(i64),
    Bool(bool),
    ConstRef(String, Span),
    Path2(String, String, Span),
    Path3(String, String, String, Span),
}

struct RawMatchArm {
    patterns: Vec<RawMatchPattern>,
    body: RawExpr,
}

enum RawStmt {
    Let {
        name: String,
        mutable: bool,
        ty: Option<Type>,
        value: RawExpr,
    },
    Assign {
        name: String,
        op: Option<BinOp>,
        value: RawExpr,
    },
    LayoutStore {
        layout: String,
        field: String,
        op: Option<BinOp>,
        value: RawExpr,
        span: Span,
    },
    While {
        cond: RawExpr,
        max_iters: RawExpr,
        body: Vec<RawStmt>,
        span: Span,
    },
    Expr(RawExpr),
}

enum RawExpr {
    IntLit(i64),
    BoolLit(bool),
    StrLit(String),
    UnitLit,
    Var(String),
    Path2 {
        head: String,
        member: String,
        span: Span,
    },
    Path3 {
        head: String,
        member: String,
        prop: String,
        span: Span,
    },
    LayoutLoad {
        layout: String,
        field: String,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<RawExpr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<RawExpr>,
        rhs: Box<RawExpr>,
    },
    If {
        cond: Box<RawExpr>,
        then_branch: Box<RawExpr>,
        else_branch: Box<RawExpr>,
    },
    Match {
        scrutinee: Box<RawExpr>,
        arms: Vec<RawMatchArm>,
        span: Span,
    },
    Call {
        target: TargetRef,
        args: Vec<RawExpr>,
    },
    Perform {
        effect: String,
        op: String,
        args: Vec<RawExpr>,
    },
    MemLoad {
        index: Box<RawExpr>,
    },
    MemStore {
        index: Box<RawExpr>,
        value: Box<RawExpr>,
    },
    SecretWrap(Box<RawExpr>),
    Reveal {
        domain: String,
        expr: Box<RawExpr>,
    },
    Block {
        stmts: Vec<RawStmt>,
        tail: Box<RawExpr>,
    },
}

struct RawFnDef {
    name: String,
    exported: bool,
    params: Vec<Param>,
    effects: Vec<EffectCap>,
    ret_type: Type,
    requires: Vec<RawExpr>,
    ensures: Vec<RawExpr>,
    body: RawExpr,
}

struct RawFile {
    uses: Vec<RawUse>,
    items: Vec<RawItem>,
    functions: Vec<RawFnDef>,
}

// ---------------------------------------------------------------------------
// Public Entry Points
// ---------------------------------------------------------------------------

pub fn parse_module(src: &str) -> Result<Module, AxiomDiagnostic> {
    let raw = parse_raw_file(src)?;
    if let Some(u) = raw.uses.first() {
        return Err(AxiomDiagnostic::new(
            "E018_USE_WITHOUT_PATH",
            "parse",
            format!(
                "Cannot resolve 'use \"{}\"' without a source file path",
                u.path
            ),
            u.span,
            "Load multi-file modules with parse_module_from_path(entry_path) or CLI file commands",
        ));
    }
    resolve_and_lower(raw.items, raw.functions)
}

/// Load and parse a multi-file Axiom module starting at `entry_path`.
///
/// Resolves `use "relative/path.ax";` directives recursively relative to each file's directory,
/// deduplicates diamond imports by canonical path, detects import cycles, and collects all
/// declarations in deterministic topological order (dependencies before dependents).
pub fn parse_module_from_path(entry_path: &Path) -> Result<Module, AxiomDiagnostic> {
    let mut visiting: Vec<PathBuf> = Vec::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut all_items: Vec<RawItem> = Vec::new();
    let mut all_fns: Vec<RawFnDef> = Vec::new();

    load_file_recursive(
        entry_path,
        Span::zero(),
        &mut visiting,
        &mut visited,
        &mut all_items,
        &mut all_fns,
    )?;

    resolve_and_lower(all_items, all_fns)
}

fn load_file_recursive(
    path: &Path,
    use_span: Span,
    visiting: &mut Vec<PathBuf>,
    visited: &mut HashSet<PathBuf>,
    all_items: &mut Vec<RawItem>,
    all_fns: &mut Vec<RawFnDef>,
) -> Result<(), AxiomDiagnostic> {
    let canon = fs::canonicalize(path).map_err(|e| {
        AxiomDiagnostic::new(
            "E040_FILE_READ",
            "parse",
            format!("Cannot resolve Axiom source file '{}': {e}", path.display()),
            use_span,
            "Provide a valid path to an .ax source file",
        )
    })?;

    if let Some(cycle_start) = visiting.iter().position(|p| p == &canon) {
        let mut chain: Vec<String> = visiting[cycle_start..]
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        chain.push(canon.display().to_string());
        return Err(AxiomDiagnostic::new(
            "E019_IMPORT_CYCLE",
            "parse",
            format!("Circular 'use' import detected: {}", chain.join(" -> ")),
            use_span,
            "Break the import cycle by moving shared declarations into a common leaf .ax file",
        ));
    }

    if visited.contains(&canon) {
        return Ok(());
    }

    let src = fs::read_to_string(&canon).map_err(|e| {
        AxiomDiagnostic::new(
            "E040_FILE_READ",
            "parse",
            format!("Cannot read '{}': {e}", canon.display()),
            use_span,
            "Check file permissions and ensure the .ax file is valid UTF-8",
        )
    })?;

    let raw = parse_raw_file(&src)?;
    let base_dir = canon.parent().unwrap_or_else(|| Path::new("."));

    visiting.push(canon.clone());
    for u in &raw.uses {
        let dep_path = base_dir.join(&u.path);
        load_file_recursive(&dep_path, u.span, visiting, visited, all_items, all_fns)?;
    }
    visiting.pop();
    visited.insert(canon);

    all_items.extend(raw.items);
    all_fns.extend(raw.functions);
    Ok(())
}

fn parse_raw_file(src: &str) -> Result<RawFile, AxiomDiagnostic> {
    let tokens = tokenize(src)?;
    let mut parser = Parser { tokens, idx: 0 };
    let mut uses = Vec::new();
    let mut items = Vec::new();
    let mut functions = Vec::new();

    while !parser.at_eof() {
        match parser.peek().kind {
            TokenKind::Use => uses.push(parser.parse_use()?),
            TokenKind::Const => items.push(RawItem::Const(parser.parse_const()?)),
            TokenKind::Enum => items.push(RawItem::Enum(parser.parse_enum()?)),
            TokenKind::Layout => items.push(RawItem::Layout(parser.parse_layout()?)),
            _ => functions.push(parser.parse_fn()?),
        }
    }

    Ok(RawFile {
        uses,
        items,
        functions,
    })
}

// ---------------------------------------------------------------------------
// Recursive-Descent Parser
// ---------------------------------------------------------------------------

pub struct Parser {
    tokens: Vec<Token>,
    idx: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.idx]
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        let pos = self.idx + offset;
        if pos < self.tokens.len() {
            &self.tokens[pos].kind
        } else {
            &TokenKind::Eof
        }
    }

    fn peek_next(&self) -> &TokenKind {
        self.peek_at(1)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.idx].clone();
        if self.idx + 1 < self.tokens.len() {
            self.idx += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &TokenKind, hint: &str) -> Result<Token, AxiomDiagnostic> {
        if &self.peek().kind == expected {
            Ok(self.advance())
        } else {
            Err(AxiomDiagnostic::new(
                "E010_UNEXPECTED_TOKEN",
                "parse",
                format!("Expected {:?}, found {:?}", expected, self.peek().kind),
                self.peek().span,
                hint,
            ))
        }
    }

    fn expect_ident(&mut self, hint: &str) -> Result<(String, Span), AxiomDiagnostic> {
        let tok = self.peek().clone();
        if let TokenKind::Ident(name) = tok.kind {
            self.advance();
            Ok((name, tok.span))
        } else {
            Err(AxiomDiagnostic::new(
                "E011_EXPECTED_IDENT",
                "parse",
                format!("Expected identifier, found {:?}", tok.kind),
                tok.span,
                hint,
            ))
        }
    }

    fn peek_is_contextual_kw(&self, kw: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(s) if s == kw)
    }

    fn parse_use(&mut self) -> Result<RawUse, AxiomDiagnostic> {
        let tok = self.expect(&TokenKind::Use, "Expected 'use'")?;
        let path = match self.peek().kind.clone() {
            TokenKind::Str(s) => {
                self.advance();
                s
            }
            other => {
                return Err(AxiomDiagnostic::new(
                    "E010_UNEXPECTED_TOKEN",
                    "parse",
                    format!("Expected relative file path string after 'use', found {other:?}"),
                    self.peek().span,
                    "Write import directives as: use \"relative/path.ax\";",
                ));
            }
        };
        self.expect(&TokenKind::Semi, "Terminate 'use' directive with ';'")?;
        Ok(RawUse {
            path,
            span: tok.span,
        })
    }

    fn parse_const(&mut self) -> Result<RawConst, AxiomDiagnostic> {
        let tok = self.expect(&TokenKind::Const, "Expected 'const'")?;
        let (name, _) = self.expect_ident("Expected constant name after 'const'")?;
        self.expect(&TokenKind::Colon, "Expected ':' and type after constant name")?;
        let ty = self.parse_type()?;
        if !matches!(ty, Type::I64 | Type::Bool) {
            return Err(AxiomDiagnostic::new(
                "E064_CONST_EVAL",
                "parse",
                format!("Constant '{name}' has unsupported type '{ty}'; only 'i64' and 'bool' are allowed"),
                tok.span,
                "Declare constants as 'const NAME: i64 = ...;' or 'const FLAG: bool = ...;'",
            ));
        }
        self.expect(&TokenKind::Assign, "Expected '=' in const declaration")?;
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::Semi, "Terminate 'const' declaration with ';'")?;
        Ok(RawConst {
            name,
            ty,
            expr,
            span: tok.span,
        })
    }

    fn parse_enum(&mut self) -> Result<RawEnum, AxiomDiagnostic> {
        let tok = self.expect(&TokenKind::Enum, "Expected 'enum'")?;
        let (name, _) = self.expect_ident("Expected enum name after 'enum'")?;
        self.expect(&TokenKind::LBrace, "Expected '{' to open enum body")?;

        let mut variants = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RBrace | TokenKind::Eof) {
            let (vname, vspan) = self.expect_ident("Expected enum variant name")?;
            let value = if matches!(self.peek().kind, TokenKind::Assign) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            variants.push(RawEnumVariant {
                name: vname,
                value,
                span: vspan,
            });
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "Expected '}' to close enum body")?;
        Ok(RawEnum {
            name,
            variants,
            span: tok.span,
        })
    }

    fn parse_layout(&mut self) -> Result<RawLayout, AxiomDiagnostic> {
        let tok = self.expect(&TokenKind::Layout, "Expected 'layout'")?;
        let (name, _) = self.expect_ident("Expected layout name after 'layout'")?;
        let base_expr = if self.peek_is_contextual_kw("at") {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&TokenKind::LBrace, "Expected '{' to open layout body")?;

        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RBrace | TokenKind::Eof) {
            let (fname, fspan) = self.expect_ident("Expected layout field name")?;
            self.expect(&TokenKind::Colon, "Expected ':' after layout field name")?;
            let (kind_kw, kind_span) =
                self.expect_ident("Expected 'word' or 'words[N]' for layout field type")?;
            let (is_array, len_expr) = match kind_kw.as_str() {
                "word" => (false, None),
                "words" => {
                    self.expect(
                        &TokenKind::LBracket,
                        "Expected '[' after 'words' (e.g. words[8200])",
                    )?;
                    let len = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket, "Expected ']' after words length")?;
                    (true, Some(len))
                }
                other => {
                    return Err(AxiomDiagnostic::new(
                        "E010_UNEXPECTED_TOKEN",
                        "parse",
                        format!("Unknown layout field type '{other}'; expected 'word' or 'words[N]'"),
                        kind_span,
                        "Use 'field: word' for a single 32-bit word slot or 'buf: words[N]' for a buffer",
                    ));
                }
            };
            let at_expr = if self.peek_is_contextual_kw("at") {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            fields.push(RawLayoutField {
                name: fname,
                len_expr,
                is_array,
                at_expr,
                span: fspan,
            });
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "Expected '}' to close layout body")?;
        Ok(RawLayout {
            name,
            base_expr,
            fields,
            span: tok.span,
        })
    }

    fn parse_fn(&mut self) -> Result<RawFnDef, AxiomDiagnostic> {
        let exported = if matches!(self.peek().kind, TokenKind::Export) {
            self.advance();
            true
        } else {
            false
        };

        self.expect(
            &TokenKind::Fn,
            "Start each Axiom node definition with 'fn <name>(...) -> <type> { ... }' or 'export fn ...'",
        )?;
        let (name, _) = self.expect_ident("Provide a function alias name after 'fn'")?;
        self.expect(&TokenKind::LParen, "Expected '(' to start parameter list")?;

        let mut params = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                let (pname, _) = self.expect_ident("Expected parameter name")?;
                self.expect(&TokenKind::Colon, "Expected ':' after parameter name")?;
                let pty = self.parse_type()?;
                params.push(Param {
                    name: pname,
                    ty: pty,
                });
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "Expected ')' after parameter list")?;

        let mut effects = Vec::new();
        match &self.peek().kind {
            TokenKind::Arrow => {
                self.advance();
            }
            TokenKind::EffectOpen => {
                self.advance();
                if !matches!(self.peek().kind, TokenKind::EffectClose) {
                    loop {
                        let (ename, _) =
                            self.expect_ident("Expected effect capability name inside '-[...]->'")?;
                        let scope = if matches!(self.peek().kind, TokenKind::LParen) {
                            self.advance();
                            let s = match self.peek().kind.clone() {
                                TokenKind::Str(val) => {
                                    self.advance();
                                    val
                                }
                                other => {
                                    return Err(AxiomDiagnostic::new(
                                        "E012_EXPECTED_SCOPE_STR",
                                        "parse",
                                        format!("Expected string literal scope, found {other:?}"),
                                        self.peek().span,
                                        "Write capability scope as a string literal, e.g. Fs(\"/tmp\")",
                                    ));
                                }
                            };
                            self.expect(
                                &TokenKind::RParen,
                                "Expected ')' after capability scope literal",
                            )?;
                            Some(s)
                        } else {
                            None
                        };
                        effects.push(EffectCap { name: ename, scope });
                        if matches!(self.peek().kind, TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(
                    &TokenKind::EffectClose,
                    "Close capability list with ']->' before return type",
                )?;
            }
            other => {
                return Err(AxiomDiagnostic::new(
                    "E013_EXPECTED_RETURN_ARROW",
                    "parse",
                    format!("Expected '->' or '-[...]->', found {other:?}"),
                    self.peek().span,
                    "Specify return type with '-> <Type>' or effects + return type with '-[Mem]-> <Type>'",
                ));
            }
        }

        let ret_type = self.parse_type()?;

        let mut requires = Vec::new();
        let mut ensures = Vec::new();
        loop {
            match self.peek().kind {
                TokenKind::Req => {
                    self.advance();
                    requires.push(self.parse_expr()?);
                }
                TokenKind::Ens => {
                    self.advance();
                    ensures.push(self.parse_expr()?);
                }
                _ => break,
            }
        }

        let body = self.parse_block_expr()?;

        Ok(RawFnDef {
            name,
            exported,
            params,
            effects,
            ret_type,
            requires,
            ensures,
            body,
        })
    }

    fn parse_type(&mut self) -> Result<Type, AxiomDiagnostic> {
        let (tname, span) =
            self.expect_ident("Expected type name (i64, bool, str, unit, Secret<T>)")?;
        match tname.as_str() {
            "i64" => Ok(Type::I64),
            "bool" => Ok(Type::Bool),
            "str" | "String" => Ok(Type::Str),
            "unit" | "Unit" => Ok(Type::Unit),
            "Secret" => {
                self.expect(&TokenKind::Lt, "Expected '<' after Secret")?;
                let inner = self.parse_type()?;
                self.expect(&TokenKind::Gt, "Expected '>' to close Secret<T>")?;
                Ok(Type::Secret(Box::new(inner)))
            }
            other => Err(AxiomDiagnostic::new(
                "E014_UNKNOWN_TYPE",
                "parse",
                format!("Unknown type '{other}'"),
                span,
                "Use one of the built-in Axiom types: i64, bool, str, unit, or Secret<T>",
            )),
        }
    }

    fn assign_op_for(kind: &TokenKind) -> Option<Option<BinOp>> {
        match kind {
            TokenKind::Assign => Some(None),
            TokenKind::PlusEq => Some(Some(BinOp::Add)),
            TokenKind::MinusEq => Some(Some(BinOp::Sub)),
            TokenKind::StarEq => Some(Some(BinOp::Mul)),
            TokenKind::PipeEq => Some(Some(BinOp::BitOr)),
            TokenKind::AmpEq => Some(Some(BinOp::BitAnd)),
            _ => None,
        }
    }

    fn try_parse_assign_stmt(&mut self) -> Result<Option<RawStmt>, AxiomDiagnostic> {
        // Variable assignment: `x = e;` or `x += e;` / `-=` / `*=` / `|=` / `&=`
        if matches!(&self.peek().kind, TokenKind::Ident(_)) {
            if let Some(op) = Self::assign_op_for(self.peek_next()) {
                let (vname, _) = self.expect_ident("Expected variable name")?;
                self.advance(); // consume assignment operator
                let val = self.parse_expr()?;
                self.expect(&TokenKind::Semi, "Terminate assignment statement with ';'")?;
                return Ok(Some(RawStmt::Assign {
                    name: vname,
                    op,
                    value: val,
                }));
            }
            // Layout word-field assignment: `Layout.field = e;` or `Layout.field += e;`
            if matches!(self.peek_at(1), TokenKind::Dot)
                && matches!(self.peek_at(2), TokenKind::Ident(_))
            {
                if let Some(op) = Self::assign_op_for(self.peek_at(3)) {
                    let (layout, span) = self.expect_ident("Expected layout name")?;
                    self.advance(); // '.'
                    let (field, _) = self.expect_ident("Expected layout field name")?;
                    self.advance(); // assignment op
                    let val = self.parse_expr()?;
                    self.expect(
                        &TokenKind::Semi,
                        "Terminate layout field assignment with ';'",
                    )?;
                    return Ok(Some(RawStmt::LayoutStore {
                        layout,
                        field,
                        op,
                        value: val,
                        span,
                    }));
                }
            }
        }
        Ok(None)
    }

    fn parse_stmt_list_block(&mut self) -> Result<Vec<RawStmt>, AxiomDiagnostic> {
        self.expect(&TokenKind::LBrace, "Expected '{' to start loop body")?;
        let mut stmts = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RBrace | TokenKind::Eof) {
            stmts.push(self.parse_single_stmt_in_loop()?);
        }
        self.expect(&TokenKind::RBrace, "Expected '}' to close loop body")?;
        Ok(stmts)
    }

    fn parse_single_stmt_in_loop(&mut self) -> Result<RawStmt, AxiomDiagnostic> {
        if matches!(self.peek().kind, TokenKind::Let) {
            return self.parse_let_stmt();
        }
        if matches!(self.peek().kind, TokenKind::While) {
            return self.parse_while_stmt();
        }
        if let Some(assign) = self.try_parse_assign_stmt()? {
            return Ok(assign);
        }
        let expr = self.parse_expr()?;
        if matches!(self.peek().kind, TokenKind::Semi) {
            self.advance();
        } else if !matches!(
            expr,
            RawExpr::If { .. } | RawExpr::Match { .. } | RawExpr::Block { .. }
        ) {
            self.expect(&TokenKind::Semi, "Terminate statement inside loop with ';'")?;
        }
        Ok(RawStmt::Expr(expr))
    }

    fn parse_let_stmt(&mut self) -> Result<RawStmt, AxiomDiagnostic> {
        self.expect(&TokenKind::Let, "Expected 'let'")?;
        let mutable = if matches!(self.peek().kind, TokenKind::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let (vname, _) = self.expect_ident("Expected variable name after 'let'")?;
        let ty = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Assign, "Expected '=' in let binding")?;
        let val = self.parse_expr()?;
        self.expect(&TokenKind::Semi, "Terminate 'let' binding with ';'")?;
        Ok(RawStmt::Let {
            name: vname,
            mutable,
            ty,
            value: val,
        })
    }

    fn parse_const_atom(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Int(v) => {
                self.advance();
                Ok(RawExpr::IntLit(v))
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after constant expression")?;
                Ok(inner)
            }
            TokenKind::Ident(name) => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::ColonColon) {
                    self.advance();
                    let (member, _) =
                        self.expect_ident("Expected member name after '::'")?;
                    if matches!(self.peek().kind, TokenKind::ColonColon) {
                        self.advance();
                        let (prop, _) = self.expect_ident(
                            "Expected layout field property ('len' or 'end') after '::'",
                        )?;
                        Ok(RawExpr::Path3 {
                            head: name,
                            member,
                            prop,
                            span: tok.span,
                        })
                    } else {
                        Ok(RawExpr::Path2 {
                            head: name,
                            member,
                            span: tok.span,
                        })
                    }
                } else {
                    Ok(RawExpr::Var(name))
                }
            }
            other => Err(AxiomDiagnostic::new(
                "E017_INVALID_LOOP_BOUND",
                "parse",
                format!("Expected positive integer constant after 'max', found {other:?}"),
                tok.span,
                "Provide a positive compile-time iteration bound (e.g. 'while i < n max 4096 { ... }' or 'max MAX_ITERS')",
            )),
        }
    }

    fn parse_while_stmt(&mut self) -> Result<RawStmt, AxiomDiagnostic> {
        let w_tok = self.expect(&TokenKind::While, "Expected 'while'")?;
        let cond = self.parse_expr()?;
        self.expect(
            &TokenKind::Max,
            "Every 'while' loop in Axiom.Strata must declare an explicit upper bound: 'while <cond> max <N> { ... }'",
        )?;
        let max_iters = self.parse_const_atom()?;
        let body = self.parse_stmt_list_block()?;
        if matches!(self.peek().kind, TokenKind::Semi) {
            self.advance();
        }
        Ok(RawStmt::While {
            cond,
            max_iters,
            body,
            span: w_tok.span,
        })
    }

    fn parse_block_expr(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        self.expect(&TokenKind::LBrace, "Expected '{' to open block")?;
        let mut stmts = Vec::new();

        while !matches!(self.peek().kind, TokenKind::RBrace | TokenKind::Eof) {
            if matches!(self.peek().kind, TokenKind::Let) {
                stmts.push(self.parse_let_stmt()?);
                continue;
            }
            if matches!(self.peek().kind, TokenKind::While) {
                stmts.push(self.parse_while_stmt()?);
                continue;
            }
            if let Some(assign) = self.try_parse_assign_stmt()? {
                stmts.push(assign);
                continue;
            }

            let expr = self.parse_expr()?;
            if matches!(self.peek().kind, TokenKind::Semi) {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RBrace) {
                    self.advance();
                    stmts.push(RawStmt::Expr(expr));
                    return Ok(RawExpr::Block {
                        stmts,
                        tail: Box::new(RawExpr::UnitLit),
                    });
                }
                stmts.push(RawStmt::Expr(expr));
            } else if matches!(expr, RawExpr::If { .. } | RawExpr::Match { .. })
                && !matches!(self.peek().kind, TokenKind::RBrace)
            {
                // Allow `if ... { ... }` and `match ... { ... }` statements without ';' before next statement
                stmts.push(RawStmt::Expr(expr));
            } else {
                self.expect(
                    &TokenKind::RBrace,
                    "Expected '}' after block tail expression (or ';' before next statement)",
                )?;
                if stmts.is_empty() {
                    return Ok(expr);
                }
                return Ok(RawExpr::Block {
                    stmts,
                    tail: Box::new(expr),
                });
            }
        }

        self.expect(&TokenKind::RBrace, "Expected '}' to close block")?;
        Ok(RawExpr::Block {
            stmts,
            tail: Box::new(RawExpr::UnitLit),
        })
    }

    fn parse_expr(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek().kind, TokenKind::OrOr) {
            self.advance();
            let rhs = self.parse_and()?;
            lhs = RawExpr::Binary {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_bitor()?;
        while matches!(self.peek().kind, TokenKind::AndAnd) {
            self.advance();
            let rhs = self.parse_bitor()?;
            lhs = RawExpr::Binary {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bitor(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_bitxor()?;
        while matches!(self.peek().kind, TokenKind::Pipe) {
            self.advance();
            let rhs = self.parse_bitxor()?;
            lhs = RawExpr::Binary {
                op: BinOp::BitOr,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_bitand()?;
        while matches!(self.peek().kind, TokenKind::Caret) {
            self.advance();
            let rhs = self.parse_bitand()?;
            lhs = RawExpr::Binary {
                op: BinOp::BitXor,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_eq()?;
        while matches!(self.peek().kind, TokenKind::Amp) {
            self.advance();
            let rhs = self.parse_eq()?;
            lhs = RawExpr::Binary {
                op: BinOp::BitAnd,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_eq(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_rel()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::NotEq => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_rel()?;
            lhs = RawExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_rel(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_shift()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Le => BinOp::Le,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_shift()?;
            lhs = RawExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_add()?;
            lhs = RawExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_mul()?;
            lhs = RawExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            lhs = RawExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        match self.peek().kind {
            TokenKind::Minus => {
                self.advance();
                let inner = self.parse_unary()?;
                Ok(RawExpr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(inner),
                })
            }
            TokenKind::Bang => {
                self.advance();
                let inner = self.parse_unary()?;
                Ok(RawExpr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(inner),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_if_expr(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        self.expect(&TokenKind::If, "Expected 'if'")?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block_expr()?;
        let else_branch = if matches!(self.peek().kind, TokenKind::Else) {
            self.advance();
            if matches!(self.peek().kind, TokenKind::If) {
                self.parse_if_expr()?
            } else {
                self.parse_block_expr()?
            }
        } else {
            RawExpr::UnitLit
        };
        Ok(RawExpr::If {
            cond: Box::new(cond),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        })
    }

    fn parse_match_expr(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let mtok = self.expect(&TokenKind::Match, "Expected 'match'")?;
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::LBrace, "Expected '{' to open match arms")?;

        let mut arms = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RBrace | TokenKind::Eof) {
            let mut patterns = vec![self.parse_match_pattern()?];
            while matches!(self.peek().kind, TokenKind::Pipe) {
                self.advance();
                patterns.push(self.parse_match_pattern()?);
            }
            self.expect(&TokenKind::FatArrow, "Expected '=>' after match pattern")?;
            let body = self.parse_expr()?;
            let is_block_like = matches!(
                body,
                RawExpr::Block { .. } | RawExpr::If { .. } | RawExpr::Match { .. }
            );
            arms.push(RawMatchArm { patterns, body });
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
            } else if !is_block_like && !matches!(self.peek().kind, TokenKind::RBrace) {
                self.expect(&TokenKind::Comma, "Expected ',' after match arm")?;
            }
        }
        self.expect(&TokenKind::RBrace, "Expected '}' to close match expression")?;
        Ok(RawExpr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
            span: mtok.span,
        })
    }

    fn parse_match_pattern(&mut self) -> Result<RawMatchPattern, AxiomDiagnostic> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Int(v) => {
                self.advance();
                Ok(RawMatchPattern::Int(v))
            }
            TokenKind::Minus => {
                self.advance();
                let next = self.peek().clone();
                if let TokenKind::Int(v) = next.kind {
                    self.advance();
                    Ok(RawMatchPattern::Int(v.wrapping_neg()))
                } else {
                    Err(AxiomDiagnostic::new(
                        "E010_UNEXPECTED_TOKEN",
                        "parse",
                        format!("Expected integer literal after '-' in match pattern, found {:?}", next.kind),
                        next.span,
                        "Use an integer literal (e.g. -1) in match patterns",
                    ))
                }
            }
            TokenKind::True => {
                self.advance();
                Ok(RawMatchPattern::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(RawMatchPattern::Bool(false))
            }
            TokenKind::Ident(name) => {
                self.advance();
                if name == "_" {
                    return Ok(RawMatchPattern::Wildcard(tok.span));
                }
                if matches!(self.peek().kind, TokenKind::ColonColon) {
                    self.advance();
                    let (member, _) = self.expect_ident("Expected variant or field name after '::'")?;
                    if matches!(self.peek().kind, TokenKind::ColonColon) {
                        self.advance();
                        let (prop, _) = self.expect_ident(
                            "Expected layout field property ('len' or 'end') after '::'",
                        )?;
                        Ok(RawMatchPattern::Path3(name, member, prop, tok.span))
                    } else {
                        Ok(RawMatchPattern::Path2(name, member, tok.span))
                    }
                } else {
                    Ok(RawMatchPattern::ConstRef(name, tok.span))
                }
            }
            other => Err(AxiomDiagnostic::new(
                "E010_UNEXPECTED_TOKEN",
                "parse",
                format!("Invalid match pattern starting with {other:?}"),
                tok.span,
                "Use an integer/char/bool literal, const name, Enum::Variant, or '_' wildcard",
            )),
        }
    }

    fn parse_primary(&mut self) -> Result<RawExpr, AxiomDiagnostic> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Int(v) => {
                self.advance();
                Ok(RawExpr::IntLit(v))
            }
            TokenKind::True => {
                self.advance();
                Ok(RawExpr::BoolLit(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(RawExpr::BoolLit(false))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(RawExpr::StrLit(s))
            }
            TokenKind::LBrace => self.parse_block_expr(),
            TokenKind::LParen => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RParen) {
                    self.advance();
                    return Ok(RawExpr::UnitLit);
                }
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after parenthesized expression")?;
                Ok(inner)
            }
            TokenKind::If => self.parse_if_expr(),
            TokenKind::Match => self.parse_match_expr(),
            TokenKind::Do => {
                self.advance();
                let (effect, _) = self.expect_ident(
                    "Expected capability name after 'do' (e.g. do Net.post(...))",
                )?;
                self.expect(&TokenKind::Dot, "Expected '.' between capability and operation")?;
                let (op, _) = self.expect_ident("Expected effect operation name after '.'")?;
                let args = self.parse_arg_list()?;
                Ok(RawExpr::Perform { effect, op, args })
            }
            TokenKind::MemLoadKw => {
                self.advance();
                self.expect(&TokenKind::LParen, "Expected '(' after 'mem_load'")?;
                let index = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after mem_load(index)")?;
                Ok(RawExpr::MemLoad {
                    index: Box::new(index),
                })
            }
            TokenKind::MemStoreKw => {
                self.advance();
                self.expect(&TokenKind::LParen, "Expected '(' after 'mem_store'")?;
                let index = self.parse_expr()?;
                self.expect(&TokenKind::Comma, "Expected ',' between mem_store index and value")?;
                let value = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after mem_store(index, value)")?;
                Ok(RawExpr::MemStore {
                    index: Box::new(index),
                    value: Box::new(value),
                })
            }
            TokenKind::SecretKw => {
                self.advance();
                self.expect(&TokenKind::LParen, "Expected '(' after 'secret'")?;
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after secret(expr)")?;
                Ok(RawExpr::SecretWrap(Box::new(inner)))
            }
            TokenKind::RevealKw => {
                self.advance();
                self.expect(&TokenKind::LParen, "Expected '(' after 'reveal'")?;
                let domain = match self.peek().kind.clone() {
                    TokenKind::Str(s) => {
                        self.advance();
                        s
                    }
                    other => {
                        return Err(AxiomDiagnostic::new(
                            "E015_EXPECTED_DOMAIN_STR",
                            "parse",
                            format!("Expected destination domain string literal, found {other:?}"),
                            self.peek().span,
                            "Pass explicit destination domain literal: reveal(\"mybank.com\", secret_val)",
                        ));
                    }
                };
                self.expect(&TokenKind::Comma, "Expected ',' after reveal domain")?;
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after reveal arguments")?;
                Ok(RawExpr::Reveal {
                    domain,
                    expr: Box::new(inner),
                })
            }
            TokenKind::HashRef(hash) => {
                self.advance();
                let args = self.parse_arg_list()?;
                Ok(RawExpr::Call {
                    target: TargetRef::Hash(hash),
                    args,
                })
            }
            TokenKind::Ident(name) => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::ColonColon) {
                    self.advance();
                    let (member, _) = self.expect_ident("Expected member name after '::'")?;
                    if matches!(self.peek().kind, TokenKind::ColonColon) {
                        self.advance();
                        let (prop, _) = self.expect_ident(
                            "Expected layout field property ('len' or 'end') after '::'",
                        )?;
                        Ok(RawExpr::Path3 {
                            head: name,
                            member,
                            prop,
                            span: tok.span,
                        })
                    } else {
                        Ok(RawExpr::Path2 {
                            head: name,
                            member,
                            span: tok.span,
                        })
                    }
                } else if matches!(self.peek().kind, TokenKind::Dot) {
                    self.advance();
                    let (field, _) = self.expect_ident("Expected layout field name after '.'")?;
                    Ok(RawExpr::LayoutLoad {
                        layout: name,
                        field,
                        span: tok.span,
                    })
                } else if matches!(self.peek().kind, TokenKind::LParen) {
                    let args = self.parse_arg_list()?;
                    let target = if name == "$self" {
                        TargetRef::SelfRec
                    } else {
                        TargetRef::Named(name)
                    };
                    Ok(RawExpr::Call { target, args })
                } else {
                    Ok(RawExpr::Var(name))
                }
            }
            other => Err(AxiomDiagnostic::new(
                "E016_EXPECTED_EXPR",
                "parse",
                format!("Expected expression, found {other:?}"),
                tok.span,
                "Provide a valid expression (literal, variable, call, block, if/else, match, mem_load/mem_store, or 'do Effect.op(...)')",
            )),
        }
    }

    fn parse_arg_list(&mut self) -> Result<Vec<RawExpr>, AxiomDiagnostic> {
        self.expect(&TokenKind::LParen, "Expected '(' to start argument list")?;
        let mut args = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "Expected ')' to close argument list")?;
        Ok(args)
    }
}

// ---------------------------------------------------------------------------
// Stage B: Compile-Time Evaluation, Static Layout Overlap Proof, & Desugaring
// ---------------------------------------------------------------------------

struct ModuleResolver {
    consts: BTreeMap<String, ConstDef>,
    enums: BTreeMap<String, EnumDef>,
    layouts: BTreeMap<String, LayoutDef>,
    consts_order: Vec<ConstDef>,
    enums_order: Vec<EnumDef>,
    layouts_order: Vec<LayoutDef>,
    layout_spans: Vec<(i64, i64, String, Span)>,
    match_counter: usize,
}

impl ModuleResolver {
    fn new() -> Self {
        Self {
            consts: BTreeMap::new(),
            enums: BTreeMap::new(),
            layouts: BTreeMap::new(),
            consts_order: Vec::new(),
            enums_order: Vec::new(),
            layouts_order: Vec::new(),
            layout_spans: Vec::new(),
            match_counter: 0,
        }
    }

    fn eval_const(&self, expr: &RawExpr, span: Span) -> Result<ConstValue, AxiomDiagnostic> {
        match expr {
            RawExpr::IntLit(v) => Ok(ConstValue::Int(*v)),
            RawExpr::BoolLit(b) => Ok(ConstValue::Bool(*b)),
            RawExpr::Var(name) => self.consts.get(name).map(|c| c.value).ok_or_else(|| {
                AxiomDiagnostic::new(
                    "E064_CONST_EVAL",
                    "const-eval",
                    format!("Unknown constant '{name}' in constant expression"),
                    span,
                    "Declare the constant with 'const NAME: i64 = ...;' before referencing it",
                )
            }),
            RawExpr::Path2 {
                head,
                member,
                span: pspan,
            } => Ok(ConstValue::Int(self.resolve_path2(head, member, *pspan)?)),
            RawExpr::Path3 {
                head,
                member,
                prop,
                span: pspan,
            } => Ok(ConstValue::Int(
                self.resolve_path3(head, member, prop, *pspan)?,
            )),
            RawExpr::Unary { op, expr: inner } => {
                let v = self.eval_const(inner, span)?;
                match (op, v) {
                    (UnaryOp::Neg, ConstValue::Int(n)) => n
                        .checked_neg()
                        .map(ConstValue::Int)
                        .ok_or_else(|| {
                            AxiomDiagnostic::new(
                                "E064_CONST_EVAL",
                                "const-eval",
                                format!("Integer overflow negating {n} in constant expression"),
                                span,
                                "Keep constant expressions within signed 64-bit integer bounds",
                            )
                        }),
                    (UnaryOp::Not, ConstValue::Bool(b)) => Ok(ConstValue::Bool(!b)),
                    _ => Err(AxiomDiagnostic::new(
                        "E064_CONST_EVAL",
                        "const-eval",
                        format!("Invalid unary operator '{}' in constant expression", op.as_str()),
                        span,
                        "Use '-' on i64 constants and '!' on bool constants",
                    )),
                }
            }
            RawExpr::Binary { op, lhs, rhs } => {
                let lv = self.eval_const(lhs, span)?;
                let rv = self.eval_const(rhs, span)?;
                match (op, lv, rv) {
                    (BinOp::Add, ConstValue::Int(a), ConstValue::Int(b)) => a
                        .checked_add(b)
                        .map(ConstValue::Int)
                        .ok_or_else(|| const_overflow("+", span)),
                    (BinOp::Sub, ConstValue::Int(a), ConstValue::Int(b)) => a
                        .checked_sub(b)
                        .map(ConstValue::Int)
                        .ok_or_else(|| const_overflow("-", span)),
                    (BinOp::Mul, ConstValue::Int(a), ConstValue::Int(b)) => a
                        .checked_mul(b)
                        .map(ConstValue::Int)
                        .ok_or_else(|| const_overflow("*", span)),
                    (BinOp::Div, ConstValue::Int(a), ConstValue::Int(b)) => {
                        if b == 0 {
                            return Err(AxiomDiagnostic::new(
                                "E064_CONST_EVAL",
                                "const-eval",
                                "Division by zero in constant expression",
                                span,
                                "Ensure the divisor in a constant expression is non-zero",
                            ));
                        }
                        a.checked_div(b)
                            .map(ConstValue::Int)
                            .ok_or_else(|| const_overflow("/", span))
                    }
                    (BinOp::Mod, ConstValue::Int(a), ConstValue::Int(b)) => {
                        if b == 0 {
                            return Err(AxiomDiagnostic::new(
                                "E064_CONST_EVAL",
                                "const-eval",
                                "Modulo by zero in constant expression",
                                span,
                                "Ensure the divisor in a constant expression is non-zero",
                            ));
                        }
                        a.checked_rem(b)
                            .map(ConstValue::Int)
                            .ok_or_else(|| const_overflow("%", span))
                    }
                    (BinOp::BitAnd, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Int(a & b))
                    }
                    (BinOp::BitOr, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Int(a | b))
                    }
                    (BinOp::BitXor, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Int(a ^ b))
                    }
                    (BinOp::Shl, ConstValue::Int(a), ConstValue::Int(b)) => {
                        if !(0..64).contains(&b) {
                            return Err(AxiomDiagnostic::new(
                                "E064_CONST_EVAL",
                                "const-eval",
                                format!("Invalid shift amount {b} in constant expression"),
                                span,
                                "Use a shift count in the range 0..63",
                            ));
                        }
                        Ok(ConstValue::Int(a.wrapping_shl(b as u32)))
                    }
                    (BinOp::Shr, ConstValue::Int(a), ConstValue::Int(b)) => {
                        if !(0..64).contains(&b) {
                            return Err(AxiomDiagnostic::new(
                                "E064_CONST_EVAL",
                                "const-eval",
                                format!("Invalid shift amount {b} in constant expression"),
                                span,
                                "Use a shift count in the range 0..63",
                            ));
                        }
                        Ok(ConstValue::Int(a.wrapping_shr(b as u32)))
                    }
                    (BinOp::Eq, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Bool(a == b))
                    }
                    (BinOp::Ne, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Bool(a != b))
                    }
                    (BinOp::Lt, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Bool(a < b))
                    }
                    (BinOp::Le, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Bool(a <= b))
                    }
                    (BinOp::Gt, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Bool(a > b))
                    }
                    (BinOp::Ge, ConstValue::Int(a), ConstValue::Int(b)) => {
                        Ok(ConstValue::Bool(a >= b))
                    }
                    (BinOp::And, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                        Ok(ConstValue::Bool(a && b))
                    }
                    (BinOp::Or, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                        Ok(ConstValue::Bool(a || b))
                    }
                    (BinOp::Eq, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                        Ok(ConstValue::Bool(a == b))
                    }
                    (BinOp::Ne, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                        Ok(ConstValue::Bool(a != b))
                    }
                    _ => Err(AxiomDiagnostic::new(
                        "E064_CONST_EVAL",
                        "const-eval",
                        format!(
                            "Type mismatch for operator '{}' in constant expression",
                            op.as_str()
                        ),
                        span,
                        "Ensure operand types match the operator in constant expressions",
                    )),
                }
            }
            _ => Err(AxiomDiagnostic::new(
                "E064_CONST_EVAL",
                "const-eval",
                "Non-constant expression used where a compile-time constant is required",
                span,
                "Use integer/bool literals, arithmetic/bitwise operators, const names, Enum::Variant, or Layout::field",
            )),
        }
    }

    fn eval_const_i64(&self, expr: &RawExpr, span: Span, context: &str) -> Result<i64, AxiomDiagnostic> {
        match self.eval_const(expr, span)? {
            ConstValue::Int(v) => Ok(v),
            ConstValue::Bool(_) => Err(AxiomDiagnostic::new(
                "E064_CONST_EVAL",
                "const-eval",
                format!("Expected i64 constant for {context}, found bool"),
                span,
                "Provide an integer constant expression",
            )),
        }
    }

    fn resolve_path2(&self, head: &str, member: &str, span: Span) -> Result<i64, AxiomDiagnostic> {
        if let Some(edef) = self.enums.get(head) {
            if let Some(v) = edef.variants.iter().find(|v| v.name == member) {
                return Ok(v.value);
            }
            return Err(AxiomDiagnostic::new(
                "E061_UNKNOWN_ENUM_VARIANT",
                "resolve",
                format!("Enum '{head}' has no variant '{member}'"),
                span,
                format!("Check the variants declared in 'enum {head}'"),
            ));
        }
        if let Some(ldef) = self.layouts.get(head) {
            if let Some(f) = ldef.fields.iter().find(|f| f.name == member) {
                return Ok(f.offset);
            }
            return Err(AxiomDiagnostic::new(
                "E061_UNKNOWN_LAYOUT_FIELD",
                "resolve",
                format!("Layout '{head}' has no field '{member}'"),
                span,
                format!("Check the fields declared in 'layout {head}'"),
            ));
        }
        Err(AxiomDiagnostic::new(
            "E061_UNKNOWN_ENUM_VARIANT",
            "resolve",
            format!("Unknown enum or layout '{head}' in '{head}::{member}'"),
            span,
            format!("Declare 'enum {head}' or 'layout {head}' (or import it with 'use')"),
        ))
    }

    fn resolve_path3(
        &self,
        head: &str,
        member: &str,
        prop: &str,
        span: Span,
    ) -> Result<i64, AxiomDiagnostic> {
        let ldef = self.layouts.get(head).ok_or_else(|| {
            AxiomDiagnostic::new(
                "E061_UNKNOWN_LAYOUT_FIELD",
                "resolve",
                format!("Unknown layout '{head}' in '{head}::{member}::{prop}'"),
                span,
                format!("Declare 'layout {head}' before referencing its field properties"),
            )
        })?;
        let field = ldef
            .fields
            .iter()
            .find(|f| f.name == member)
            .ok_or_else(|| {
                AxiomDiagnostic::new(
                    "E061_UNKNOWN_LAYOUT_FIELD",
                    "resolve",
                    format!("Layout '{head}' has no field '{member}'"),
                    span,
                    format!("Check the fields declared in 'layout {head}'"),
                )
            })?;
        match prop {
            "len" => Ok(field.len),
            "end" => Ok(field.offset + field.len),
            other => Err(AxiomDiagnostic::new(
                "E061_UNKNOWN_LAYOUT_FIELD",
                "resolve",
                format!("Unknown layout field property '{other}' on '{head}::{member}'"),
                span,
                "Supported layout field properties are '::len' and '::end'",
            )),
        }
    }

    fn resolve_layout_word_offset(
        &self,
        layout: &str,
        field: &str,
        span: Span,
    ) -> Result<i64, AxiomDiagnostic> {
        let ldef = self.layouts.get(layout).ok_or_else(|| {
            AxiomDiagnostic::new(
                "E061_UNKNOWN_LAYOUT_FIELD",
                "resolve",
                format!("Unknown layout '{layout}' in '{layout}.{field}'"),
                span,
                format!("Declare 'layout {layout}' before accessing '{layout}.{field}'"),
            )
        })?;
        let fdef = ldef
            .fields
            .iter()
            .find(|f| f.name == field)
            .ok_or_else(|| {
                AxiomDiagnostic::new(
                    "E061_UNKNOWN_LAYOUT_FIELD",
                    "resolve",
                    format!("Layout '{layout}' has no field '{field}'"),
                    span,
                    format!("Check the fields declared in 'layout {layout}'"),
                )
            })?;
        if fdef.is_array {
            return Err(AxiomDiagnostic::new(
                "E061_LAYOUT_ARRAY_ACCESS",
                "resolve",
                format!(
                    "Cannot use direct scalar access '{layout}.{field}' on array buffer 'words[{}]'",
                    fdef.len
                ),
                span,
                format!(
                    "Use 'mem_load({layout}::{field} + idx)' / 'mem_store({layout}::{field} + idx, val)' for buffer fields"
                ),
            ));
        }
        Ok(fdef.offset)
    }

    fn register_item(&mut self, item: RawItem) -> Result<(), AxiomDiagnostic> {
        match item {
            RawItem::Const(rc) => {
                if self.consts.contains_key(&rc.name)
                    || self.enums.contains_key(&rc.name)
                    || self.layouts.contains_key(&rc.name)
                {
                    return Err(AxiomDiagnostic::new(
                        "E065_DUPLICATE_DECL",
                        "resolve",
                        format!("Duplicate top-level declaration '{}'", rc.name),
                        rc.span,
                        "Rename the constant so every top-level const/enum/layout name is unique",
                    ));
                }
                let val = self.eval_const(&rc.expr, rc.span)?;
                let matches_ty = matches!(
                    (&rc.ty, val),
                    (Type::I64, ConstValue::Int(_)) | (Type::Bool, ConstValue::Bool(_))
                );
                if !matches_ty {
                    return Err(AxiomDiagnostic::new(
                        "E064_CONST_EVAL",
                        "const-eval",
                        format!(
                            "Constant '{}' declared as '{}' does not match initializer value",
                            rc.name, rc.ty
                        ),
                        rc.span,
                        "Ensure the constant's declared type matches its initializer expression",
                    ));
                }
                let def = ConstDef {
                    name: rc.name.clone(),
                    ty: rc.ty,
                    value: val,
                };
                self.consts.insert(rc.name, def.clone());
                self.consts_order.push(def);
            }
            RawItem::Enum(re) => {
                if self.consts.contains_key(&re.name)
                    || self.enums.contains_key(&re.name)
                    || self.layouts.contains_key(&re.name)
                {
                    return Err(AxiomDiagnostic::new(
                        "E065_DUPLICATE_DECL",
                        "resolve",
                        format!("Duplicate top-level declaration '{}'", re.name),
                        re.span,
                        "Rename the enum so every top-level const/enum/layout name is unique",
                    ));
                }
                let mut next_val: i64 = 0;
                let mut seen_variants = BTreeSet::new();
                let mut variants = Vec::with_capacity(re.variants.len());
                for rv in re.variants {
                    if !seen_variants.insert(rv.name.clone()) {
                        return Err(AxiomDiagnostic::new(
                            "E065_DUPLICATE_DECL",
                            "resolve",
                            format!("Duplicate variant '{}' in enum '{}'", rv.name, re.name),
                            rv.span,
                            "Ensure all enum variant names are unique",
                        ));
                    }
                    let val = match rv.value {
                        Some(expr) => self.eval_const_i64(
                            &expr,
                            rv.span,
                            &format!("enum variant {}::{}", re.name, rv.name),
                        )?,
                        None => next_val,
                    };
                    next_val = val.checked_add(1).ok_or_else(|| const_overflow("+", rv.span))?;
                    variants.push(EnumVariant {
                        name: rv.name,
                        value: val,
                    });
                }
                let def = EnumDef {
                    name: re.name.clone(),
                    variants,
                };
                self.enums.insert(re.name, def.clone());
                self.enums_order.push(def);
            }
            RawItem::Layout(rl) => {
                if self.consts.contains_key(&rl.name)
                    || self.enums.contains_key(&rl.name)
                    || self.layouts.contains_key(&rl.name)
                {
                    return Err(AxiomDiagnostic::new(
                        "E065_DUPLICATE_DECL",
                        "resolve",
                        format!("Duplicate top-level declaration '{}'", rl.name),
                        rl.span,
                        "Rename the layout so every top-level const/enum/layout name is unique",
                    ));
                }
                let base = match rl.base_expr {
                    Some(ref expr) => {
                        self.eval_const_i64(expr, rl.span, &format!("layout '{}' base", rl.name))?
                    }
                    None => 0,
                };
                if base < 0 {
                    return Err(AxiomDiagnostic::new(
                        "E064_CONST_EVAL",
                        "layout",
                        format!("Layout '{}' base offset ({base}) cannot be negative", rl.name),
                        rl.span,
                        "Specify a non-negative base word offset (e.g. 'at 0')",
                    ));
                }
                let mut cursor = base;
                let mut seen_fields = BTreeSet::new();
                let mut fields = Vec::with_capacity(rl.fields.len());
                for rf in rl.fields {
                    if !seen_fields.insert(rf.name.clone()) {
                        return Err(AxiomDiagnostic::new(
                            "E065_DUPLICATE_DECL",
                            "layout",
                            format!("Duplicate field '{}' in layout '{}'", rf.name, rl.name),
                            rf.span,
                            "Ensure all field names within a layout are unique",
                        ));
                    }
                    let len = match rf.len_expr {
                        Some(ref expr) => {
                            let l = self.eval_const_i64(
                                expr,
                                rf.span,
                                &format!("layout field '{}::{}' length", rl.name, rf.name),
                            )?;
                            if l <= 0 {
                                return Err(AxiomDiagnostic::new(
                                    "E064_CONST_EVAL",
                                    "layout",
                                    format!(
                                        "Layout buffer '{}::{}' length ({l}) must be positive",
                                        rl.name, rf.name
                                    ),
                                    rf.span,
                                    "Use a positive integer length for 'words[N]'",
                                ));
                            }
                            l
                        }
                        None => 1,
                    };
                    let offset = match rf.at_expr {
                        Some(ref expr) => {
                            let o = self.eval_const_i64(
                                expr,
                                rf.span,
                                &format!("layout field '{}::{}' offset", rl.name, rf.name),
                            )?;
                            if o < 0 {
                                return Err(AxiomDiagnostic::new(
                                    "E064_CONST_EVAL",
                                    "layout",
                                    format!(
                                        "Layout field '{}::{}' offset ({o}) cannot be negative",
                                        rl.name, rf.name
                                    ),
                                    rf.span,
                                    "Specify a non-negative word offset after 'at'",
                                ));
                            }
                            o
                        }
                        None => cursor,
                    };
                    let end = offset
                        .checked_add(len)
                        .ok_or_else(|| const_overflow("+", rf.span))?;
                    cursor = end;
                    self.layout_spans.push((
                        offset,
                        end,
                        format!("{}::{}", rl.name, rf.name),
                        rf.span,
                    ));
                    fields.push(LayoutField {
                        name: rf.name,
                        offset,
                        len,
                        is_array: rf.is_array,
                    });
                }
                let def = LayoutDef {
                    name: rl.name.clone(),
                    base,
                    fields,
                };
                self.layouts.insert(rl.name, def.clone());
                self.layouts_order.push(def);
            }
        }
        Ok(())
    }

    fn verify_no_layout_overlaps(&self) -> Result<(), AxiomDiagnostic> {
        let mut intervals = self.layout_spans.clone();
        intervals.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        let mut max_end: Option<(i64, i64, String)> = None;
        for (start, end, label, span) in intervals {
            if let Some((prev_start, prev_end, ref prev_label)) = max_end {
                if prev_end > start {
                    return Err(AxiomDiagnostic::new(
                        "E062_LAYOUT_OVERLAP",
                        "layout",
                        format!(
                            "Linear memory layout overlap: '{prev_label}' ([{prev_start}..{prev_end})) overlaps '{label}' ([{start}..{end}))"
                        ),
                        span,
                        format!(
                            "Move '{label}' to at least word offset {prev_end} (or shrink '{prev_label}') so all layout regions are strictly disjoint"
                        ),
                    ));
                }
            }
            let replace = match &max_end {
                Some((_, prev_end, _)) => end > *prev_end,
                None => true,
            };
            if replace {
                max_end = Some((start, end, label));
            }
        }
        Ok(())
    }

    fn lower_fn(&mut self, raw: RawFnDef) -> Result<FnDef, AxiomDiagnostic> {
        let mut locals: Vec<BTreeSet<String>> = vec![raw.params.iter().map(|p| p.name.clone()).collect()];

        let mut requires = Vec::with_capacity(raw.requires.len());
        for r in &raw.requires {
            requires.push(self.lower_expr(r, &mut locals)?);
        }
        let mut ensures = Vec::with_capacity(raw.ensures.len());
        for e in &raw.ensures {
            ensures.push(self.lower_expr(e, &mut locals)?);
        }
        let body = self.lower_expr(&raw.body, &mut locals)?;

        Ok(FnDef {
            name: raw.name,
            exported: raw.exported,
            params: raw.params,
            effects: raw.effects,
            ret_type: raw.ret_type,
            requires,
            ensures,
            body,
        })
    }

    fn is_shadowed(locals: &[BTreeSet<String>], name: &str) -> bool {
        locals.iter().any(|frame| frame.contains(name))
    }

    fn lower_stmts(
        &mut self,
        stmts: &[RawStmt],
        locals: &mut Vec<BTreeSet<String>>,
    ) -> Result<Vec<Stmt>, AxiomDiagnostic> {
        let mut out = Vec::with_capacity(stmts.len());
        for s in stmts {
            match s {
                RawStmt::Let {
                    name,
                    mutable,
                    ty,
                    value,
                } => {
                    let v = self.lower_expr(value, locals)?;
                    if let Some(top) = locals.last_mut() {
                        top.insert(name.clone());
                    }
                    out.push(Stmt::Let {
                        name: name.clone(),
                        mutable: *mutable,
                        ty: ty.clone(),
                        value: v,
                    });
                }
                RawStmt::Assign { name, op, value } => {
                    let rhs = self.lower_expr(value, locals)?;
                    let final_val = match op {
                        None => rhs,
                        Some(binop) => Expr::Binary {
                            op: *binop,
                            lhs: Box::new(Expr::Var(name.clone())),
                            rhs: Box::new(rhs),
                        },
                    };
                    out.push(Stmt::Assign {
                        name: name.clone(),
                        value: final_val,
                    });
                }
                RawStmt::LayoutStore {
                    layout,
                    field,
                    op,
                    value,
                    span,
                } => {
                    let offset = self.resolve_layout_word_offset(layout, field, *span)?;
                    let rhs = self.lower_expr(value, locals)?;
                    let final_val = match op {
                        None => rhs,
                        Some(binop) => Expr::Binary {
                            op: *binop,
                            lhs: Box::new(Expr::MemLoad {
                                index: Box::new(Expr::IntLit(offset)),
                            }),
                            rhs: Box::new(rhs),
                        },
                    };
                    out.push(Stmt::Expr(Expr::MemStore {
                        index: Box::new(Expr::IntLit(offset)),
                        value: Box::new(final_val),
                    }));
                }
                RawStmt::While {
                    cond,
                    max_iters,
                    body,
                    span,
                } => {
                    let max_val = self.eval_const_i64(max_iters, *span, "'while ... max' bound")?;
                    if max_val <= 0 || max_val > u32::MAX as i64 {
                        return Err(AxiomDiagnostic::new(
                            "E017_INVALID_LOOP_BOUND",
                            "parse",
                            format!("Loop bound 'max {max_val}' must be in range 1..={}", u32::MAX),
                            *span,
                            "Provide a positive 32-bit compile-time iteration bound after 'max'",
                        ));
                    }
                    let c = self.lower_expr(cond, locals)?;
                    locals.push(BTreeSet::new());
                    let b = self.lower_stmts(body, locals)?;
                    locals.pop();
                    out.push(Stmt::While {
                        cond: c,
                        max_iters: max_val as u32,
                        body: b,
                    });
                }
                RawStmt::Expr(e) => {
                    out.push(Stmt::Expr(self.lower_expr(e, locals)?));
                }
            }
        }
        Ok(out)
    }

    fn lower_expr(
        &mut self,
        expr: &RawExpr,
        locals: &mut Vec<BTreeSet<String>>,
    ) -> Result<Expr, AxiomDiagnostic> {
        match expr {
            RawExpr::IntLit(v) => Ok(Expr::IntLit(*v)),
            RawExpr::BoolLit(b) => Ok(Expr::BoolLit(*b)),
            RawExpr::StrLit(s) => Ok(Expr::StrLit(s.clone())),
            RawExpr::UnitLit => Ok(Expr::UnitLit),
            RawExpr::Var(name) => {
                if !Self::is_shadowed(locals, name) {
                    if let Some(cdef) = self.consts.get(name) {
                        return Ok(match cdef.value {
                            ConstValue::Int(v) => Expr::IntLit(v),
                            ConstValue::Bool(b) => Expr::BoolLit(b),
                        });
                    }
                }
                Ok(Expr::Var(name.clone()))
            }
            RawExpr::Path2 { head, member, span } => {
                let val = self.resolve_path2(head, member, *span)?;
                Ok(Expr::IntLit(val))
            }
            RawExpr::Path3 {
                head,
                member,
                prop,
                span,
            } => {
                let val = self.resolve_path3(head, member, prop, *span)?;
                Ok(Expr::IntLit(val))
            }
            RawExpr::LayoutLoad {
                layout,
                field,
                span,
            } => {
                let offset = self.resolve_layout_word_offset(layout, field, *span)?;
                Ok(Expr::MemLoad {
                    index: Box::new(Expr::IntLit(offset)),
                })
            }
            RawExpr::Unary { op, expr: inner } => Ok(Expr::Unary {
                op: *op,
                expr: Box::new(self.lower_expr(inner, locals)?),
            }),
            RawExpr::Binary { op, lhs, rhs } => Ok(Expr::Binary {
                op: *op,
                lhs: Box::new(self.lower_expr(lhs, locals)?),
                rhs: Box::new(self.lower_expr(rhs, locals)?),
            }),
            RawExpr::If {
                cond,
                then_branch,
                else_branch,
            } => Ok(Expr::If {
                cond: Box::new(self.lower_expr(cond, locals)?),
                then_branch: Box::new(self.lower_expr(then_branch, locals)?),
                else_branch: Box::new(self.lower_expr(else_branch, locals)?),
            }),
            RawExpr::Match {
                scrutinee,
                arms,
                span,
            } => self.desugar_match(scrutinee, arms, *span, locals),
            RawExpr::Call { target, args } => {
                let mut largs = Vec::with_capacity(args.len());
                for a in args {
                    largs.push(self.lower_expr(a, locals)?);
                }
                Ok(Expr::Call {
                    target: target.clone(),
                    args: largs,
                })
            }
            RawExpr::Perform { effect, op, args } => {
                let mut largs = Vec::with_capacity(args.len());
                for a in args {
                    largs.push(self.lower_expr(a, locals)?);
                }
                Ok(Expr::Perform {
                    effect: effect.clone(),
                    op: op.clone(),
                    args: largs,
                })
            }
            RawExpr::MemLoad { index } => Ok(Expr::MemLoad {
                index: Box::new(self.lower_expr(index, locals)?),
            }),
            RawExpr::MemStore { index, value } => Ok(Expr::MemStore {
                index: Box::new(self.lower_expr(index, locals)?),
                value: Box::new(self.lower_expr(value, locals)?),
            }),
            RawExpr::SecretWrap(inner) => {
                Ok(Expr::SecretWrap(Box::new(self.lower_expr(inner, locals)?)))
            }
            RawExpr::Reveal { domain, expr: inner } => Ok(Expr::Reveal {
                domain: domain.clone(),
                expr: Box::new(self.lower_expr(inner, locals)?),
            }),
            RawExpr::Block { stmts, tail } => {
                locals.push(BTreeSet::new());
                let lstmts = self.lower_stmts(stmts, locals)?;
                let ltail = self.lower_expr(tail, locals)?;
                locals.pop();
                if lstmts.is_empty() {
                    Ok(ltail)
                } else {
                    Ok(Expr::Block {
                        stmts: lstmts,
                        tail: Box::new(ltail),
                    })
                }
            }
        }
    }

    fn desugar_match(
        &mut self,
        scrutinee: &RawExpr,
        arms: &[RawMatchArm],
        span: Span,
        locals: &mut Vec<BTreeSet<String>>,
    ) -> Result<Expr, AxiomDiagnostic> {
        if arms.is_empty() {
            return Err(AxiomDiagnostic::new(
                "E063_NON_EXHAUSTIVE_MATCH",
                "resolve",
                "Empty 'match' expression is not exhaustive",
                span,
                "Add at least one arm ending with a wildcard '_ => ...' or covering all enum variants",
            ));
        }

        enum ResolvedPat {
            Wildcard,
            Int(i64),
            Bool(bool),
            Enum {
                enum_name: String,
                variant: String,
                value: i64,
            },
        }

        let mut resolved_arms: Vec<(Vec<ResolvedPat>, Expr)> = Vec::with_capacity(arms.len());
        let mut wildcard_arm_idx: Option<usize> = None;

        for (arm_idx, arm) in arms.iter().enumerate() {
            let mut rpats = Vec::with_capacity(arm.patterns.len());
            for pat in &arm.patterns {
                let rp = match pat {
                    RawMatchPattern::Wildcard(pspan) => {
                        if arm_idx + 1 != arms.len() {
                            return Err(AxiomDiagnostic::new(
                                "E063_UNREACHABLE_MATCH_ARM",
                                "resolve",
                                "Wildcard '_' arm must be the final arm of a 'match' expression",
                                *pspan,
                                "Move the '_ => ...' fallback arm to the end of the match block",
                            ));
                        }
                        wildcard_arm_idx = Some(arm_idx);
                        ResolvedPat::Wildcard
                    }
                    RawMatchPattern::Int(v) => ResolvedPat::Int(*v),
                    RawMatchPattern::Bool(b) => ResolvedPat::Bool(*b),
                    RawMatchPattern::ConstRef(name, pspan) => {
                        let cdef = self.consts.get(name).ok_or_else(|| {
                            AxiomDiagnostic::new(
                                "E064_CONST_EVAL",
                                "resolve",
                                format!("Unknown constant '{name}' in match pattern"),
                                *pspan,
                                "Use a declared 'const', literal, Enum::Variant, or '_' wildcard in match patterns",
                            )
                        })?;
                        match cdef.value {
                            ConstValue::Int(v) => ResolvedPat::Int(v),
                            ConstValue::Bool(b) => ResolvedPat::Bool(b),
                        }
                    }
                    RawMatchPattern::Path2(head, member, pspan) => {
                        if let Some(edef) = self.enums.get(head) {
                            let v = edef
                                .variants
                                .iter()
                                .find(|v| v.name == *member)
                                .ok_or_else(|| {
                                    AxiomDiagnostic::new(
                                        "E061_UNKNOWN_ENUM_VARIANT",
                                        "resolve",
                                        format!("Enum '{head}' has no variant '{member}'"),
                                        *pspan,
                                        format!("Check the variants declared in 'enum {head}'"),
                                    )
                                })?;
                            ResolvedPat::Enum {
                                enum_name: head.clone(),
                                variant: member.clone(),
                                value: v.value,
                            }
                        } else {
                            let v = self.resolve_path2(head, member, *pspan)?;
                            ResolvedPat::Int(v)
                        }
                    }
                    RawMatchPattern::Path3(head, member, prop, pspan) => {
                        let v = self.resolve_path3(head, member, prop, *pspan)?;
                        ResolvedPat::Int(v)
                    }
                };
                rpats.push(rp);
            }
            let body = self.lower_expr(&arm.body, locals)?;
            resolved_arms.push((rpats, body));
        }

        // Verify exhaustiveness when there is no wildcard arm
        if wildcard_arm_idx.is_none() {
            let mut all_bool = true;
            let mut seen_true = false;
            let mut seen_false = false;
            let mut single_enum: Option<String> = None;
            let mut all_same_enum = true;
            let mut covered_variants: BTreeSet<String> = BTreeSet::new();

            for (pats, _) in &resolved_arms {
                for p in pats {
                    match p {
                        ResolvedPat::Bool(true) => {
                            seen_true = true;
                            all_same_enum = false;
                        }
                        ResolvedPat::Bool(false) => {
                            seen_false = true;
                            all_same_enum = false;
                        }
                        ResolvedPat::Enum {
                            enum_name, variant, ..
                        } => {
                            all_bool = false;
                            match &single_enum {
                                None => single_enum = Some(enum_name.clone()),
                                Some(prev) if prev != enum_name => all_same_enum = false,
                                Some(_) => {}
                            }
                            covered_variants.insert(variant.clone());
                        }
                        ResolvedPat::Int(_) | ResolvedPat::Wildcard => {
                            all_bool = false;
                            all_same_enum = false;
                        }
                    }
                }
            }

            let exhaustive = if all_bool && seen_true && seen_false {
                true
            } else if all_same_enum {
                if let Some(ref ename) = single_enum {
                    if let Some(edef) = self.enums.get(ename) {
                        let missing: Vec<String> = edef
                            .variants
                            .iter()
                            .filter(|v| !covered_variants.contains(&v.name))
                            .map(|v| format!("{ename}::{}", v.name))
                            .collect();
                        if !missing.is_empty() {
                            return Err(AxiomDiagnostic::new(
                                "E063_NON_EXHAUSTIVE_MATCH",
                                "resolve",
                                format!(
                                    "Non-exhaustive 'match' on enum '{ename}': missing variant(s) {}",
                                    missing.join(", ")
                                ),
                                span,
                                "Cover all enum variants or add a wildcard fallback arm '_ => ...'",
                            ));
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if !exhaustive {
                return Err(AxiomDiagnostic::new(
                    "E063_NON_EXHAUSTIVE_MATCH",
                    "resolve",
                    "Non-exhaustive 'match' expression: missing wildcard fallback '_ => ...'",
                    span,
                    "Add a wildcard fallback arm '_ => ...' (or cover all variants of a single enum / both bool values)",
                ));
            }
        }

        let lowered_scrut = self.lower_expr(scrutinee, locals)?;
        let scrut_var = format!("$match_{}", self.match_counter);
        self.match_counter += 1;

        // Build nested if/else chain from the last arm backwards
        let (_, last_body) = resolved_arms.pop().unwrap();
        let mut acc = last_body;

        for (pats, arm_body) in resolved_arms.into_iter().rev() {
            let mut cond_opt: Option<Expr> = None;
            for p in pats {
                let lit = match p {
                    ResolvedPat::Int(v) | ResolvedPat::Enum { value: v, .. } => Expr::IntLit(v),
                    ResolvedPat::Bool(b) => Expr::BoolLit(b),
                    ResolvedPat::Wildcard => continue,
                };
                let eq = Expr::Binary {
                    op: BinOp::Eq,
                    lhs: Box::new(Expr::Var(scrut_var.clone())),
                    rhs: Box::new(lit),
                };
                cond_opt = Some(match cond_opt {
                    None => eq,
                    Some(prev) => Expr::Binary {
                        op: BinOp::Or,
                        lhs: Box::new(prev),
                        rhs: Box::new(eq),
                    },
                });
            }
            if let Some(cond) = cond_opt {
                acc = Expr::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(arm_body),
                    else_branch: Box::new(acc),
                };
            }
        }

        Ok(Expr::Block {
            stmts: vec![Stmt::Let {
                name: scrut_var,
                mutable: false,
                ty: None,
                value: lowered_scrut,
            }],
            tail: Box::new(acc),
        })
    }
}

fn const_overflow(op: &str, span: Span) -> AxiomDiagnostic {
    AxiomDiagnostic::new(
        "E064_CONST_EVAL",
        "const-eval",
        format!("Signed 64-bit overflow evaluating '{op}' in constant expression"),
        span,
        "Keep constant arithmetic within signed 64-bit integer bounds",
    )
}

fn resolve_and_lower(
    items: Vec<RawItem>,
    raw_fns: Vec<RawFnDef>,
) -> Result<Module, AxiomDiagnostic> {
    let mut resolver = ModuleResolver::new();
    for item in items {
        resolver.register_item(item)?;
    }
    resolver.verify_no_layout_overlaps()?;

    let mut functions = Vec::with_capacity(raw_fns.len());
    for rf in raw_fns {
        functions.push(resolver.lower_fn(rf)?);
    }

    Ok(Module {
        consts: resolver.consts_order,
        enums: resolver.enums_order,
        layouts: resolver.layouts_order,
        functions,
    })
}
