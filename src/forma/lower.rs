// Copyright (c) 2026 Axiom Language Contributors
// License: BUSL-1.1

//! Lowering pass from `Axiom.Forma` declarative UI trees into pure `Axiom.Flux` / `Axiom.Strata`
//! functions (`FnDef`) compatible with the `axiom-web` 1-Draw-Call GPU primitive protocol.

use crate::ast::{BinOp, EffectCap, Expr, FnDef, Param, Stmt, TargetRef, Type, UnaryOp};
use crate::forma::ast::{Align, FormaDef, FormaNode, FormaNodeKind};

/// Linear memory addresses for the `Axiom.Forma` input/hover register block (free region
/// between hardware clip rects `48000..48063` and the GPU primitive stream `65536..`).
pub const FORMA_MEM_MX: i64 = 48_100;
pub const FORMA_MEM_MY: i64 = 48_101;
pub const FORMA_MEM_CLICKED: i64 = 48_102;
pub const FORMA_MEM_WHEEL: i64 = 48_103;
pub const FORMA_MEM_HOVERED_OUT: i64 = 48_104;

const HELPER_CLAMP: &str = "$forma_clamp";
const HELPER_SET_CLIP: &str = "$forma_set_clip";
const HELPER_PUSH_RECT: &str = "$forma_push_rect";

#[derive(Debug, Clone)]
struct LoweredNodeInfo {
    id: usize,
    kind: FormaNodeKind,
    x_var: String,
    y_var: String,
    w_var: String,
    h_var: String,
    vis_var: String,
    clip_var: String,
    clip_prop: Option<Expr>,
    clip_x0: String,
    clip_y0: String,
    clip_x1: String,
    clip_y1: String,
    in_overlay: bool,
    overlay_group: Option<usize>,
    bg: Option<Expr>,
    hover_bg: Option<Expr>,
    border: Option<Expr>,
    border_col: Option<Expr>,
    radius: Option<Expr>,
    shadow: Option<Expr>,
    has_visual_rect: bool,
    node_id_expr: Option<Expr>,
    on_click_expr: Option<Expr>,
    render_fn: Option<String>,
    on_wheel_fn: Option<String>,
}

struct FormaLowerer {
    stmts: Vec<Stmt>,
    nodes: Vec<LoweredNodeInfo>,
    next_id: usize,
    next_overlay_group: usize,
}

impl FormaLowerer {
    fn new() -> Self {
        Self {
            stmts: Vec::new(),
            nodes: Vec::new(),
            next_id: 0,
            next_overlay_group: 0,
        }
    }

    fn alloc_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn emit_let(&mut self, name: &str, value: Expr) -> String {
        self.stmts.push(Stmt::Let {
            name: name.to_string(),
            mutable: false,
            ty: None,
            value,
        });
        name.to_string()
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_node_geometry(
        &mut self,
        node: &FormaNode,
        rx_expr: Expr,
        ry_expr: Expr,
        rw_expr: Expr,
        rh_expr: Expr,
        parent_vis: Expr,
        parent_clip_id: Expr,
        parent_clip_rect: (String, String, String, String),
        root_clip_rect: &(String, String, String, String),
        in_overlay: bool,
        overlay_group: Option<usize>,
    ) {
        let nid = self.alloc_id();
        let x_var = self.emit_let(&format!("_fx_{nid}"), rx_expr);
        let y_var = self.emit_let(&format!("_fy_{nid}"), ry_expr);
        let w_var = self.emit_let(&format!("_fw_{nid}"), rw_expr);
        let h_var = self.emit_let(&format!("_fh_{nid}"), rh_expr);

        let vis_expr = match &node.props.visible {
            Some(v) => bin(BinOp::And, parent_vis, v.clone()),
            None => parent_vis,
        };
        let vis_var = self.emit_let(&format!("_fv_{nid}"), vis_expr);

        let (clip_var, clip_x0, clip_y0, clip_x1, clip_y1) = if let Some(cid_expr) = &node.props.clip
        {
            let cv = self.emit_let(&format!("_fc_{nid}"), cid_expr.clone());
            let (pcx0, pcy0, pcx1, pcy1) = parent_clip_rect;
            let cx0 = self.emit_let(
                &format!("_fcx0_{nid}"),
                max_expr(var(&pcx0), var(&x_var)),
            );
            let cy0 = self.emit_let(
                &format!("_fcy0_{nid}"),
                max_expr(var(&pcy0), var(&y_var)),
            );
            let cx1 = self.emit_let(
                &format!("_fcx1_{nid}"),
                min_expr(var(&pcx1), bin(BinOp::Add, var(&x_var), var(&w_var))),
            );
            let cy1 = self.emit_let(
                &format!("_fcy1_{nid}"),
                min_expr(var(&pcy1), bin(BinOp::Add, var(&y_var), var(&h_var))),
            );
            (cv, cx0, cy0, cx1, cy1)
        } else {
            let cv = self.emit_let(&format!("_fc_{nid}"), parent_clip_id);
            (
                cv,
                parent_clip_rect.0,
                parent_clip_rect.1,
                parent_clip_rect.2,
                parent_clip_rect.3,
            )
        };

        self.nodes.push(LoweredNodeInfo {
            id: nid,
            kind: node.kind,
            x_var: x_var.clone(),
            y_var: y_var.clone(),
            w_var: w_var.clone(),
            h_var: h_var.clone(),
            vis_var: vis_var.clone(),
            clip_var: clip_var.clone(),
            clip_prop: node.props.clip.clone(),
            clip_x0: clip_x0.clone(),
            clip_y0: clip_y0.clone(),
            clip_x1: clip_x1.clone(),
            clip_y1: clip_y1.clone(),
            in_overlay,
            overlay_group,
            bg: node.props.bg.clone(),
            hover_bg: node.props.hover_bg.clone(),
            border: node.props.border.clone(),
            border_col: node.props.border_col.clone(),
            radius: node.props.radius.clone(),
            shadow: node.props.shadow.clone(),
            has_visual_rect: node.props.has_visual_rect(),
            node_id_expr: node.props.id.clone(),
            on_click_expr: node.props.on_click.clone(),
            render_fn: node.props.render.clone(),
            on_wheel_fn: node.props.on_wheel.clone(),
        });

        if node.children.is_empty() {
            return;
        }

        let pad_default = node.props.pad.clone().unwrap_or(Expr::IntLit(0));
        let pad_x_expr = node.props.pad_x.clone().unwrap_or_else(|| pad_default.clone());
        let pad_y_expr = node.props.pad_y.clone().unwrap_or(pad_default);

        let px_var = self.emit_let(&format!("_fpx_{nid}"), pad_x_expr);
        let py_var = self.emit_let(&format!("_fpy_{nid}"), pad_y_expr);
        let ix_var = self.emit_let(
            &format!("_fix_{nid}"),
            bin(BinOp::Add, var(&x_var), var(&px_var)),
        );
        let iy_var = self.emit_let(
            &format!("_fiy_{nid}"),
            bin(BinOp::Add, var(&y_var), var(&py_var)),
        );
        let iw_var = self.emit_let(
            &format!("_fiw_{nid}"),
            bin(
                BinOp::Sub,
                var(&w_var),
                bin(BinOp::Mul, Expr::IntLit(2), var(&px_var)),
            ),
        );
        let ih_var = self.emit_let(
            &format!("_fih_{nid}"),
            bin(
                BinOp::Sub,
                var(&h_var),
                bin(BinOp::Mul, Expr::IntLit(2), var(&py_var)),
            ),
        );

        let active_clip_rect = (clip_x0, clip_y0, clip_x1, clip_y1);
        let flow_children: Vec<&FormaNode> = node
            .children
            .iter()
            .filter(|c| !c.kind.is_overlay())
            .collect();
        let overlay_children: Vec<&FormaNode> = node
            .children
            .iter()
            .filter(|c| c.kind.is_overlay())
            .collect();

        let align = node.props.align.unwrap_or(Align::Start);

        match node.kind {
            FormaNodeKind::Row => {
                self.lower_row(
                    nid,
                    node,
                    &flow_children,
                    &ix_var,
                    &iy_var,
                    &iw_var,
                    &ih_var,
                    align,
                    &vis_var,
                    &clip_var,
                    &active_clip_rect,
                    root_clip_rect,
                    in_overlay,
                    overlay_group,
                );
            }
            FormaNodeKind::Col => {
                self.lower_col(
                    nid,
                    node,
                    &flow_children,
                    &ix_var,
                    &iy_var,
                    &iw_var,
                    &ih_var,
                    align,
                    &vis_var,
                    &clip_var,
                    &active_clip_rect,
                    root_clip_rect,
                    in_overlay,
                    overlay_group,
                );
            }
            _ => {
                for (ci, child) in flow_children.iter().enumerate() {
                    let cw_var = self.emit_let(
                        &format!("_fcw_{nid}_{ci}"),
                        child.props.w.clone().unwrap_or_else(|| var(&iw_var)),
                    );
                    let ch_var = self.emit_let(
                        &format!("_fch_{nid}_{ci}"),
                        child.props.h.clone().unwrap_or_else(|| var(&ih_var)),
                    );
                    let cx_expr = child
                        .props
                        .x
                        .clone()
                        .unwrap_or_else(|| align_pos_expr(var(&ix_var), var(&iw_var), var(&cw_var), align));
                    let cy_expr = child
                        .props
                        .y
                        .clone()
                        .unwrap_or_else(|| align_pos_expr(var(&iy_var), var(&ih_var), var(&ch_var), align));
                    self.lower_node_geometry(
                        child,
                        cx_expr,
                        cy_expr,
                        var(&cw_var),
                        var(&ch_var),
                        var(&vis_var),
                        var(&clip_var),
                        active_clip_rect.clone(),
                        root_clip_rect,
                        in_overlay,
                        overlay_group,
                    );
                }
            }
        }

        for oc in overlay_children {
            let group_id = self.next_overlay_group;
            self.next_overlay_group += 1;
            let ox = oc.props.x.clone().unwrap_or_else(|| var(&x_var));
            let oy = oc.props.y.clone().unwrap_or_else(|| var(&y_var));
            let ow = oc.props.w.clone().unwrap_or_else(|| var(&w_var));
            let oh = oc.props.h.clone().unwrap_or_else(|| var(&h_var));
            self.lower_node_geometry(
                oc,
                ox,
                oy,
                ow,
                oh,
                var(&vis_var),
                Expr::IntLit(0),
                root_clip_rect.clone(),
                root_clip_rect,
                true,
                Some(group_id),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_row(
        &mut self,
        nid: usize,
        parent: &FormaNode,
        children: &[&FormaNode],
        ix_var: &str,
        iy_var: &str,
        iw_var: &str,
        ih_var: &str,
        align: Align,
        vis_var: &str,
        clip_var: &str,
        active_clip_rect: &(String, String, String, String),
        root_clip_rect: &(String, String, String, String),
        in_overlay: bool,
        overlay_group: Option<usize>,
    ) {
        if children.is_empty() {
            return;
        }
        let gap_var = self.emit_let(
            &format!("_fgap_{nid}"),
            parent.props.gap.clone().unwrap_or(Expr::IntLit(0)),
        );
        let n = children.len();
        let total_gap = if n > 1 {
            bin(BinOp::Mul, Expr::IntLit((n - 1) as i64), var(&gap_var))
        } else {
            Expr::IntLit(0)
        };

        let mut fixed_terms = Vec::new();
        let mut grow_weights: Vec<Option<Expr>> = Vec::with_capacity(n);
        let mut grow_terms = Vec::new();
        for child in children {
            if let Some(w_expr) = &child.props.w {
                fixed_terms.push(w_expr.clone());
                grow_weights.push(None);
            } else {
                let g = child.props.grow.clone().unwrap_or(Expr::IntLit(1));
                grow_terms.push(g.clone());
                grow_weights.push(Some(g));
            }
        }
        let fixed_w = sum_exprs(&fixed_terms);
        let flex_count = grow_terms.len();
        let total_grow_var = if flex_count > 0 {
            Some(self.emit_let(&format!("_ftg_{nid}"), sum_exprs(&grow_terms)))
        } else {
            None
        };
        let rem_w_var = if flex_count > 0 {
            Some(self.emit_let(
                &format!("_frem_{nid}"),
                bin(BinOp::Sub, bin(BinOp::Sub, var(iw_var), fixed_w), total_gap),
            ))
        } else {
            None
        };

        let mut cur_x_var = self.emit_let(&format!("_fcur_{nid}_0"), var(ix_var));
        let mut alloc_terms = Vec::new();
        let mut flex_seen = 0usize;

        for (ci, child) in children.iter().enumerate() {
            let cw_expr = if let Some(w_expr) = &child.props.w {
                w_expr.clone()
            } else {
                let g = grow_weights[ci].clone().unwrap();
                let rem_v = rem_w_var.as_ref().unwrap();
                let tg_v = total_grow_var.as_ref().unwrap();
                flex_seen += 1;
                if flex_seen == flex_count {
                    bin(BinOp::Sub, var(rem_v), sum_exprs(&alloc_terms))
                } else {
                    let share_var = self.emit_let(
                        &format!("_fsh_{nid}_{ci}"),
                        Expr::If {
                            cond: Box::new(bin(BinOp::Gt, var(tg_v), Expr::IntLit(0))),
                            then_branch: Box::new(bin(
                                BinOp::Div,
                                bin(BinOp::Mul, var(rem_v), g),
                                var(tg_v),
                            )),
                            else_branch: Box::new(Expr::IntLit(0)),
                        },
                    );
                    alloc_terms.push(var(&share_var));
                    var(&share_var)
                }
            };
            let cw_var = self.emit_let(&format!("_frw_{nid}_{ci}"), cw_expr);
            let ch_var = self.emit_let(
                &format!("_frh_{nid}_{ci}"),
                child.props.h.clone().unwrap_or_else(|| var(ih_var)),
            );
            let cy_expr = align_pos_expr(var(iy_var), var(ih_var), var(&ch_var), align);

            self.lower_node_geometry(
                child,
                var(&cur_x_var),
                cy_expr,
                var(&cw_var),
                var(&ch_var),
                var(vis_var),
                var(clip_var),
                active_clip_rect.clone(),
                root_clip_rect,
                in_overlay,
                overlay_group,
            );

            if ci + 1 < n {
                cur_x_var = self.emit_let(
                    &format!("_fcur_{nid}_{}", ci + 1),
                    bin(
                        BinOp::Add,
                        bin(BinOp::Add, var(&cur_x_var), var(&cw_var)),
                        var(&gap_var),
                    ),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_col(
        &mut self,
        nid: usize,
        parent: &FormaNode,
        children: &[&FormaNode],
        ix_var: &str,
        iy_var: &str,
        iw_var: &str,
        ih_var: &str,
        align: Align,
        vis_var: &str,
        clip_var: &str,
        active_clip_rect: &(String, String, String, String),
        root_clip_rect: &(String, String, String, String),
        in_overlay: bool,
        overlay_group: Option<usize>,
    ) {
        if children.is_empty() {
            return;
        }
        let gap_var = self.emit_let(
            &format!("_fgap_{nid}"),
            parent.props.gap.clone().unwrap_or(Expr::IntLit(0)),
        );
        let n = children.len();
        let total_gap = if n > 1 {
            bin(BinOp::Mul, Expr::IntLit((n - 1) as i64), var(&gap_var))
        } else {
            Expr::IntLit(0)
        };

        let mut fixed_terms = Vec::new();
        let mut grow_weights: Vec<Option<Expr>> = Vec::with_capacity(n);
        let mut grow_terms = Vec::new();
        for child in children {
            if let Some(h_expr) = &child.props.h {
                fixed_terms.push(h_expr.clone());
                grow_weights.push(None);
            } else {
                let g = child.props.grow.clone().unwrap_or(Expr::IntLit(1));
                grow_terms.push(g.clone());
                grow_weights.push(Some(g));
            }
        }
        let fixed_h = sum_exprs(&fixed_terms);
        let flex_count = grow_terms.len();
        let total_grow_var = if flex_count > 0 {
            Some(self.emit_let(&format!("_ftg_{nid}"), sum_exprs(&grow_terms)))
        } else {
            None
        };
        let rem_h_var = if flex_count > 0 {
            Some(self.emit_let(
                &format!("_frem_{nid}"),
                bin(BinOp::Sub, bin(BinOp::Sub, var(ih_var), fixed_h), total_gap),
            ))
        } else {
            None
        };

        let mut cur_y_var = self.emit_let(&format!("_fcur_{nid}_0"), var(iy_var));
        let mut alloc_terms = Vec::new();
        let mut flex_seen = 0usize;

        for (ci, child) in children.iter().enumerate() {
            let ch_expr = if let Some(h_expr) = &child.props.h {
                h_expr.clone()
            } else {
                let g = grow_weights[ci].clone().unwrap();
                let rem_v = rem_h_var.as_ref().unwrap();
                let tg_v = total_grow_var.as_ref().unwrap();
                flex_seen += 1;
                if flex_seen == flex_count {
                    bin(BinOp::Sub, var(rem_v), sum_exprs(&alloc_terms))
                } else {
                    let share_var = self.emit_let(
                        &format!("_fsh_{nid}_{ci}"),
                        Expr::If {
                            cond: Box::new(bin(BinOp::Gt, var(tg_v), Expr::IntLit(0))),
                            then_branch: Box::new(bin(
                                BinOp::Div,
                                bin(BinOp::Mul, var(rem_v), g),
                                var(tg_v),
                            )),
                            else_branch: Box::new(Expr::IntLit(0)),
                        },
                    );
                    alloc_terms.push(var(&share_var));
                    var(&share_var)
                }
            };
            let ch_var = self.emit_let(&format!("_fkh_{nid}_{ci}"), ch_expr);
            let cw_var = self.emit_let(
                &format!("_fkw_{nid}_{ci}"),
                child.props.w.clone().unwrap_or_else(|| var(iw_var)),
            );
            let cx_expr = align_pos_expr(var(ix_var), var(iw_var), var(&cw_var), align);

            self.lower_node_geometry(
                child,
                cx_expr,
                var(&cur_y_var),
                var(&cw_var),
                var(&ch_var),
                var(vis_var),
                var(clip_var),
                active_clip_rect.clone(),
                root_clip_rect,
                in_overlay,
                overlay_group,
            );

            if ci + 1 < n {
                cur_y_var = self.emit_let(
                    &format!("_fcur_{nid}_{}", ci + 1),
                    bin(
                        BinOp::Add,
                        bin(BinOp::Add, var(&cur_y_var), var(&ch_var)),
                        var(&gap_var),
                    ),
                );
            }
        }
    }
}

/// Lower a `FormaDef` into a pure `Axiom.Flux` / `Axiom.Strata` `FnDef`.
pub fn lower_forma_def(forma: &FormaDef) -> FnDef {
    let mut lowerer = FormaLowerer::new();

    // Resolve mouse / click / wheel inputs from explicit params or Forma linear memory mailbox.
    let has_param = |name: &str| forma.params.iter().any(|p| p.name == name && p.ty == Type::I64);
    let mx_expr = if has_param("mx") {
        var("mx")
    } else {
        mem_load(FORMA_MEM_MX)
    };
    let my_expr = if has_param("my") {
        var("my")
    } else {
        mem_load(FORMA_MEM_MY)
    };
    let click_expr = if has_param("m_clicked") {
        var("m_clicked")
    } else if has_param("clicked") {
        var("clicked")
    } else {
        mem_load(FORMA_MEM_CLICKED)
    };
    let wheel_expr = if has_param("wheel_dy") {
        var("wheel_dy")
    } else {
        mem_load(FORMA_MEM_WHEEL)
    };

    lowerer.emit_let("_f_mx", mx_expr);
    lowerer.emit_let("_f_my", my_expr);
    lowerer.emit_let("_f_click", click_expr);
    lowerer.emit_let("_f_wheel", wheel_expr);

    lowerer.stmts.push(Stmt::Let {
        name: "_f_clicked".to_string(),
        mutable: true,
        ty: Some(Type::I64),
        value: Expr::IntLit(0),
    });
    lowerer.stmts.push(Stmt::Let {
        name: "_f_hovered".to_string(),
        mutable: true,
        ty: Some(Type::I64),
        value: Expr::IntLit(0),
    });
    lowerer.stmts.push(Stmt::Let {
        name: "_f_ov_hit".to_string(),
        mutable: true,
        ty: Some(Type::Bool),
        value: Expr::BoolLit(false),
    });

    // Step 1: Compute single-source-of-truth geometry for all nodes.
    let rx0 = forma.root.props.x.clone().unwrap_or(Expr::IntLit(0));
    let ry0 = forma.root.props.y.clone().unwrap_or(Expr::IntLit(0));
    let rw0 = forma.root.props.w.clone().unwrap_or(Expr::IntLit(0));
    let rh0 = forma.root.props.h.clone().unwrap_or(Expr::IntLit(0));
    let rcx0 = lowerer.emit_let("_f_root_cx0", rx0.clone());
    let rcy0 = lowerer.emit_let("_f_root_cy0", ry0.clone());
    let rcx1 = lowerer.emit_let("_f_root_cx1", bin(BinOp::Add, var(&rcx0), rw0.clone()));
    let rcy1 = lowerer.emit_let("_f_root_cy1", bin(BinOp::Add, var(&rcy0), rh0.clone()));
    let root_clip_rect = (rcx0, rcy0, rcx1, rcy1);

    lowerer.lower_node_geometry(
        &forma.root,
        rx0,
        ry0,
        rw0,
        rh0,
        Expr::BoolLit(true),
        Expr::IntLit(0),
        root_clip_rect.clone(),
        &root_clip_rect,
        false,
        None,
    );

    // Step 2: Priority hit-test for visible Overlay nodes (top-most first).
    let overlay_nodes: Vec<LoweredNodeInfo> = lowerer
        .nodes
        .iter()
        .filter(|n| n.in_overlay)
        .cloned()
        .collect();
    for n in overlay_nodes.iter().rev() {
        let hit_cond = bin(
            BinOp::And,
            var(&n.vis_var),
            point_in_rect("_f_mx", "_f_my", &n.x_var, &n.y_var, &n.w_var, &n.h_var),
        );
        if n.kind == FormaNodeKind::Overlay {
            lowerer.stmts.push(Stmt::Expr(if_unit(
                hit_cond,
                vec![Stmt::Assign {
                    name: "_f_ov_hit".to_string(),
                    value: Expr::BoolLit(true),
                }],
            )));
        } else if matches!(n.kind, FormaNodeKind::Button | FormaNodeKind::Dropdown) {
            emit_interactive_hit(&mut lowerer.stmts, n, hit_cond, true);
        }
    }

    // Step 3: Normal tree hit-test (suppressed if an Overlay intercepted the pointer).
    let normal_nodes: Vec<LoweredNodeInfo> = lowerer
        .nodes
        .iter()
        .filter(|n| !n.in_overlay)
        .cloned()
        .collect();
    for n in normal_nodes.iter().rev() {
        let not_ov = Expr::Unary {
            op: UnaryOp::Not,
            expr: Box::new(var("_f_ov_hit")),
        };
        let hit_cond = bin(
            BinOp::And,
            bin(BinOp::And, not_ov, var(&n.vis_var)),
            point_in_rect("_f_mx", "_f_my", &n.x_var, &n.y_var, &n.w_var, &n.h_var),
        );
        if matches!(n.kind, FormaNodeKind::Button | FormaNodeKind::Dropdown) {
            emit_interactive_hit(&mut lowerer.stmts, n, hit_cond, false);
        } else if n.kind == FormaNodeKind::Slot {
            if let Some(wheel_fn) = &n.on_wheel_fn {
                let wheel_cond = bin(
                    BinOp::And,
                    bin(BinOp::Ne, var("_f_wheel"), Expr::IntLit(0)),
                    hit_cond,
                );
                lowerer.stmts.push(Stmt::Expr(if_unit(
                    wheel_cond,
                    vec![Stmt::Expr(call_named(
                        wheel_fn,
                        vec![
                            var(&n.x_var),
                            var(&n.y_var),
                            var(&n.w_var),
                            var(&n.h_var),
                            var(&n.clip_var),
                        ],
                    ))],
                )));
            }
        }
    }

    // Step 4: Render normal tree in paint order (1-Draw-Call GPU stream).
    for n in &normal_nodes {
        emit_render_node(&mut lowerer.stmts, n);
    }

    // Step 5: Deferred Overlay render pass (guaranteed above all normal nodes).
    let mut max_group = 0usize;
    for n in &overlay_nodes {
        if let Some(g) = n.overlay_group {
            max_group = max_group.max(g + 1);
        }
    }
    for g in 0..max_group {
        for n in overlay_nodes.iter().filter(|n| n.overlay_group == Some(g)) {
            emit_render_node(&mut lowerer.stmts, n);
        }
    }

    // Step 6: Store hovered ID register and return clicked action ID (or unit).
    lowerer.stmts.push(Stmt::Expr(Expr::MemStore {
        index: Box::new(Expr::IntLit(FORMA_MEM_HOVERED_OUT)),
        value: Box::new(var("_f_hovered")),
    }));

    let tail = if forma.ret_type == Type::Unit {
        Expr::UnitLit
    } else {
        var("_f_clicked")
    };

    let mut effects = forma.effects.clone();
    if !effects.iter().any(|e| e.name == "Mem") {
        effects.push(EffectCap {
            name: "Mem".to_string(),
            scope: None,
        });
    }

    FnDef {
        name: forma.name.clone(),
        exported: forma.exported,
        params: forma.params.clone(),
        effects,
        ret_type: forma.ret_type.clone(),
        requires: forma.requires.clone(),
        ensures: forma.ensures.clone(),
        body: Expr::Block {
            stmts: lowerer.stmts,
            tail: Box::new(tail),
        },
    }
}

fn emit_interactive_hit(
    stmts: &mut Vec<Stmt>,
    n: &LoweredNodeInfo,
    hit_cond: Expr,
    is_overlay: bool,
) {
    let hover_id = n
        .node_id_expr
        .clone()
        .unwrap_or(Expr::IntLit((n.id + 1) as i64));
    let click_id = n
        .on_click_expr
        .clone()
        .or_else(|| n.node_id_expr.clone())
        .unwrap_or(Expr::IntLit((n.id + 1) as i64));

    let mut body = Vec::new();
    if is_overlay {
        body.push(Stmt::Assign {
            name: "_f_ov_hit".to_string(),
            value: Expr::BoolLit(true),
        });
    }
    body.push(Stmt::Expr(if_unit(
        bin(BinOp::Eq, var("_f_hovered"), Expr::IntLit(0)),
        vec![Stmt::Assign {
            name: "_f_hovered".to_string(),
            value: hover_id,
        }],
    )));
    body.push(Stmt::Expr(if_unit(
        bin(
            BinOp::And,
            bin(BinOp::Ne, var("_f_click"), Expr::IntLit(0)),
            bin(BinOp::Eq, var("_f_clicked"), Expr::IntLit(0)),
        ),
        vec![Stmt::Assign {
            name: "_f_clicked".to_string(),
            value: click_id,
        }],
    )));
    stmts.push(Stmt::Expr(if_unit(hit_cond, body)));
}

fn emit_render_node(stmts: &mut Vec<Stmt>, n: &LoweredNodeInfo) {
    let mut body = Vec::new();

    if let Some(cid_expr) = &n.clip_prop {
        body.push(Stmt::Expr(call_named(
            HELPER_SET_CLIP,
            vec![
                cid_expr.clone(),
                var(&n.clip_x0),
                var(&n.clip_y0),
                var(&n.clip_x1),
                var(&n.clip_y1),
            ],
        )));
    }

    match n.kind {
        FormaNodeKind::Spacer => {}
        FormaNodeKind::Divider => {
            let col = n
                .bg
                .clone()
                .or_else(|| n.border_col.clone())
                .unwrap_or(Expr::IntLit(0x44FFFFFF));
            body.push(Stmt::Expr(call_named(
                HELPER_PUSH_RECT,
                vec![
                    var(&n.x_var),
                    var(&n.y_var),
                    var(&n.w_var),
                    var(&n.h_var),
                    Expr::IntLit(0),
                    Expr::IntLit(0),
                    Expr::IntLit(0),
                    col,
                    Expr::IntLit(0),
                    var(&n.clip_var),
                ],
            )));
        }
        FormaNodeKind::Slot => {
            if let Some(render_fn) = &n.render_fn {
                body.push(Stmt::Expr(call_named(
                    render_fn,
                    vec![
                        var(&n.x_var),
                        var(&n.y_var),
                        var(&n.w_var),
                        var(&n.h_var),
                        var(&n.clip_var),
                    ],
                )));
            }
        }
        _ => {
            if n.has_visual_rect || n.kind == FormaNodeKind::Rect {
                let base_bg = n.bg.clone().unwrap_or(Expr::IntLit(0));
                let fill_expr = if let Some(hbg) = &n.hover_bg {
                    let hover_id = n
                        .node_id_expr
                        .clone()
                        .unwrap_or(Expr::IntLit((n.id + 1) as i64));
                    Expr::If {
                        cond: Box::new(bin(BinOp::Eq, var("_f_hovered"), hover_id)),
                        then_branch: Box::new(hbg.clone()),
                        else_branch: Box::new(base_bg),
                    }
                } else {
                    base_bg
                };
                let radius = n.radius.clone().unwrap_or(Expr::IntLit(0));
                let border = n.border.clone().unwrap_or(Expr::IntLit(0));
                let shadow = n.shadow.clone().unwrap_or(Expr::IntLit(0));
                let border_col = n.border_col.clone().unwrap_or(Expr::IntLit(0));
                body.push(Stmt::Expr(call_named(
                    HELPER_PUSH_RECT,
                    vec![
                        var(&n.x_var),
                        var(&n.y_var),
                        var(&n.w_var),
                        var(&n.h_var),
                        radius,
                        border,
                        shadow,
                        fill_expr,
                        border_col,
                        var(&n.clip_var),
                    ],
                )));
            }
        }
    }

    if !body.is_empty() {
        stmts.push(Stmt::Expr(if_unit(var(&n.vis_var), body)));
    }
}

/// Generate the shared `Axiom.Forma` runtime helper functions (`$forma_clamp`, `$forma_set_clip`,
/// `$forma_push_rect`) targeting the `axiom-web` 1-Draw-Call linear memory ABI.
pub fn forma_runtime_helpers() -> Vec<FnDef> {
    let mem_eff = vec![EffectCap {
        name: "Mem".to_string(),
        scope: None,
    }];

    // fn $forma_clamp(v: i64, lo: i64, hi: i64) -> i64
    let clamp_fn = FnDef {
        name: HELPER_CLAMP.to_string(),
        exported: false,
        params: vec![
            Param {
                name: "v".to_string(),
                ty: Type::I64,
            },
            Param {
                name: "lo".to_string(),
                ty: Type::I64,
            },
            Param {
                name: "hi".to_string(),
                ty: Type::I64,
            },
        ],
        effects: Vec::new(),
        ret_type: Type::I64,
        requires: Vec::new(),
        ensures: Vec::new(),
        body: Expr::If {
            cond: Box::new(bin(BinOp::Lt, var("v"), var("lo"))),
            then_branch: Box::new(var("lo")),
            else_branch: Box::new(Expr::If {
                cond: Box::new(bin(BinOp::Gt, var("v"), var("hi"))),
                then_branch: Box::new(var("hi")),
                else_branch: Box::new(var("v")),
            }),
        },
    };

    // fn $forma_set_clip(cid: i64, x0: i64, y0: i64, x1: i64, y1: i64) -[Mem]-> unit
    let set_clip_fn = FnDef {
        name: HELPER_SET_CLIP.to_string(),
        exported: false,
        params: vec![
            Param {
                name: "cid".to_string(),
                ty: Type::I64,
            },
            Param {
                name: "x0".to_string(),
                ty: Type::I64,
            },
            Param {
                name: "y0".to_string(),
                ty: Type::I64,
            },
            Param {
                name: "x1".to_string(),
                ty: Type::I64,
            },
            Param {
                name: "y1".to_string(),
                ty: Type::I64,
            },
        ],
        effects: mem_eff.clone(),
        ret_type: Type::Unit,
        requires: Vec::new(),
        ensures: Vec::new(),
        body: if_unit(
            bin(
                BinOp::And,
                bin(BinOp::Ge, var("cid"), Expr::IntLit(0)),
                bin(BinOp::Lt, var("cid"), Expr::IntLit(16)),
            ),
            vec![
                Stmt::Let {
                    name: "b".to_string(),
                    mutable: false,
                    ty: None,
                    value: bin(
                        BinOp::Add,
                        Expr::IntLit(48_000),
                        bin(BinOp::Mul, var("cid"), Expr::IntLit(4)),
                    ),
                },
                mem_store_stmt(var("b"), var("x0")),
                mem_store_stmt(bin(BinOp::Add, var("b"), Expr::IntLit(1)), var("y0")),
                mem_store_stmt(bin(BinOp::Add, var("b"), Expr::IntLit(2)), var("x1")),
                mem_store_stmt(bin(BinOp::Add, var("b"), Expr::IntLit(3)), var("y1")),
            ],
        ),
    };

    // fn $forma_push_rect(x, y, w, h, radius, border, shadow, fill, border_col, clip_id) -[Mem]-> unit
    let push_rect_fn = FnDef {
        name: HELPER_PUSH_RECT.to_string(),
        exported: false,
        params: vec![
            Param { name: "x".to_string(), ty: Type::I64 },
            Param { name: "y".to_string(), ty: Type::I64 },
            Param { name: "w".to_string(), ty: Type::I64 },
            Param { name: "h".to_string(), ty: Type::I64 },
            Param { name: "radius".to_string(), ty: Type::I64 },
            Param { name: "border".to_string(), ty: Type::I64 },
            Param { name: "shadow".to_string(), ty: Type::I64 },
            Param { name: "fill".to_string(), ty: Type::I64 },
            Param { name: "border_col".to_string(), ty: Type::I64 },
            Param { name: "clip_id".to_string(), ty: Type::I64 },
        ],
        effects: mem_eff,
        ret_type: Type::Unit,
        requires: Vec::new(),
        ensures: Vec::new(),
        body: if_unit(
            bin(
                BinOp::And,
                bin(BinOp::Gt, var("w"), Expr::IntLit(0)),
                bin(BinOp::Gt, var("h"), Expr::IntLit(0)),
            ),
            vec![
                Stmt::Let {
                    name: "idx".to_string(),
                    mutable: false,
                    ty: None,
                    value: mem_load(12),
                },
                Stmt::Expr(if_unit(
                    bin(BinOp::Lt, var("idx"), Expr::IntLit(80_000)),
                    vec![
                        Stmt::Let {
                            name: "r".to_string(),
                            mutable: false,
                            ty: None,
                            value: call_named(
                                HELPER_CLAMP,
                                vec![var("radius"), Expr::IntLit(0), Expr::IntLit(120)],
                            ),
                        },
                        Stmt::Let {
                            name: "bw".to_string(),
                            mutable: false,
                            ty: None,
                            value: call_named(
                                HELPER_CLAMP,
                                vec![var("border"), Expr::IntLit(0), Expr::IntLit(15)],
                            ),
                        },
                        Stmt::Let {
                            name: "sh".to_string(),
                            mutable: false,
                            ty: None,
                            value: call_named(
                                HELPER_CLAMP,
                                vec![var("shadow"), Expr::IntLit(0), Expr::IntLit(60)],
                            ),
                        },
                        Stmt::Let {
                            name: "pmeta".to_string(),
                            mutable: false,
                            ty: None,
                            value: bin(
                                BinOp::BitOr,
                                bin(
                                    BinOp::BitOr,
                                    bin(BinOp::Shl, var("r"), Expr::IntLit(4)),
                                    bin(BinOp::Shl, var("bw"), Expr::IntLit(12)),
                                ),
                                bin(BinOp::Shl, var("sh"), Expr::IntLit(16)),
                            ),
                        },
                        Stmt::Let {
                            name: "b".to_string(),
                            mutable: false,
                            ty: None,
                            value: bin(
                                BinOp::Add,
                                Expr::IntLit(65_536),
                                bin(BinOp::Mul, var("idx"), Expr::IntLit(8)),
                            ),
                        },
                        mem_store_stmt(var("b"), var("x")),
                        mem_store_stmt(bin(BinOp::Add, var("b"), Expr::IntLit(1)), var("y")),
                        mem_store_stmt(bin(BinOp::Add, var("b"), Expr::IntLit(2)), var("w")),
                        mem_store_stmt(bin(BinOp::Add, var("b"), Expr::IntLit(3)), var("h")),
                        mem_store_stmt(bin(BinOp::Add, var("b"), Expr::IntLit(4)), var("pmeta")),
                        mem_store_stmt(bin(BinOp::Add, var("b"), Expr::IntLit(5)), var("fill")),
                        mem_store_stmt(
                            bin(BinOp::Add, var("b"), Expr::IntLit(6)),
                            var("border_col"),
                        ),
                        mem_store_stmt(
                            bin(BinOp::Add, var("b"), Expr::IntLit(7)),
                            bin(BinOp::BitAnd, var("clip_id"), Expr::IntLit(15)),
                        ),
                        mem_store_stmt(
                            Expr::IntLit(12),
                            bin(BinOp::Add, var("idx"), Expr::IntLit(1)),
                        ),
                    ],
                )),
            ],
        ),
    };

    vec![clamp_fn, set_clip_fn, push_rect_fn]
}

fn var(name: &str) -> Expr {
    Expr::Var(name.to_string())
}

fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn mem_load(addr: i64) -> Expr {
    Expr::MemLoad {
        index: Box::new(Expr::IntLit(addr)),
    }
}

fn mem_store_stmt(index: Expr, value: Expr) -> Stmt {
    Stmt::Expr(Expr::MemStore {
        index: Box::new(index),
        value: Box::new(value),
    })
}

fn call_named(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        target: TargetRef::Named(name.to_string()),
        args,
    }
}

fn if_unit(cond: Expr, then_stmts: Vec<Stmt>) -> Expr {
    Expr::If {
        cond: Box::new(cond),
        then_branch: Box::new(Expr::Block {
            stmts: then_stmts,
            tail: Box::new(Expr::UnitLit),
        }),
        else_branch: Box::new(Expr::UnitLit),
    }
}

fn max_expr(a: Expr, b: Expr) -> Expr {
    Expr::If {
        cond: Box::new(bin(BinOp::Ge, a.clone(), b.clone())),
        then_branch: Box::new(a),
        else_branch: Box::new(b),
    }
}

fn min_expr(a: Expr, b: Expr) -> Expr {
    Expr::If {
        cond: Box::new(bin(BinOp::Le, a.clone(), b.clone())),
        then_branch: Box::new(a),
        else_branch: Box::new(b),
    }
}

fn sum_exprs(terms: &[Expr]) -> Expr {
    let mut iter = terms.iter();
    let Some(first) = iter.next() else {
        return Expr::IntLit(0);
    };
    iter.fold(first.clone(), |acc, e| bin(BinOp::Add, acc, e.clone()))
}

fn align_pos_expr(start: Expr, avail: Expr, size: Expr, align: Align) -> Expr {
    match align {
        Align::Start | Align::Stretch => start,
        Align::Center => bin(
            BinOp::Add,
            start,
            bin(BinOp::Div, bin(BinOp::Sub, avail, size), Expr::IntLit(2)),
        ),
        Align::End => bin(BinOp::Add, start, bin(BinOp::Sub, avail, size)),
    }
}

fn point_in_rect(mx: &str, my: &str, rx: &str, ry: &str, rw: &str, rh: &str) -> Expr {
    bin(
        BinOp::And,
        bin(
            BinOp::And,
            bin(BinOp::Ge, var(mx), var(rx)),
            bin(BinOp::Lt, var(mx), bin(BinOp::Add, var(rx), var(rw))),
        ),
        bin(
            BinOp::And,
            bin(BinOp::Ge, var(my), var(ry)),
            bin(BinOp::Lt, var(my), bin(BinOp::Add, var(ry), var(rh))),
        ),
    )
}
