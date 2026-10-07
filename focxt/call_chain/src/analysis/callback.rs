use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::mir::Operand;
use rustc_middle::mir::TerminatorKind;
use rustc_middle::ty::GenericArgKind;
use rustc_middle::ty::Ty;
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::TyKind;
use std::collections::HashSet;
use std::fs::create_dir_all;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use syn::token::Impl;

use super::exporter::CallsAndTypes;
use super::hirvisitor::HirVisitor;
use super::hirvisitor::ImplInformation;
use super::hirvisitor::VisitorData;

pub struct CallChainCallbacks {
    pub source_name: String,
    pub crate_path: PathBuf, // cond_map: HashMap<SourceInfo, Condition>,
}

impl CallChainCallbacks {
    pub fn new(crate_path: PathBuf) -> Self {
        Self {
            source_name: String::new(),
            crate_path: crate_path, // cond_map: HashMap::new(),
        }
    }
}

impl rustc_driver::Callbacks for CallChainCallbacks {
    /// Called before creating the compiler instance
    fn config(&mut self, config: &mut interface::Config) {
        self.source_name = format!("{:?}", config.input.source_name());
        config.crate_cfg.push("call_chain".to_string());
        info!("Source file: {}", self.source_name);
    }

    fn after_expansion<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        _tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        self.run_analysis(_tcx);

        Compilation::Continue
    }
}

fn collect_subtypes<'tcx>(ty: Ty<'tcx>, tcx: TyCtxt<'tcx>, result: &mut HashSet<Ty<'tcx>>) {
    let ty = ty.peel_refs();
    if !result.insert(ty) {
        return;
    }
    result.insert(ty);

    match ty.kind() {
        TyKind::Adt(adt, args) => {
            for arg in args.iter() {
                if let GenericArgKind::Type(sub_ty) = arg.unpack() {
                    collect_subtypes(sub_ty, tcx, result);
                }
            }
        }

        TyKind::Array(sub_ty, _) => {
            collect_subtypes(*sub_ty, tcx, result);
        }

        TyKind::Slice(sub_ty) => {
            collect_subtypes(*sub_ty, tcx, result);
        }

        TyKind::RawPtr(ty_mut, _) => {
            collect_subtypes(*ty_mut, tcx, result);
        }

        TyKind::Tuple(sub_tys) => {
            for sub_ty in sub_tys.iter() {
                collect_subtypes(sub_ty, tcx, result);
            }
        }
        _ => {}
    }
}

impl CallChainCallbacks {
    fn run_analysis<'tcx, 'compiler>(&mut self, tcx: TyCtxt<'tcx>) {
        let mut visitor = HirVisitor::new(tcx);
        // hir_map.visit_all_item_likes_in_crate(&mut visitor);
        tcx.hir_walk_toplevel_module(&mut visitor);
        let result = visitor.move_result();
        let mut impl_informations: Vec<ImplInformation> = Vec::new();
        for data in result {
            let VisitorData {
                id,
                fn_name,
                impl_information,
                doc,
                has_ret,
                mod_info,
                visible,
                fn_source,
                basic_blocks,
                local_decls,
            } = data;
            // println!("{}", mod_info.name);
            impl_informations.push(impl_information.clone());
            let mut calls: HashSet<String> = HashSet::new();
            let mut tys: HashSet<Ty<'tcx>> = HashSet::new();
            let mut types: HashSet<String> = HashSet::new();
            for basic_block in basic_blocks.iter() {
                if let TerminatorKind::Call {
                    func,
                    args,
                    destination,
                    target,
                    unwind,
                    call_source,
                    fn_span,
                } = &basic_block.terminator().kind
                {
                    // println!("{:#?}", &basic_block.terminator().kind);
                    let kind_string = format!("{:#?}", &basic_block.terminator().kind);
                    let kind_strings: Vec<&str> = kind_string.splitn(3, ' ').into_iter().collect();
                    let kind_string = kind_strings[2];
                    let call_string = &kind_string[..kind_string.find("(").unwrap()];
                    calls.insert(call_string.to_string());

                    for arg in args.iter() {
                        if let Operand::Constant(constant) = &arg.node {
                            // let arg_type = constant.ty().peel_refs().to_string();
                            // types.insert(arg_type);
                            collect_subtypes(constant.ty(), tcx, &mut tys);
                        }
                    }
                }
            }
            // println!("{}", fn_name);
            // println!("Calls:");
            // for call in calls.iter() {
            //     println!("{:#?}", call);
            // }
            for local_decl in local_decls.iter() {
                // let decl_type = local_decl.ty.peel_refs().to_string();
                // println!("{:#?}", local_decl.ty.peel_refs().to_string());
                // types.insert(decl_type);
                collect_subtypes(local_decl.ty, tcx, &mut tys);
            }
            for ty in tys.iter() {
                types.insert(ty.to_string());
            }
            // println!("Types:");
            // for a_type in types.iter() {
            //     println!("{:#?}", a_type);
            // }
            // println!();
            // let mut new_calls: HashSet<String> = HashSet::new();
            // for call in calls.iter() {
            //     if call.chars().nth(call.len() - 1).unwrap() == '>' {
            //         let mut index = None;
            //         for (i, c) in call.chars().rev().enumerate() {
            //             if c == '<' {
            //                 index = Some(call.len() - i - 3);
            //                 break;
            //             }
            //         }
            //         if let Some(pos) = index {
            //             let sub_call = String::from(&call[..pos]);
            //             new_calls.insert(sub_call);
            //         }
            //     }
            // }
            // for new_call in new_calls.iter() {
            //     calls.insert(new_call.clone());
            // }
            let calls_and_types = CallsAndTypes::new(&mod_info.name, &calls, &types);
            let directory_path = self.crate_path.join("focxt/callsandtypes");
            create_dir_all(&directory_path).unwrap();
            let file_path = PathBuf::from(&directory_path)
                .join(format!("{}.json", impl_information.encoded_name.clone()));
            let mut file = File::create(&file_path).unwrap();
            file.write_all(serde_json::to_string(&calls_and_types).unwrap().as_bytes())
                .unwrap();

            let directory_path = self.crate_path.join("focxt/basic_blocks");
            create_dir_all(&directory_path).unwrap();
            let file_path = PathBuf::from(&directory_path)
                .join(format!("{}.txt", impl_information.encoded_name.clone()));
            let mut file = File::create(&file_path).unwrap();
            file.write_all(format!("{:#?}\n{:#?}", basic_blocks, local_decls).as_bytes())
                .unwrap();
        }
        let file_path =
            PathBuf::from(&self.crate_path).join(format!("focxt/impl_informations.json"));
        let mut file = File::create(&file_path).unwrap();
        file.write_all(
            serde_json::to_string(&impl_informations)
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
    }
}
