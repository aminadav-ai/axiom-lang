// Copyright (c) 2026 Axiom Language Contributors
// License: BUSL-1.1

//! AST types for `Axiom.Forma` — the spatial-interactive UI layer.

use crate::ast::{EffectCap, Expr, Param, Type};
use crate::diag::Span;
use serde::{Deserialize, Serialize};

/// Cross-axis alignment inside `Row`, `Col`, `Box`, `Stack`, `Button`, and `Dropdown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

impl Align {
    pub fn from_ident(s: &str) -> Option<Self> {
        match s {
            "start" => Some(Align::Start),
            "center" => Some(Align::Center),
            "end" => Some(Align::End),
            "stretch" => Some(Align::Stretch),
            _ => None,
        }
    }
}

/// Built-in declarative `Axiom.Forma` node kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormaNodeKind {
    // Layout containers
    Box,
    Col,
    Row,
    Stack,
    Scroll,
    Overlay,
    // Interactive and leaf nodes
    Button,
    Dropdown,
    Spacer,
    Divider,
    Rect,
    Slot,
}

impl FormaNodeKind {
    pub fn from_ident(s: &str) -> Option<Self> {
        match s {
            "Box" => Some(Self::Box),
            "Col" => Some(Self::Col),
            "Row" => Some(Self::Row),
            "Stack" => Some(Self::Stack),
            "Scroll" => Some(Self::Scroll),
            "Overlay" => Some(Self::Overlay),
            "Button" => Some(Self::Button),
            "Dropdown" => Some(Self::Dropdown),
            "Spacer" => Some(Self::Spacer),
            "Divider" => Some(Self::Divider),
            "Rect" => Some(Self::Rect),
            "Slot" => Some(Self::Slot),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Col => "Col",
            Self::Row => "Row",
            Self::Stack => "Stack",
            Self::Scroll => "Scroll",
            Self::Overlay => "Overlay",
            Self::Button => "Button",
            Self::Dropdown => "Dropdown",
            Self::Spacer => "Spacer",
            Self::Divider => "Divider",
            Self::Rect => "Rect",
            Self::Slot => "Slot",
        }
    }

    /// Interactive nodes must prove non-degeneracy (`rw > 0 && rh > 0`) under Z3.
    pub fn is_interactive(self) -> bool {
        matches!(self, Self::Button | Self::Dropdown | Self::Scroll)
    }

    pub fn is_overlay(self) -> bool {
        matches!(self, Self::Overlay)
    }

    pub fn is_scroll(self) -> bool {
        matches!(self, Self::Scroll)
    }
}

/// Declarative properties on a `FormaNode`, generic over expression representation `E`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormaProps<E = Expr> {
    pub x: Option<E>,
    pub y: Option<E>,
    pub w: Option<E>,
    pub h: Option<E>,
    pub min_w: Option<E>,
    pub min_h: Option<E>,
    pub grow: Option<E>,
    pub pad: Option<E>,
    pub pad_x: Option<E>,
    pub pad_y: Option<E>,
    pub gap: Option<E>,
    pub align: Option<Align>,
    pub bg: Option<E>,
    pub hover_bg: Option<E>,
    pub border: Option<E>,
    pub border_col: Option<E>,
    pub radius: Option<E>,
    pub shadow: Option<E>,
    pub clip: Option<E>,
    pub id: Option<E>,
    pub on_click: Option<E>,
    pub visible: Option<E>,
    pub render: Option<String>,
    pub on_wheel: Option<String>,
}

impl<E> Default for FormaProps<E> {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            w: None,
            h: None,
            min_w: None,
            min_h: None,
            grow: None,
            pad: None,
            pad_x: None,
            pad_y: None,
            gap: None,
            align: None,
            bg: None,
            hover_bg: None,
            border: None,
            border_col: None,
            radius: None,
            shadow: None,
            clip: None,
            id: None,
            on_click: None,
            visible: None,
            render: None,
            on_wheel: None,
        }
    }
}

impl<E> FormaProps<E> {
    pub fn has_visual_rect(&self) -> bool {
        self.bg.is_some()
            || self.hover_bg.is_some()
            || self.border.is_some()
            || self.border_col.is_some()
            || self.shadow.is_some()
    }

    pub fn try_map<T, Err, F>(self, f: &mut F) -> Result<FormaProps<T>, Err>
    where
        F: FnMut(E) -> Result<T, Err>,
    {
        Ok(FormaProps {
            x: self.x.map(&mut *f).transpose()?,
            y: self.y.map(&mut *f).transpose()?,
            w: self.w.map(&mut *f).transpose()?,
            h: self.h.map(&mut *f).transpose()?,
            min_w: self.min_w.map(&mut *f).transpose()?,
            min_h: self.min_h.map(&mut *f).transpose()?,
            grow: self.grow.map(&mut *f).transpose()?,
            pad: self.pad.map(&mut *f).transpose()?,
            pad_x: self.pad_x.map(&mut *f).transpose()?,
            pad_y: self.pad_y.map(&mut *f).transpose()?,
            gap: self.gap.map(&mut *f).transpose()?,
            align: self.align,
            bg: self.bg.map(&mut *f).transpose()?,
            hover_bg: self.hover_bg.map(&mut *f).transpose()?,
            border: self.border.map(&mut *f).transpose()?,
            border_col: self.border_col.map(&mut *f).transpose()?,
            radius: self.radius.map(&mut *f).transpose()?,
            shadow: self.shadow.map(&mut *f).transpose()?,
            clip: self.clip.map(&mut *f).transpose()?,
            id: self.id.map(&mut *f).transpose()?,
            on_click: self.on_click.map(&mut *f).transpose()?,
            visible: self.visible.map(&mut *f).transpose()?,
            render: self.render,
            on_wheel: self.on_wheel,
        })
    }
}

/// A single node in a `forma` UI tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormaNode<E = Expr> {
    pub kind: FormaNodeKind,
    pub props: FormaProps<E>,
    pub children: Vec<FormaNode<E>>,
    pub span: Span,
}

impl<E> FormaNode<E> {
    pub fn try_map<T, Err, F>(self, f: &mut F) -> Result<FormaNode<T>, Err>
    where
        F: FnMut(E) -> Result<T, Err>,
    {
        let props = self.props.try_map(f)?;
        let mut children = Vec::with_capacity(self.children.len());
        for child in self.children {
            children.push(child.try_map(f)?);
        }
        Ok(FormaNode {
            kind: self.kind,
            props,
            children,
            span: self.span,
        })
    }

    /// Static upper bound on GPU primitives emitted by this subtree in a single frame.
    pub fn static_prim_upper_bound(&self) -> usize {
        let self_prims = match self.kind {
            FormaNodeKind::Spacer => 0,
            FormaNodeKind::Divider | FormaNodeKind::Rect => 1,
            FormaNodeKind::Slot => 64,
            FormaNodeKind::Box
            | FormaNodeKind::Col
            | FormaNodeKind::Row
            | FormaNodeKind::Stack
            | FormaNodeKind::Scroll
            | FormaNodeKind::Overlay
            | FormaNodeKind::Button
            | FormaNodeKind::Dropdown => usize::from(self.props.has_visual_rect()),
        };
        self_prims
            + self
                .children
                .iter()
                .map(FormaNode::static_prim_upper_bound)
                .sum::<usize>()
    }
}

/// Top-level `forma Name(...)` declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormaDef<E = Expr> {
    pub name: String,
    #[serde(default)]
    pub exported: bool,
    pub params: Vec<Param>,
    pub effects: Vec<EffectCap>,
    pub ret_type: Type,
    pub requires: Vec<E>,
    pub ensures: Vec<E>,
    pub root: FormaNode<E>,
    pub span: Span,
}

impl<E> FormaDef<E> {
    pub fn try_map<T, Err, F>(self, f: &mut F) -> Result<FormaDef<T>, Err>
    where
        F: FnMut(E) -> Result<T, Err>,
    {
        let mut requires = Vec::with_capacity(self.requires.len());
        for r in self.requires {
            requires.push(f(r)?);
        }
        let mut ensures = Vec::with_capacity(self.ensures.len());
        for e in self.ensures {
            ensures.push(f(e)?);
        }
        let root = self.root.try_map(f)?;
        Ok(FormaDef {
            name: self.name,
            exported: self.exported,
            params: self.params,
            effects: self.effects,
            ret_type: self.ret_type,
            requires,
            ensures,
            root,
            span: self.span,
        })
    }
}
