use super::exporter::ModInfo;
use super::sourceinfo::SourceInfo;
use regex::Regex;
use rustc_ast::token::CommentKind;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::CRATE_DEF_ID;
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{self, BodyId, FnDecl};
use rustc_middle::hir::nested_filter;
use rustc_middle::mir::{BasicBlockData, LocalDecl};
use rustc_middle::ty;
use rustc_middle::ty::TyCtxt;
use rustc_span::symbol::sym;
use serde::{Deserialize, Serialize};
use syn::parse_str;
use twox_hash::XxHash3_64;

fn is_valid_code(code: &str) -> bool {
    parse_str::<syn::Item>(code).is_ok()
}

fn encoded_name(s: &str) -> String {
    base62::encode(XxHash3_64::oneshot(s.as_bytes()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct ImplInformation {
    pub mod_name: String,
    pub fn_name: String,
    pub struct_name: String,
    pub trait_name: String,
    pub full_name: String,
    pub encoded_name: String,
}

pub struct VisitorData<'tcx> {
    pub id: String,
    pub fn_name: String,
    pub impl_information: ImplInformation,
    pub doc: String,
    pub has_ret: bool,
    pub mod_info: ModInfo,
    pub visible: bool,
    pub fn_source: SourceInfo,
    pub basic_blocks: Vec<BasicBlockData<'tcx>>,
    pub local_decls: Vec<LocalDecl<'tcx>>,
}

pub struct HirVisitor<'tcx> {
    tcx: TyCtxt<'tcx>,
    mod_infos: Vec<ModInfo>,
    result: Vec<VisitorData<'tcx>>,
    re: Regex,
}

impl<'tcx> HirVisitor<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        HirVisitor {
            tcx,
            mod_infos: Vec::new(),
            result: Vec::new(),
            re: Regex::new(r"\{impl#\d+\}").unwrap(),
        }
    }

    pub fn move_result(self) -> Vec<VisitorData<'tcx>> {
        self.result
    }

    fn is_accessible_from_crate(
        &self,
        def_id: rustc_hir::def_id::DefId,
        source: &SourceInfo,
    ) -> bool {
        let visibility = self.tcx.visibility(def_id);
        visibility.is_accessible_from(CRATE_DEF_ID.to_def_id(), self.tcx)
            && !source.get_file().contains("main.rs")
    }

    fn replace_impl_name(&self, impl_id: rustc_hir::def_id::DefId, def_str: &String) -> String {
        let impl_name = self.tcx.def_path_str(impl_id);
        self.re.replace(def_str, impl_name).to_string()
    }
}

impl<'tcx> Visitor<'tcx> for HirVisitor<'tcx> {
    type NestedFilter = nested_filter::All;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.tcx
    }

    fn visit_mod(
        &mut self,
        m: &'tcx rustc_hir::Mod<'tcx>,
        _s: rustc_span::Span,
        n: rustc_hir::HirId,
    ) -> Self::Result {
        let mod_source = SourceInfo::from_span(_s, self.tcx.sess.source_map());
        let def_id = n.owner.to_def_id();
        let mut module_name = self.tcx.def_path(def_id).to_string_no_crate_verbose();
        module_name = self.tcx.crate_name(def_id.krate).to_string() + &module_name;
        info!("Visiting module: {}, {:?}", module_name, mod_source);
        self.mod_infos.push(ModInfo {
            name: module_name.clone(),
            loc: mod_source,
        });
        intravisit::walk_mod(self, m);
        info!("Leaving module: {}", module_name);
        self.mod_infos.pop();
    }

    fn visit_fn(
        &mut self,
        fk: intravisit::FnKind<'tcx>,
        _fd: &'tcx FnDecl<'tcx>,
        b: BodyId,
        span: rustc_span::Span,
        id: rustc_hir::def_id::LocalDefId,
    ) -> Self::Result {
        let id_str = format!("{:?}", id);
        let def_id = id.to_def_id();
        let fn_name = self
            .tcx
            .def_path_str(def_id)
            .split("::")
            .last()
            .unwrap()
            .to_string();
        info!("Visiting function: {}, name: {}", id_str, fn_name);

        let mod_info = self.mod_infos.last().unwrap();
        let has_ret = matches!(_fd.output, rustc_hir::FnRetTy::Return(_));

        // Skip functions that are automatically derived
        for parent in self.tcx.hir_parent_id_iter(b.hir_id) {
            let attrs = self.tcx.hir_attrs(parent);
            if attrs
                .iter()
                .any(|attr| attr.has_name(sym::automatically_derived))
            {
                warn!("Skip because it is automatically derived");
                return;
            }
        }

        // Skip functions that are not valid code
        let fn_source = SourceInfo::from_span(span, self.tcx.sess.source_map());
        let code = fn_source.get_string();
        if !is_valid_code(&code) {
            warn!("Skip because it is not valid code");
            return;
        }

        let hir = self.tcx.hir_body(b);
        let steal_mir = self.tcx.mir_built(id);
        if steal_mir.is_stolen() {
            error!("Skip because MIR has been stolen");
            return;
        }
        let mir = steal_mir.borrow();

        // check visibility
        let visible = self.is_accessible_from_crate(def_id, &fn_source);

        // get doc comments
        let hir_id = self.tcx.local_def_id_to_hir_id(id);
        let attrs = self.tcx.hir_attrs(hir_id);
        let mut doc = String::new();
        for attr in attrs {
            if let Some((sym, kind)) = attr.doc_str_and_comment_kind() {
                match kind {
                    CommentKind::Line => {
                        doc += &format!("///{}\n", sym.to_string());
                    }
                    CommentKind::Block => {
                        doc += &format!("/**{}*/\n", sym.to_string());
                    }
                }
            }
        }

        let mut struct_name = String::new();
        let mut trait_name = String::new();
        let def_id = id.to_def_id();
        let parent_def_id = self.tcx.parent(def_id);
        let parent_kind = self.tcx.def_kind(parent_def_id);

        match parent_kind {
            DefKind::Impl { of_trait } => {
                let impl_ty = self.tcx.type_of(parent_def_id).instantiate_identity();

                if let ty::Adt(adt_def, _) = impl_ty.kind() {
                    struct_name = self
                        .tcx
                        .def_path_str(adt_def.0.did)
                        .split("::")
                        .last()
                        .unwrap()
                        .to_string();
                }

                if of_trait {
                    if let Some(trait_def_id) = self.tcx.trait_id_of_impl(parent_def_id) {
                        trait_name = self
                            .tcx
                            .def_path_str(trait_def_id)
                            .split("::")
                            .last()
                            .unwrap()
                            .to_string();
                    }
                }
            }
            DefKind::Trait => {
                trait_name = self
                    .tcx
                    .def_path_str(parent_def_id)
                    .split("::")
                    .last()
                    .unwrap()
                    .to_string();
            }
            _ => {}
        }

        let crate_name = self.tcx.crate_name(def_id.krate).to_string();
        let mut def_str = self.tcx.def_path(def_id).to_string_no_crate_verbose();
        let name_with_impl = crate_name.clone() + &def_str;
        if let intravisit::FnKind::Method(_, _) = fk {
            if let Some(impl_id) = self.tcx.impl_of_method(def_id) {
                def_str = self.replace_impl_name(impl_id, &def_str);
            }
        }
        let full_name = crate_name + &def_str;
        let encoded_name = encoded_name(&full_name);

        let impl_information = ImplInformation {
            mod_name: mod_info.name.clone(),
            fn_name: fn_name.clone(),
            struct_name,
            trait_name,
            full_name: full_name,
            encoded_name: encoded_name,
        };

        let data = VisitorData {
            id: id_str,
            fn_name,
            impl_information,
            doc,
            has_ret,
            mod_info: mod_info.clone(),
            visible,
            fn_source,
            basic_blocks: mir.basic_blocks.raw.to_vec(),
            local_decls: mir.local_decls.raw.to_vec(),
        };

        self.result.push(data);

        // intravisit::walk_fn(self, fk, fd, b, id);
    }
}
