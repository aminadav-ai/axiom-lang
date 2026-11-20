// Copyright (c) 2026 Axiom Language Contributors
// License: BUSL-1.1

//! Parser for declarative `Axiom.Forma` UI blocks (`forma Name(...) { ... }`).

use crate::ast::{EffectCap, Param, Type};
use crate::diag::{AxiomDiagnostic, Span};
use crate::forma::ast::{Align, FormaDef, FormaNode, FormaNodeKind, FormaProps};
use crate::lexer::{Token, TokenKind};

/// Host parser interface supplied by `crate::parser::Parser` so `forma` blocks
/// reuse the full Axiom expression, type, parameter, and effect grammar.
pub(crate) trait FormaHostParser {
    type Expr;

    fn peek(&self) -> &Token;
    fn advance(&mut self) -> Token;
    fn expect_ident(&mut self, what: &str) -> Result<String, AxiomDiagnostic>;
    fn parse_params(&mut self) -> Result<Vec<Param>, AxiomDiagnostic>;
    fn parse_effect_and_ret(&mut self) -> Result<(Vec<EffectCap>, Type), AxiomDiagnostic>;
    fn parse_expr(&mut self) -> Result<Self::Expr, AxiomDiagnostic>;
}

fn expect_tok<P: FormaHostParser>(
    p: &mut P,
    expected: &TokenKind,
    desc: &str,
) -> Result<Span, AxiomDiagnostic> {
    if &p.peek().kind == expected {
        Ok(p.advance().span)
    } else {
        let tok = p.peek();
        Err(AxiomDiagnostic::new(
            "E091_FORMA_SYNTAX",
            "forma-parse",
            format!("Expected {desc} in forma block, found {:?}", tok.kind),
            tok.span,
            format!("Insert {desc} according to the Axiom.Forma grammar"),
        ))
    }
}

/// Parse a top-level `[export] forma Name(...) [req/ens]* [-[Eff]-> Ret] [req/ens]* { <FormaNode> }`.
pub(crate) fn parse_forma_def<P: FormaHostParser>(
    p: &mut P,
    exported: bool,
) -> Result<FormaDef<P::Expr>, AxiomDiagnostic> {
    let forma_span = expect_tok(p, &TokenKind::Forma, "'forma'")?;
    let name = p.expect_ident("forma component name")?;
    let params = p.parse_params()?;

    let mut requires = Vec::new();
    let mut ensures = Vec::new();
    parse_contracts(p, &mut requires, &mut ensures)?;

    let (effects, ret_type) =
        if matches!(p.peek().kind, TokenKind::EffectOpen | TokenKind::Arrow) {
            p.parse_effect_and_ret()?
        } else {
            (
                vec![EffectCap {
                    name: "Mem".to_string(),
                    scope: None,
                }],
                Type::I64,
            )
        };

    // Also allow contracts after the effect/return arrow (matching `fn` syntax).
    parse_contracts(p, &mut requires, &mut ensures)?;

    expect_tok(p, &TokenKind::LBrace, "'{' to open forma body")?;
    let root = parse_forma_node(p)?;
    expect_tok(p, &TokenKind::RBrace, "'}' to close forma body")?;

    Ok(FormaDef {
        name,
        exported,
        params,
        effects,
        ret_type,
        requires,
        ensures,
        root,
        span: forma_span,
    })
}

fn parse_contracts<P: FormaHostParser>(
    p: &mut P,
    requires: &mut Vec<P::Expr>,
    ensures: &mut Vec<P::Expr>,
) -> Result<(), AxiomDiagnostic> {
    loop {
        match p.peek().kind {
            TokenKind::Req => {
                p.advance();
                requires.push(p.parse_expr()?);
            }
            TokenKind::Ens => {
                p.advance();
                ensures.push(p.parse_expr()?);
            }
            _ => break,
        }
    }
    Ok(())
}

fn parse_forma_node<P: FormaHostParser>(p: &mut P) -> Result<FormaNode<P::Expr>, AxiomDiagnostic> {
    let span = p.peek().span;
    let kind_name = p.expect_ident("Forma node kind (Box, Col, Row, ...)")?;
    let kind = FormaNodeKind::from_ident(&kind_name).ok_or_else(|| {
        AxiomDiagnostic::new(
            "E091_FORMA_SYNTAX",
            "forma-parse",
            format!("Unknown Forma node kind '{kind_name}'"),
            span,
            "Use one of: Box, Col, Row, Stack, Scroll, Overlay, Button, Dropdown, Spacer, Divider, Rect, Slot",
        )
    })?;

    let props = if p.peek().kind == TokenKind::LParen {
        parse_forma_props(p)?
    } else {
        FormaProps::default()
    };

    let mut children = Vec::new();
    if p.peek().kind == TokenKind::LBrace {
        p.advance();
        while p.peek().kind != TokenKind::RBrace && p.peek().kind != TokenKind::Eof {
            children.push(parse_forma_node(p)?);
        }
        expect_tok(p, &TokenKind::RBrace, "'}' to close Forma node children")?;
    }

    Ok(FormaNode {
        kind,
        props,
        children,
        span,
    })
}

fn parse_forma_props<P: FormaHostParser>(p: &mut P) -> Result<FormaProps<P::Expr>, AxiomDiagnostic> {
    expect_tok(p, &TokenKind::LParen, "'('")?;
    let mut props = FormaProps::default();

    while p.peek().kind != TokenKind::RParen && p.peek().kind != TokenKind::Eof {
        let prop_span = p.peek().span;
        let prop_name = p.expect_ident("Forma property name")?;
        expect_tok(p, &TokenKind::Colon, "':' after property name")?;

        match prop_name.as_str() {
            "x" => props.x = Some(p.parse_expr()?),
            "y" => props.y = Some(p.parse_expr()?),
            "w" => props.w = Some(p.parse_expr()?),
            "h" => props.h = Some(p.parse_expr()?),
            "min_w" => props.min_w = Some(p.parse_expr()?),
            "min_h" => props.min_h = Some(p.parse_expr()?),
            "grow" => props.grow = Some(p.parse_expr()?),
            "pad" => props.pad = Some(p.parse_expr()?),
            "pad_x" => props.pad_x = Some(p.parse_expr()?),
            "pad_y" => props.pad_y = Some(p.parse_expr()?),
            "gap" => props.gap = Some(p.parse_expr()?),
            "align" => {
                let a_span = p.peek().span;
                let a_name = p.expect_ident("alignment (start | center | end | stretch)")?;
                let align = Align::from_ident(&a_name).ok_or_else(|| {
                    AxiomDiagnostic::new(
                        "E091_FORMA_SYNTAX",
                        "forma-parse",
                        format!("Invalid alignment '{a_name}'"),
                        a_span,
                        "Use one of: start, center, end, stretch",
                    )
                })?;
                props.align = Some(align);
            }
            "bg" => props.bg = Some(p.parse_expr()?),
            "hover_bg" => props.hover_bg = Some(p.parse_expr()?),
            "border" => props.border = Some(p.parse_expr()?),
            "border_col" => props.border_col = Some(p.parse_expr()?),
            "radius" => props.radius = Some(p.parse_expr()?),
            "shadow" => props.shadow = Some(p.parse_expr()?),
            "clip" => props.clip = Some(p.parse_expr()?),
            "id" => props.id = Some(p.parse_expr()?),
            "on_click" => props.on_click = Some(p.parse_expr()?),
            "visible" => props.visible = Some(p.parse_expr()?),
            "render" => {
                let fn_name = p.expect_ident("Slot render function name")?;
                props.render = Some(fn_name);
            }
            "on_wheel" => {
                let fn_name = p.expect_ident("Slot on_wheel function name")?;
                props.on_wheel = Some(fn_name);
            }
            other => {
                return Err(AxiomDiagnostic::new(
                    "E091_FORMA_SYNTAX",
                    "forma-parse",
                    format!("Unknown Forma property '{other}'"),
                    prop_span,
                    "Supported properties: x, y, w, h, min_w, min_h, grow, pad, pad_x, pad_y, gap, align, bg, hover_bg, border, border_col, radius, shadow, clip, id, on_click, visible, render, on_wheel",
                ));
            }
        }

        if p.peek().kind == TokenKind::Comma {
            p.advance();
        } else {
            break;
        }
    }

    expect_tok(p, &TokenKind::RParen, "')' to close Forma properties")?;
    Ok(props)
}
