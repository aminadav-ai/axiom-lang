use crate::ast::{
    BinOp, EffectCap, Expr, FnDef, Module, Param, Stmt, TargetRef, Type, UnaryOp,
};
use crate::diag::{AxiomDiagnostic, Span};
use crate::lexer::{tokenize, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    idx: usize,
}

pub fn parse_module(src: &str) -> Result<Module, AxiomDiagnostic> {
    let tokens = tokenize(src)?;
    let mut parser = Parser { tokens, idx: 0 };
    let mut functions = Vec::new();
    while !parser.at_eof() {
        functions.push(parser.parse_fn()?);
    }
    Ok(Module { functions })
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.idx]
    }

    fn peek_next(&self) -> &TokenKind {
        if self.idx + 1 < self.tokens.len() {
            &self.tokens[self.idx + 1].kind
        } else {
            &TokenKind::Eof
        }
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

    fn parse_fn(&mut self) -> Result<FnDef, AxiomDiagnostic> {
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

        Ok(FnDef {
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
        let (tname, span) = self.expect_ident("Expected type name (i64, bool, str, unit, Secret<T>)")?;
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

    fn parse_stmt_list_block(&mut self) -> Result<Vec<Stmt>, AxiomDiagnostic> {
        self.expect(&TokenKind::LBrace, "Expected '{' to start loop body")?;
        let mut stmts = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RBrace | TokenKind::Eof) {
            stmts.push(self.parse_single_stmt_in_loop()?);
        }
        self.expect(&TokenKind::RBrace, "Expected '}' to close loop body")?;
        Ok(stmts)
    }

    fn parse_single_stmt_in_loop(&mut self) -> Result<Stmt, AxiomDiagnostic> {
        if matches!(self.peek().kind, TokenKind::Let) {
            return self.parse_let_stmt();
        }
        if matches!(self.peek().kind, TokenKind::While) {
            return self.parse_while_stmt();
        }
        if matches!(&self.peek().kind, TokenKind::Ident(_))
            && matches!(self.peek_next(), TokenKind::Assign)
        {
            let (vname, _) = self.expect_ident("Expected variable name")?;
            self.expect(&TokenKind::Assign, "Expected '='")?;
            let val = self.parse_expr()?;
            self.expect(&TokenKind::Semi, "Terminate assignment statement with ';'")?;
            return Ok(Stmt::Assign {
                name: vname,
                value: val,
            });
        }
        let expr = self.parse_expr()?;
        if matches!(self.peek().kind, TokenKind::Semi) {
            self.advance();
        } else if !matches!(expr, Expr::If { .. } | Expr::Block { .. }) {
            self.expect(&TokenKind::Semi, "Terminate statement inside loop with ';'")?;
        }
        Ok(Stmt::Expr(expr))
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, AxiomDiagnostic> {
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
        Ok(Stmt::Let {
            name: vname,
            mutable,
            ty,
            value: val,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, AxiomDiagnostic> {
        let w_tok = self.expect(&TokenKind::While, "Expected 'while'")?;
        let cond = self.parse_expr()?;
        self.expect(
            &TokenKind::Max,
            "Every 'while' loop in Axiom.Strata must declare an explicit upper bound: 'while <cond> max <N> { ... }'",
        )?;
        let bound_tok = self.peek().clone();
        let max_iters = match bound_tok.kind {
            TokenKind::Int(v) if v > 0 && v <= u32::MAX as i64 => {
                self.advance();
                v as u32
            }
            _ => {
                return Err(AxiomDiagnostic::new(
                    "E017_INVALID_LOOP_BOUND",
                    "parse",
                    format!(
                        "Expected positive integer constant after 'max', found {:?}",
                        bound_tok.kind
                    ),
                    w_tok.span,
                    "Provide a positive compile-time iteration bound (e.g. 'while i < n max 4096 { ... }')",
                ));
            }
        };
        let body = self.parse_stmt_list_block()?;
        if matches!(self.peek().kind, TokenKind::Semi) {
            self.advance();
        }
        Ok(Stmt::While {
            cond,
            max_iters,
            body,
        })
    }

    fn parse_block_expr(&mut self) -> Result<Expr, AxiomDiagnostic> {
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
            if matches!(&self.peek().kind, TokenKind::Ident(_))
                && matches!(self.peek_next(), TokenKind::Assign)
            {
                let (vname, _) = self.expect_ident("Expected variable name")?;
                self.expect(&TokenKind::Assign, "Expected '='")?;
                let val = self.parse_expr()?;
                self.expect(&TokenKind::Semi, "Terminate assignment with ';'")?;
                stmts.push(Stmt::Assign {
                    name: vname,
                    value: val,
                });
                continue;
            }

            let expr = self.parse_expr()?;
            if matches!(self.peek().kind, TokenKind::Semi) {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RBrace) {
                    self.advance();
                    stmts.push(Stmt::Expr(expr));
                    return Ok(Expr::Block {
                        stmts,
                        tail: Box::new(Expr::UnitLit),
                    });
                }
                stmts.push(Stmt::Expr(expr));
            } else if matches!(expr, Expr::If { .. })
                && !matches!(self.peek().kind, TokenKind::RBrace)
            {
                // Allow `if cond { ... } else { ... }` statement without `;` before next statement
                stmts.push(Stmt::Expr(expr));
            } else {
                self.expect(
                    &TokenKind::RBrace,
                    "Expected '}' after block tail expression (or ';' before next statement)",
                )?;
                if stmts.is_empty() {
                    return Ok(expr);
                }
                return Ok(Expr::Block {
                    stmts,
                    tail: Box::new(expr),
                });
            }
        }

        self.expect(&TokenKind::RBrace, "Expected '}' to close block")?;
        Ok(Expr::Block {
            stmts,
            tail: Box::new(Expr::UnitLit),
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, AxiomDiagnostic> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek().kind, TokenKind::OrOr) {
            self.advance();
            let rhs = self.parse_and()?;
            lhs = Expr::Binary {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let mut lhs = self.parse_bitor()?;
        while matches!(self.peek().kind, TokenKind::AndAnd) {
            self.advance();
            let rhs = self.parse_bitor()?;
            lhs = Expr::Binary {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bitor(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let mut lhs = self.parse_bitxor()?;
        while matches!(self.peek().kind, TokenKind::Pipe) {
            self.advance();
            let rhs = self.parse_bitxor()?;
            lhs = Expr::Binary {
                op: BinOp::BitOr,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let mut lhs = self.parse_bitand()?;
        while matches!(self.peek().kind, TokenKind::Caret) {
            self.advance();
            let rhs = self.parse_bitand()?;
            lhs = Expr::Binary {
                op: BinOp::BitXor,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let mut lhs = self.parse_eq()?;
        while matches!(self.peek().kind, TokenKind::Amp) {
            self.advance();
            let rhs = self.parse_eq()?;
            lhs = Expr::Binary {
                op: BinOp::BitAnd,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_eq(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let mut lhs = self.parse_rel()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::NotEq => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_rel()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_rel(&mut self) -> Result<Expr, AxiomDiagnostic> {
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
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_add()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_mul()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, AxiomDiagnostic> {
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
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, AxiomDiagnostic> {
        match self.peek().kind {
            TokenKind::Minus => {
                self.advance();
                let inner = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(inner),
                })
            }
            TokenKind::Bang => {
                self.advance();
                let inner = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(inner),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, AxiomDiagnostic> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Int(v) => {
                self.advance();
                Ok(Expr::IntLit(v))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::BoolLit(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::BoolLit(false))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr::StrLit(s))
            }
            TokenKind::LBrace => self.parse_block_expr(),
            TokenKind::LParen => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RParen) {
                    self.advance();
                    return Ok(Expr::UnitLit);
                }
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after parenthesized expression")?;
                Ok(inner)
            }
            TokenKind::If => {
                self.advance();
                let cond = self.parse_expr()?;
                let then_branch = self.parse_block_expr()?;
                self.expect(
                    &TokenKind::Else,
                    "Every 'if' expression in Axiom must have an explicit 'else { ... }' branch",
                )?;
                let else_branch = if matches!(self.peek().kind, TokenKind::If) {
                    self.parse_primary()?
                } else {
                    self.parse_block_expr()?
                };
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                })
            }
            TokenKind::Do => {
                self.advance();
                let (effect, _) =
                    self.expect_ident("Expected capability name after 'do' (e.g. do Net.post(...))")?;
                self.expect(&TokenKind::Dot, "Expected '.' between capability and operation")?;
                let (op, _) = self.expect_ident("Expected effect operation name after '.'")?;
                let args = self.parse_arg_list()?;
                Ok(Expr::Perform { effect, op, args })
            }
            TokenKind::MemLoadKw => {
                self.advance();
                self.expect(&TokenKind::LParen, "Expected '(' after 'mem_load'")?;
                let index = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after mem_load(index)")?;
                Ok(Expr::MemLoad {
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
                Ok(Expr::MemStore {
                    index: Box::new(index),
                    value: Box::new(value),
                })
            }
            TokenKind::SecretKw => {
                self.advance();
                self.expect(&TokenKind::LParen, "Expected '(' after 'secret'")?;
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "Expected ')' after secret(expr)")?;
                Ok(Expr::SecretWrap(Box::new(inner)))
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
                Ok(Expr::Reveal {
                    domain,
                    expr: Box::new(inner),
                })
            }
            TokenKind::HashRef(hash) => {
                self.advance();
                let args = self.parse_arg_list()?;
                Ok(Expr::Call {
                    target: TargetRef::Hash(hash),
                    args,
                })
            }
            TokenKind::Ident(name) => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::LParen) {
                    let args = self.parse_arg_list()?;
                    let target = if name == "$self" {
                        TargetRef::SelfRec
                    } else {
                        TargetRef::Named(name)
                    };
                    Ok(Expr::Call { target, args })
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(AxiomDiagnostic::new(
                "E016_EXPECTED_EXPR",
                "parse",
                format!("Expected expression, found {other:?}"),
                tok.span,
                "Provide a valid expression (literal, variable, call, block, if/else, mem_load/mem_store, or 'do Effect.op(...)')",
            )),
        }
    }

    fn parse_arg_list(&mut self) -> Result<Vec<Expr>, AxiomDiagnostic> {
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
