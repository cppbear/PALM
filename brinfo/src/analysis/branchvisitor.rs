use super::condition::{
    Arm, BinKind, BinaryCond, BoolCond, Condition, ForCond, MatchCond, MatchKind, Patt, PattKind,
};
use super::sourceinfo::SourceInfo;
use rustc_ast::BinOpKind;
use rustc_hir::intravisit::{self, Visitor};
use rustc_middle::ty::{self, TyCtxt, TyKind};
use rustc_span::source_map::Spanned;
use serde_json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

pub struct BranchVisitor<'tcx> {
    tcx: TyCtxt<'tcx>,
    // id: String,
    // name: String,
    encoded_name: String,
    fn_source: SourceInfo,
    typeck_res: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    source_cond_map: BTreeMap<SourceInfo, Vec<Condition>>,
    panic: bool,
}

impl<'tcx> BranchVisitor<'tcx> {
    pub fn new(
        tcx: TyCtxt<'tcx>,
        // id: String,
        // name: String,
        encoded_name: String,
        fn_source: SourceInfo,
        typeck_res: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    ) -> Self {
        Self {
            tcx,
            // id,
            // name,
            encoded_name,
            fn_source,
            typeck_res,
            source_cond_map: BTreeMap::new(),
            panic: false,
        }
    }

    pub fn output_map(&self, crate_dir: &Path) {
        let dir_path = crate_dir.join(format!("brinfo/tmp/{}", self.encoded_name));
        let file_path = dir_path.join("cond_map.json");
        fs::create_dir_all(dir_path).unwrap();
        let map: BTreeMap<SourceInfo, BTreeSet<Condition>> = self
            .source_cond_map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().into_iter().collect()))
            .collect();
        let json = serde_json::to_string_pretty(&map).unwrap();
        let mut file = File::create(file_path).unwrap();
        file.write_all(json.as_bytes()).unwrap();
    }

    pub fn move_map(self) -> BTreeMap<SourceInfo, BTreeSet<Condition>> {
        self.source_cond_map
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect()
    }

    pub fn is_panic(&self) -> bool {
        self.panic
    }

    fn is_comparable_literal(expr: &rustc_hir::Expr) -> bool {
        match expr.kind {
            rustc_hir::ExprKind::Lit(lit) => {
                matches!(
                    lit.node,
                    rustc_ast::LitKind::Byte(_)
                        | rustc_ast::LitKind::Char(_)
                        | rustc_ast::LitKind::Int(_, _)
                )
            }
            rustc_hir::ExprKind::Unary(op, subexpr) => {
                matches!(op, rustc_hir::UnOp::Neg)
                    && match subexpr.kind {
                        rustc_hir::ExprKind::Lit(lit) => {
                            matches!(
                                lit.node,
                                rustc_ast::LitKind::Byte(_)
                                    | rustc_ast::LitKind::Char(_)
                                    | rustc_ast::LitKind::Int(_, _)
                            )
                        }
                        _ => false,
                    }
            }
            _ => false,
        }
    }

    fn may_panic(&self, name: &str, did: rustc_span::def_id::DefId) -> bool {
        if name == "unwrap" || name == "expect" || name.contains("unchecked") {
            return true;
        }

        if let Some(impl_id) = self.tcx.impl_of_method(did) {
            let impl_str = self.tcx.def_path_str(impl_id);
            // println!("Method call: {:?}, impl: {:?}", name, impl_str);
            let mut flag = match impl_str.as_str() {
                "std::vec::Vec<T, A>" => {
                    name == "remove"
                        || name == "swap_remove"
                        || name == "insert"
                        || name == "drain"
                        || name == "split_off"
                }
                "std::collections::VecDeque<T, A>" => {
                    name == "swap"
                        || name == "range"
                        || name == "range_mut"
                        || name == "drain"
                        || name == "insert"
                        || name == "split_off"
                        || name == "rotate_left"
                        || name == "rotate_right"
                }
                "std::collections::LinkedList<T, A>" => name == "split_off",
                "std::collections::BTreeMap<K, V, A>" => name == "range" || name == "range_mut",
                "std::collections::BTreeSet<T, A>" => name == "range",
                "core::slice::<impl [T]>" => {
                    name == "swap"
                        || name == "windows"
                        || name == "chunks"
                        || name == "chunks_mut"
                        || name == "chunks_exact"
                        || name == "chunks_exact_mut"
                        || name == "rchunks"
                        || name == "rchunks_mut"
                        || name == "rchunks_exact"
                        || name == "rchunks_exact_mut"
                        || name == "split_at"
                        || name == "split_at_mut"
                        || name == "select_nth_unstable"
                        || name == "select_nth_unstable_by"
                        || name == "select_nth_unstable_by_key"
                        || name == "rotate_left"
                        || name == "rotate_right"
                        || name == "clone_from_slice"
                        || name == "copy_from_slice"
                        || name == "copy_within"
                        || name == "swap_with_slice"
                }
                _ => false,
            };
            // handle some special cases
            if impl_str.contains("core::num::") {
                flag = flag || name == "wrapping_div" || name == "wrapping_rem";
            }
            return flag;
        }
        false
    }

    fn handle_binary(
        &mut self,
        op: &Spanned<BinOpKind>,
        expr_source: SourceInfo,
        lexpr: &'tcx rustc_hir::Expr<'tcx>,
        rexpr: &'tcx rustc_hir::Expr<'tcx>,
    ) -> Result<BTreeMap<SourceInfo, Vec<Condition>>, ()> {
        let lhs = SourceInfo::from_span(lexpr.span, self.tcx.sess.source_map()).get_string();
        let rhs = SourceInfo::from_span(rexpr.span, self.tcx.sess.source_map()).get_string();
        let cmp_with_int = Self::is_comparable_literal(lexpr) || Self::is_comparable_literal(rexpr);
        let mut map = BTreeMap::new();
        match op.node {
            rustc_hir::BinOpKind::And | rustc_hir::BinOpKind::Or => {
                if let Ok(res) = self.handle_expr(lexpr) {
                    map.extend(res);
                } else {
                    return Err(());
                }
                if let Ok(res) = self.handle_expr(rexpr) {
                    map.extend(res);
                } else {
                    return Err(());
                }
            }
            _ => {
                let kind = match op.node {
                    BinOpKind::Eq => BinKind::Eq,
                    BinOpKind::Ne => BinKind::Ne,
                    BinOpKind::Ge => BinKind::Ge,
                    BinOpKind::Gt => BinKind::Gt,
                    BinOpKind::Le => BinKind::Le,
                    BinOpKind::Lt => BinKind::Lt,
                    _ => BinKind::Other,
                };
                let bin_cond = BinaryCond {
                    kind,
                    expr: expr_source.get_string(),
                    lhs,
                    rhs,
                    cmp_with_int,
                };
                let cond = Condition::Bool(BoolCond::Binary(bin_cond));
                map.entry(expr_source.clone())
                    .and_modify(|_v| {
                        warn!("Duplicated condition for Binary: {:?}", expr_source);
                    })
                    .or_default()
                    .push(cond);
            }
        }
        Ok(map)
    }

    fn handle_expr(
        &mut self,
        expr: &'tcx rustc_hir::Expr<'tcx>,
    ) -> Result<BTreeMap<SourceInfo, Vec<Condition>>, ()> {
        let expr_source = SourceInfo::from_span(expr.span, self.tcx.sess.source_map());
        let expr_str = expr_source.get_string();
        let mut map = BTreeMap::new();
        match expr.kind {
            rustc_hir::ExprKind::DropTemps(temp_expr) => {
                if let Ok(res) = self.handle_expr(temp_expr) {
                    map.extend(res);
                } else {
                    return Err(());
                }
            }
            rustc_hir::ExprKind::Binary(op, lexpr, rexpr) => {
                if let Ok(res) = self.handle_binary(&op, expr_source, lexpr, rexpr) {
                    map.extend(res);
                } else {
                    return Err(());
                }
            }
            rustc_hir::ExprKind::Unary(op, subexpr) => match op {
                rustc_hir::UnOp::Not => {
                    if let Ok(res) = self.handle_expr(subexpr) {
                        map.extend(res);
                    } else {
                        return Err(());
                    }
                }
                _ => {
                    let cond = Condition::Bool(BoolCond::Other(expr_str));
                    map.entry(expr_source.clone())
                        .and_modify(|_v| {
                            warn!("Duplicated condition for Unary: {:?}", expr_source);
                        })
                        .or_default()
                        .push(cond);
                }
            },
            rustc_hir::ExprKind::Let(let_expr) => {
                if let Err(()) = self.handle_iflet(expr_source, let_expr) {
                    return Err(());
                }
            }
            rustc_hir::ExprKind::Lit(_) => {
                // FIXME: handle literals which means determinated conditions
                let cond = Condition::Bool(BoolCond::Other(expr_str));
                map.entry(expr_source.clone())
                    .and_modify(|_v| {
                        warn!("Duplicated condition for Lit: {:?}", expr_source);
                    })
                    .or_default()
                    .push(cond);
            }
            rustc_hir::ExprKind::MethodCall(_, _, _, _) => {
                let cond = Condition::Bool(BoolCond::Other(expr_str));
                map.entry(expr_source.clone())
                    .and_modify(|_v| {
                        warn!("Duplicated condition for MethodCall: {:?}", expr_source);
                    })
                    .or_default()
                    .push(cond);
            }
            rustc_hir::ExprKind::Call(_, _) => {
                let cond = Condition::Bool(BoolCond::Other(expr_str));
                map.entry(expr_source.clone())
                    .and_modify(|_v| {
                        warn!("Duplicated condition for Call: {:?}", expr_source);
                    })
                    .or_default()
                    .push(cond);
            }
            rustc_hir::ExprKind::Path(_) => {
                let cond = Condition::Bool(BoolCond::Other(expr_str));
                map.entry(expr_source.clone())
                    .and_modify(|_v| {
                        warn!("Duplicated condition for Path: {:?}", expr_source);
                    })
                    .or_default()
                    .push(cond);
            }
            rustc_hir::ExprKind::Block(_, _) => {
                let cond = Condition::Bool(BoolCond::Other(expr_str));
                map.entry(expr_source.clone())
                    .and_modify(|_v| {
                        warn!("Duplicated condition for Block: {:?}", expr_source);
                    })
                    .or_default()
                    .push(cond);
            }
            rustc_hir::ExprKind::Match(expr, arms, match_kind) => match match_kind {
                rustc_hir::MatchSource::Normal => {
                    if let Err(()) = self.handle_match(expr_source.clone(), expr, arms) {
                        return Err(());
                    }
                }
                rustc_hir::MatchSource::TryDesugar(_) => self.handle_try(expr),
                _ => {}
            },
            rustc_hir::ExprKind::Field(_, _) => {
                let cond = Condition::Bool(BoolCond::Other(expr_str));
                map.entry(expr_source.clone())
                    .and_modify(|_v| {
                        warn!("Duplicated condition for Field: {:?}", expr_source);
                    })
                    .or_default()
                    .push(cond);
            }
            rustc_hir::ExprKind::If(cond_expr, _, _) => {
                if let Ok(res) = self.handle_expr(cond_expr) {
                    map.extend(res);
                } else {
                    return Err(());
                }
            }
            _ => {
                error!("Unsupported expression kind: {:?}", expr.kind);
                return Err(());
            }
        }
        Ok(map)
    }

    fn handle_forloop(&mut self, block: &rustc_hir::Block) -> Result<(), ()> {
        let stmt = block.stmts[0];
        if let rustc_hir::StmtKind::Expr(expr) = stmt.kind {
            if let rustc_hir::ExprKind::Match(_, arms, match_kind) = expr.kind {
                assert_eq!(match_kind, rustc_hir::MatchSource::ForLoopDesugar);
                let var_source =
                    SourceInfo::from_span(arms[1].pat.span, self.tcx.sess.source_map());
                let range_source = SourceInfo::from_span(expr.span, self.tcx.sess.source_map());
                let cond = Condition::For(ForCond {
                    iter_var: var_source.get_string(),
                    iter_range: range_source.get_string(),
                });
                self.source_cond_map
                    .entry(range_source.clone())
                    .and_modify(|_v| {
                        warn!("Duplicated condition for ForLoop: {:?}", range_source);
                    })
                    .or_default()
                    .push(cond);
            } else {
                error!(
                    "The ExprKind of the first stmt in ForLoop is {:?}.",
                    expr.kind
                );
                return Err(());
            }
        } else {
            error!(
                "The StmtKind of the first stmt in ForLoop is {:?}.",
                stmt.kind
            );
            return Err(());
        }
        Ok(())
    }

    fn resolve_pat_kind(&self, pat: &'tcx rustc_hir::Pat<'tcx>) -> rustc_hir::PatKind<'tcx> {
        match pat.kind {
            rustc_hir::PatKind::Ref(subpat, _) | rustc_hir::PatKind::Deref(subpat) => {
                return self.resolve_pat_kind(subpat);
            }
            _ => pat.kind,
        }
    }

    fn handle_enum_pat(
        &self,
        pat: &'tcx rustc_hir::Pat<'tcx>,
        adt_def: &'tcx rustc_middle::ty::AdtDef<'tcx>,
    ) -> Result<(SourceInfo, Patt), ()> {
        let pat_source = SourceInfo::from_span(pat.span, self.tcx.sess.source_map());
        // println!("Pattern: {:?}, {}", pat_source, pat_source.get_string());
        // let pat_ty = self.typeck_res.pat_ty(pat);
        // println!("Type of {} is {:?}", pat_source.get_string(), pat_ty);
        let pat_kind = self.resolve_pat_kind(pat);
        match pat_kind {
            rustc_hir::PatKind::Expr(&rustc_hir::PatExpr {
                kind: rustc_hir::PatExprKind::Path(qpath),
                ..
            })
            | rustc_hir::PatKind::TupleStruct(qpath, _, _)
            | rustc_hir::PatKind::Struct(qpath, _, _) => match qpath {
                rustc_hir::QPath::Resolved(_, path) => {
                    // println!("path.res is: {:?}", path.res);
                    match path.res {
                        rustc_hir::def::Res::Def(
                            rustc_hir::def::DefKind::Ctor(rustc_hir::def::CtorOf::Variant, _),
                            ctor_def_id,
                        ) => {
                            let variant_index = adt_def.variant_index_with_ctor_id(ctor_def_id);
                            // println!("variant_index: {}", variant_index.index());
                            let discr = adt_def
                                .discriminant_for_variant(self.tcx, variant_index)
                                .val;
                            let patt = Patt {
                                pat_str: pat_source.get_string(),
                                kind: PattKind::Enum(discr),
                            };
                            Ok((pat_source, patt))
                        }
                        rustc_hir::def::Res::Def(
                            rustc_hir::def::DefKind::Variant,
                            variant_def_id,
                        ) => {
                            let variant_index = adt_def.variant_index_with_id(variant_def_id);
                            // println!("variant_index: {}", variant_index.index());
                            let discr = adt_def
                                .discriminant_for_variant(self.tcx, variant_index)
                                .val;
                            let patt = Patt {
                                pat_str: pat_source.get_string(),
                                kind: PattKind::Enum(discr),
                            };
                            Ok((pat_source, patt))
                        }
                        _ => {
                            error!("path.res is: {:?}", path.res);
                            return Err(());
                        }
                    }
                }
                rustc_hir::QPath::TypeRelative(
                    rustc_hir::Ty {
                        kind:
                            rustc_hir::TyKind::Path(rustc_hir::QPath::Resolved(
                                _,
                                rustc_hir::Path {
                                    res: rustc_hir::def::Res::SelfTyAlias { .. },
                                    ..
                                },
                            )),
                        ..
                    },
                    path_seg,
                ) => {
                    let ident = path_seg.ident;
                    let variant_index = adt_def
                        .variants()
                        .iter_enumerated()
                        .find(|(_, v)| v.ident(self.tcx) == ident)
                        .unwrap()
                        .0;
                    let discr = adt_def
                        .discriminant_for_variant(self.tcx, variant_index)
                        .val;
                    let patt = Patt {
                        pat_str: pat_source.get_string(),
                        kind: PattKind::Enum(discr),
                    };
                    Ok((pat_source, patt))
                }
                _ => {
                    error!("qpath is: {:?}", qpath);
                    return Err(());
                }
            },
            rustc_hir::PatKind::Binding(_, _, _, _) => {
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::Other(None),
                };
                Ok((pat_source, patt))
            }
            _ => {
                error!("pat_kind is: {:?}", pat.kind);
                return Err(());
            }
        }
    }

    fn lit_to_constant(
        &self,
        lit_kind: &rustc_ast::LitKind,
        width: rustc_abi::Size,
        neg: bool,
    ) -> Result<u128, ()> {
        match lit_kind {
            rustc_ast::LitKind::Byte(b) => Ok(*b as u128),
            rustc_ast::LitKind::Char(ch) => Ok(*ch as u128),
            rustc_ast::LitKind::Int(n, _) => Ok(width.truncate(if neg {
                (n.get() as i128).overflowing_neg().0 as u128
            } else {
                n.get()
            })),
            _ => {
                error!("lit_kind is: {:?}", lit_kind);
                return Err(());
            }
        }
    }

    fn handle_struct_pat(
        &self,
        pat: &'tcx rustc_hir::Pat<'tcx>,
        adt_def: &'tcx rustc_middle::ty::AdtDef<'tcx>,
    ) -> Result<(SourceInfo, Patt), ()> {
        let pat_source = SourceInfo::from_span(pat.span, self.tcx.sess.source_map());
        // println!("Pattern: {:?}, {}", pat_source, pat_source.get_string());
        let pat_kind = self.resolve_pat_kind(pat);
        match pat_kind {
            rustc_hir::PatKind::Struct(_, fields, _) => {
                let mut lit_map = BTreeMap::new();
                for field in fields {
                    let field_name = field.ident.name;
                    let index = adt_def
                        .all_fields()
                        .position(|f| f.name == field_name)
                        .unwrap();
                    // println!("Field {}: {:?}", index, field_name);
                    let field_pat_kind = self.resolve_pat_kind(&field.pat);
                    let mut mir_const: Option<u128> = None;
                    match field_pat_kind {
                        rustc_hir::PatKind::Expr(pat_expr) => match pat_expr.kind {
                            rustc_hir::PatExprKind::Lit {
                                lit: expr_lit,
                                negated,
                            } => match expr_lit.node {
                                rustc_ast::LitKind::Byte(_)
                                | rustc_ast::LitKind::Char(_)
                                | rustc_ast::LitKind::Int(_, _) => {
                                    // let lit_str =
                                    //     SourceInfo::from_span(expr_lit.span, &self.span_re)
                                    //         .get_string();
                                    let lit_ty = self.typeck_res.node_type(pat_expr.hir_id);
                                    let width = self
                                        .tcx
                                        .layout_of(
                                            ty::TypingEnv::fully_monomorphized()
                                                .as_query_input(lit_ty),
                                        )
                                        .unwrap()
                                        .size;
                                    // println!("Field type layout: {:?}", layout);
                                    if let Ok(n) =
                                        self.lit_to_constant(&expr_lit.node, width, negated)
                                    {
                                        mir_const = Some(n);
                                    } else {
                                        return Err(());
                                    }
                                    // println!("Literal: {}, mir_const: {:?}", lit_str, mir_const);
                                }
                                _ => {}
                            },
                            _ => {}
                        },
                        _ => {}
                    }
                    lit_map.insert(
                        index,
                        (
                            mir_const,
                            SourceInfo::from_span(field.pat.span, self.tcx.sess.source_map()),
                        ),
                    );
                }
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::StructLike(lit_map),
                };
                Ok((pat_source, patt))
            }
            rustc_hir::PatKind::TupleStruct(_, fields, pos) => {
                let offset = adt_def.all_fields().count() - fields.len();
                let mut lit_map = BTreeMap::new();
                let mut index = 0;
                for field in fields {
                    if pos.as_opt_usize().map_or(false, |dot_pos| index == dot_pos) {
                        index += offset;
                    }
                    let field_pat_kind = self.resolve_pat_kind(field);
                    let mut mir_const: Option<u128> = None;
                    match field_pat_kind {
                        rustc_hir::PatKind::Expr(pat_expr) => match pat_expr.kind {
                            rustc_hir::PatExprKind::Lit {
                                lit: expr_lit,
                                negated,
                            } => match expr_lit.node {
                                rustc_ast::LitKind::Byte(_)
                                | rustc_ast::LitKind::Char(_)
                                | rustc_ast::LitKind::Int(_, _) => {
                                    // let lit_str =
                                    //     SourceInfo::from_span(expr_lit.span, &self.span_re)
                                    //         .get_string();
                                    let lit_ty = self.typeck_res.node_type(pat_expr.hir_id);
                                    let width = self
                                        .tcx
                                        .layout_of(
                                            ty::TypingEnv::fully_monomorphized()
                                                .as_query_input(lit_ty),
                                        )
                                        .unwrap()
                                        .size;
                                    // println!("Field type layout: {:?}", layout);
                                    if let Ok(n) =
                                        self.lit_to_constant(&expr_lit.node, width, negated)
                                    {
                                        mir_const = Some(n);
                                    } else {
                                        return Err(());
                                    }
                                    // println!("Literal: {}, mir_const: {:?}", lit_str, mir_const);
                                }
                                _ => {}
                            },
                            _ => {}
                        },
                        _ => {}
                    }
                    lit_map.insert(
                        index,
                        (
                            mir_const,
                            SourceInfo::from_span(field.span, self.tcx.sess.source_map()),
                        ),
                    );
                    index += 1;
                }
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::StructLike(lit_map),
                };
                Ok((pat_source, patt))
            }
            _ => {
                error!("pat_kind is: {:?}", pat_kind);
                return Err(());
            }
        }
    }

    fn handle_adt_pat(
        &self,
        pat: &'tcx rustc_hir::Pat<'tcx>,
        adt_def: &'tcx rustc_middle::ty::AdtDef<'tcx>,
    ) -> Result<BTreeMap<SourceInfo, Patt>, ()> {
        let mut map = BTreeMap::new();
        let pat_source = SourceInfo::from_span(pat.span, self.tcx.sess.source_map());
        // println!("Pattern: {:?}, {}", pat_source, pat_source.get_string());
        // let pat_ty = self.typeck_res.pat_ty(pat);
        // println!("Type of {} is {:?}", pat_source.get_string(), pat_ty);
        let pat_kind = self.resolve_pat_kind(pat);
        // println!("pat_kind is: {:?}", pat_kind);
        match pat_kind {
            rustc_hir::PatKind::Or(subpats) => {
                for subpat in subpats {
                    if let Ok(res) = self.handle_adt_pat(subpat, adt_def) {
                        map.extend(res);
                    } else {
                        return Err(());
                    }
                }
            }
            rustc_hir::PatKind::Wild => {
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::Wild,
                };
                map.insert(pat_source, patt);
            }
            _ => {
                if adt_def.is_enum() {
                    if let Ok((pat_source, patt)) = self.handle_enum_pat(pat, adt_def) {
                        map.insert(pat_source, patt);
                    } else {
                        return Err(());
                    }
                } else if adt_def.is_struct() {
                    if let Ok((pat_source, patt)) = self.handle_struct_pat(pat, adt_def) {
                        map.insert(pat_source, patt);
                    } else {
                        return Err(());
                    }
                }
            }
        }
        Ok(map)
    }

    fn handle_tuple_pat(
        &self,
        pat: &'tcx rustc_hir::Pat<'tcx>,
        tuple_def: &'tcx [rustc_middle::ty::Ty<'tcx>],
    ) -> Result<BTreeMap<SourceInfo, Patt>, ()> {
        let mut map = BTreeMap::new();
        let pat_source = SourceInfo::from_span(pat.span, self.tcx.sess.source_map());
        // println!("Pattern: {:?}, {}", pat_source, pat_source.get_string());
        let pat_kind = self.resolve_pat_kind(pat);
        match pat_kind {
            rustc_hir::PatKind::Or(subpats) => {
                for subpat in subpats {
                    if let Ok(res) = self.handle_tuple_pat(subpat, tuple_def) {
                        map.extend(res);
                    } else {
                        return Err(());
                    }
                }
            }
            rustc_hir::PatKind::Tuple(fields, pos) => {
                let offset = tuple_def.len() - fields.len();
                let mut lit_map = BTreeMap::new();
                let mut index = 0;
                for field in fields {
                    if pos.as_opt_usize().map_or(false, |dot_pos| index == dot_pos) {
                        index += offset;
                    }
                    let field_pat_kind = self.resolve_pat_kind(field);
                    let mut mir_const: Option<u128> = None;
                    match field_pat_kind {
                        rustc_hir::PatKind::Expr(pat_expr) => match pat_expr.kind {
                            rustc_hir::PatExprKind::Lit {
                                lit: expr_lit,
                                negated,
                            } => match expr_lit.node {
                                rustc_ast::LitKind::Byte(_)
                                | rustc_ast::LitKind::Char(_)
                                | rustc_ast::LitKind::Int(_, _) => {
                                    // let lit_str =
                                    //     SourceInfo::from_span(expr_lit.span, &self.span_re)
                                    //         .get_string();
                                    let lit_ty = self.typeck_res.node_type(pat_expr.hir_id);
                                    let width = self
                                        .tcx
                                        .layout_of(
                                            ty::TypingEnv::fully_monomorphized()
                                                .as_query_input(lit_ty),
                                        )
                                        .unwrap()
                                        .size;
                                    // println!("Field type layout: {:?}", layout);
                                    if let Ok(n) =
                                        self.lit_to_constant(&expr_lit.node, width, negated)
                                    {
                                        mir_const = Some(n);
                                    } else {
                                        return Err(());
                                    }
                                    // println!("Literal: {}, mir_const: {:?}", lit_str, mir_const);
                                }
                                _ => {}
                            },
                            _ => {}
                        },
                        _ => {}
                    }
                    lit_map.insert(
                        index,
                        (
                            mir_const,
                            SourceInfo::from_span(field.span, self.tcx.sess.source_map()),
                        ),
                    );
                    index += 1;
                }
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::StructLike(lit_map),
                };
                map.insert(pat_source, patt);
            }
            rustc_hir::PatKind::Wild => {
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::Wild,
                };
                map.insert(pat_source, patt);
            }
            _ => {
                error!("pat.kind is: {:?}", pat.kind);
                return Err(());
            }
        }
        Ok(map)
    }

    fn handle_other_pat(
        &self,
        pat: &'tcx rustc_hir::Pat<'tcx>,
    ) -> Result<BTreeMap<SourceInfo, Patt>, ()> {
        let mut map = BTreeMap::new();
        let pat_source = SourceInfo::from_span(pat.span, self.tcx.sess.source_map());
        let pat_kind = self.resolve_pat_kind(pat);
        match pat_kind {
            rustc_hir::PatKind::Or(subpats) => {
                for subpat in subpats {
                    if let Ok(res) = self.handle_other_pat(subpat) {
                        map.extend(res);
                    } else {
                        return Err(());
                    }
                }
            }
            rustc_hir::PatKind::Expr(pat_expr) => {
                let mut mir_const: Option<u128> = None;
                match pat_expr.kind {
                    rustc_hir::PatExprKind::Lit {
                        lit: expr_lit,
                        negated,
                    } => match expr_lit.node {
                        rustc_ast::LitKind::Byte(_)
                        | rustc_ast::LitKind::Char(_)
                        | rustc_ast::LitKind::Int(_, _) => {
                            // let lit_str =
                            //     SourceInfo::from_span(expr_lit.span, &self.span_re).get_string();
                            let lit_ty = self.typeck_res.node_type(pat_expr.hir_id);
                            let width = self
                                .tcx
                                .layout_of(
                                    ty::TypingEnv::fully_monomorphized().as_query_input(lit_ty),
                                )
                                .unwrap()
                                .size;
                            // println!("Field type layout: {:?}", layout);
                            if let Ok(n) = self.lit_to_constant(&expr_lit.node, width, negated) {
                                mir_const = Some(n);
                            } else {
                                return Err(());
                            }
                            // println!("Literal: {}, mir_const: {:?}", lit_str, mir_const);
                        }
                        _ => {}
                    },
                    _ => {}
                }
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::Other(mir_const),
                };
                map.insert(pat_source, patt);
            }
            rustc_hir::PatKind::Wild => {
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::Wild,
                };
                map.insert(pat_source, patt);
            }
            _ => {
                let patt = Patt {
                    pat_str: pat_source.get_string(),
                    kind: PattKind::Other(None),
                };
                map.insert(pat_source, patt);
            }
        }
        Ok(map)
    }

    fn resolve_match_type(&self, tykind: &'tcx TyKind<'tcx>) -> &'tcx TyKind<'tcx> {
        match tykind {
            TyKind::Ref(_, ty, _) => self.resolve_match_type(ty.kind()),
            _ => tykind,
        }
    }

    fn handle_match(
        &mut self,
        cond_source: SourceInfo,
        expr: &'tcx rustc_hir::Expr<'tcx>,
        arms: &'tcx [rustc_hir::Arm<'tcx>],
    ) -> Result<(), ()> {
        let match_source = SourceInfo::from_span(expr.span, self.tcx.sess.source_map());
        let match_str = match_source.get_string();
        let expr_ty = self.typeck_res.expr_ty(expr);
        let expr_ty = self.resolve_match_type(expr_ty.kind());
        let mut cond;

        match expr_ty {
            TyKind::Adt(adt_def, _) => {
                let match_kind = if adt_def.is_enum() {
                    MatchKind::Enum(
                        adt_def
                            .variants()
                            .iter()
                            .map(|variant| variant.name.to_string())
                            .collect(),
                    )
                } else if adt_def.is_struct() {
                    let field_names = adt_def
                        .all_fields()
                        .map(|field| field.name.to_string())
                        .collect();
                    MatchKind::StructLike(Some(field_names))
                } else {
                    MatchKind::Other
                };
                cond = MatchCond::new(match_source.clone(), match_str.clone(), match_kind);
                for arm in arms {
                    let mut guard_map = None;
                    if let Some(guard) = arm.guard {
                        if let Ok(cond_map) = self.handle_expr(guard) {
                            self.source_cond_map.extend(cond_map.clone());
                            let cond_map = cond_map
                                .into_iter()
                                .map(|(source_info, cond)| {
                                    (source_info, cond.into_iter().collect())
                                })
                                .collect();
                            guard_map = Some(cond_map);
                        } else {
                            return Err(());
                        }
                    }
                    let body_source =
                        SourceInfo::from_span(arm.body.span, self.tcx.sess.source_map());
                    let mut source_wrapper = None;
                    if self.fn_source.contains(&body_source) {
                        source_wrapper = Some(body_source);
                    }
                    if let Ok(patt_map) = self.handle_adt_pat(arm.pat, adt_def) {
                        for (source_info, patt) in patt_map {
                            let arm = Arm {
                                pat: patt.clone(),
                                guard: guard_map.clone(),
                                body_source: source_wrapper.clone(),
                            };
                            cond.arms.insert(source_info, arm);
                        }
                    } else {
                        return Err(());
                    }
                }
            }
            TyKind::Tuple(tuple_def) => {
                cond = MatchCond::new(
                    match_source.clone(),
                    match_str.clone(),
                    MatchKind::StructLike(None),
                );
                for arm in arms {
                    let mut guard_map = None;
                    if let Some(guard) = &arm.guard {
                        if let Ok(cond_map) = self.handle_expr(guard) {
                            self.source_cond_map.extend(cond_map.clone());
                            let cond_map = cond_map
                                .into_iter()
                                .map(|(source_info, cond)| {
                                    (source_info, cond.into_iter().collect())
                                })
                                .collect();
                            guard_map = Some(cond_map);
                        } else {
                            return Err(());
                        }
                    }
                    let body_source =
                        SourceInfo::from_span(arm.body.span, self.tcx.sess.source_map());
                    let mut source_wrapper = None;
                    if self.fn_source.contains(&body_source) {
                        source_wrapper = Some(body_source);
                    }
                    if let Ok(patt_map) = self.handle_tuple_pat(arm.pat, tuple_def) {
                        for (source_info, patt) in patt_map {
                            let arm = Arm {
                                pat: patt.clone(),
                                guard: guard_map.clone(),
                                body_source: source_wrapper.clone(),
                            };
                            cond.arms.insert(source_info, arm);
                        }
                    } else {
                        return Err(());
                    }
                }
            }
            _ => {
                cond = MatchCond::new(match_source.clone(), match_str.clone(), MatchKind::Other);
                for arm in arms {
                    let mut guard_map = None;
                    if let Some(guard) = &arm.guard {
                        if let Ok(cond_map) = self.handle_expr(guard) {
                            self.source_cond_map.extend(cond_map.clone());
                            let cond_map = cond_map
                                .into_iter()
                                .map(|(source_info, cond)| {
                                    (source_info, cond.into_iter().collect())
                                })
                                .collect();
                            guard_map = Some(cond_map);
                        } else {
                            return Err(());
                        }
                    }
                    let body_source =
                        SourceInfo::from_span(arm.body.span, self.tcx.sess.source_map());
                    let mut source_wrapper = None;
                    if self.fn_source.contains(&body_source) {
                        source_wrapper = Some(body_source);
                    }
                    if let Ok(patt_map) = self.handle_other_pat(arm.pat) {
                        for (source_info, patt) in patt_map {
                            let arm = Arm {
                                pat: patt.clone(),
                                guard: guard_map.clone(),
                                body_source: source_wrapper.clone(),
                            };
                            cond.arms.insert(source_info, arm);
                        }
                    } else {
                        return Err(());
                    }
                }
            }
        }
        let cond = Condition::Match(cond);
        if !self.fn_source.contains(&cond_source) {
            self.source_cond_map
                .entry(cond_source.clone())
                .and_modify(|_v| {
                    warn!("Duplicated condition for Match: {:?}", cond_source);
                })
                .or_default()
                .push(cond.clone());
        }
        let mut source = match_source.clone();
        source.set_startline(cond_source.get_startline());
        source.set_startcolumn(cond_source.get_startcolumn());
        self.source_cond_map
            .entry(source.clone())
            .and_modify(|_v| {
                warn!("Duplicated condition for Match: {:?}", source);
            })
            .or_default()
            .push(cond);
        Ok(())
    }

    fn handle_try(&mut self, expr: &'tcx rustc_hir::Expr<'tcx>) {
        let try_source = SourceInfo::from_span(expr.span, self.tcx.sess.source_map());
        let cond = Condition::Try(try_source.get_string());
        self.source_cond_map
            .entry(try_source.clone())
            .and_modify(|_v| {
                warn!("Duplicated condition for Try: {:?}", try_source);
            })
            .or_default()
            .push(cond);
    }

    fn handle_iflet(
        &mut self,
        _cond_source: SourceInfo,
        expr: &'tcx rustc_hir::LetExpr<'tcx>,
    ) -> Result<(), ()> {
        let pat = expr.pat;
        let init = expr.init;
        let match_source = SourceInfo::from_span(init.span, self.tcx.sess.source_map());
        let match_str = match_source.get_string();
        let init_ty = self.typeck_res.expr_ty(init);
        let init_ty = self.resolve_match_type(init_ty.kind());

        let mut cond;
        match init_ty {
            TyKind::Adt(adt_def, _) => {
                let match_kind = if adt_def.is_enum() {
                    MatchKind::Enum(
                        adt_def
                            .variants()
                            .iter()
                            .map(|variant| variant.name.to_string())
                            .collect(),
                    )
                } else if adt_def.is_struct() {
                    let field_names = adt_def
                        .all_fields()
                        .map(|field| field.name.to_string())
                        .collect();
                    MatchKind::StructLike(Some(field_names))
                } else {
                    MatchKind::Other
                };
                cond = MatchCond::new(match_source.clone(), match_str.clone(), match_kind);
                if let Ok(patt_map) = self.handle_adt_pat(pat, adt_def) {
                    for (source_info, patt) in patt_map {
                        let arm = Arm {
                            pat: patt.clone(),
                            guard: None,
                            body_source: None,
                        };
                        cond.arms.insert(source_info, arm);
                    }
                } else {
                    return Err(());
                }
            }
            TyKind::Tuple(tuple_def) => {
                cond = MatchCond::new(
                    match_source.clone(),
                    match_str.clone(),
                    MatchKind::StructLike(None),
                );
                if let Ok(patt_map) = self.handle_tuple_pat(pat, tuple_def) {
                    for (source_info, patt) in patt_map {
                        let arm = Arm {
                            pat: patt.clone(),
                            guard: None,
                            body_source: None,
                        };
                        cond.arms.insert(source_info, arm);
                    }
                } else {
                    return Err(());
                }
            }
            _ => {
                cond = MatchCond::new(match_source.clone(), match_str.clone(), MatchKind::Other);
                if let Ok(patt_map) = self.handle_other_pat(pat) {
                    for (source_info, patt) in patt_map {
                        let arm = Arm {
                            pat: patt.clone(),
                            guard: None,
                            body_source: None,
                        };
                        cond.arms.insert(source_info, arm);
                    }
                } else {
                    return Err(());
                }
            }
        }
        let source = SourceInfo::from_span(pat.span, self.tcx.sess.source_map());
        let cond = Condition::IfLet(cond);
        self.source_cond_map
            .entry(source.clone())
            .and_modify(|_v| {
                warn!("Duplicated condition for IfLet: {:?}", source);
            })
            .or_default()
            .push(cond);
        Ok(())
    }
}

impl<'tcx> Visitor<'tcx> for BranchVisitor<'tcx> {
    fn visit_expr(&mut self, ex: &'tcx rustc_hir::Expr<'tcx>) -> Self::Result {
        match ex.kind {
            rustc_hir::ExprKind::If(cond_expr, _, _) => {
                if let Ok(res) = self.handle_expr(cond_expr) {
                    self.source_cond_map.extend(res);
                } else {
                    self.panic = true;
                }
            }
            rustc_hir::ExprKind::Loop(block, _, loop_kind, _) => {
                if let rustc_hir::LoopSource::ForLoop = loop_kind {
                    if let Err(()) = self.handle_forloop(block) {
                        self.panic = true;
                    }
                }
            }
            rustc_hir::ExprKind::Match(expr, arms, match_kind) => match match_kind {
                rustc_hir::MatchSource::Normal => {
                    let expr_source = SourceInfo::from_span(ex.span, self.tcx.sess.source_map());
                    if let Err(()) = self.handle_match(expr_source, expr, arms) {
                        self.panic = true;
                    }
                }
                rustc_hir::MatchSource::TryDesugar(_) => self.handle_try(expr),
                _ => {}
            },
            // FIXME: handle other boolean expressions
            rustc_hir::ExprKind::Binary(op, lexpr, rexpr) => {
                let expr_source = SourceInfo::from_span(ex.span, self.tcx.sess.source_map());
                if let Ok(res) = self.handle_binary(&op, expr_source, lexpr, rexpr) {
                    self.source_cond_map.extend(res);
                } else {
                    self.panic = true;
                }
            }
            // Method calls like unwrap, expect, etc.
            rustc_hir::ExprKind::MethodCall(pathseg, _, _, _) => {
                if let Some(method_did) = self.typeck_res.type_dependent_def_id(ex.hir_id) {
                    let name = pathseg.ident.as_str();
                    if self.may_panic(name, method_did) {
                        let expr_source =
                            SourceInfo::from_span(ex.span, self.tcx.sess.source_map());
                        let cond = Condition::MayPanic(
                            expr_source.get_string(),
                            Some(format!("{:?}", method_did)),
                        );
                        self.source_cond_map
                            .entry(expr_source.clone())
                            .and_modify(|_v| {
                                warn!("Duplicated condition for MayPanic: {:?}", expr_source);
                            })
                            .or_default()
                            .push(cond);
                    }
                }
            }
            // TODO: handle associated function calls
            rustc_hir::ExprKind::Call(_, _) => {}
            // Index expressions like vec[0], but skip slice expressions
            rustc_hir::ExprKind::Index(_, _, _) => {
                if let Some(method_did) = self.typeck_res.type_dependent_def_id(ex.hir_id) {
                    let expr_source = SourceInfo::from_span(ex.span, self.tcx.sess.source_map());
                    let cond = Condition::MayPanic(
                        expr_source.get_string(),
                        Some(format!("{:?}", method_did)),
                    );
                    self.source_cond_map
                        .entry(expr_source.clone())
                        .and_modify(|_v| {
                            warn!("Duplicated condition for MayPanic: {:?}", expr_source);
                        })
                        .or_default()
                        .push(cond);
                }
            }
            _ => {}
        }
        intravisit::walk_expr(self, ex);
    }
}
