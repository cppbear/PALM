use super::fnblocks::{FnBlocks, MyBlock};
use super::hirvisitor::{HirVisitor, VisitorData};
use rustc_data_structures::graph::StartNode;
use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::mir::BasicBlock;
use rustc_middle::ty::TyCtxt;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub struct BrInfoCallbacks {
    pub source_name: String,
    crate_dir: PathBuf,
    // cond_map: BTreeMap<SourceInfo, Condition>,
}

impl BrInfoCallbacks {
    pub fn new(crate_dir: PathBuf) -> Self {
        Self {
            source_name: String::new(),
            crate_dir,
            // cond_map: BTreeMap::new(),
        }
    }
}

impl rustc_driver::Callbacks for BrInfoCallbacks {
    /// Called before creating the compiler instance
    fn config(&mut self, config: &mut interface::Config) {
        self.source_name = format!("{:?}", config.input.source_name());
        config.crate_cfg.push("brinfo".to_string());
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

impl BrInfoCallbacks {
    fn run_analysis<'tcx, 'compiler>(&mut self, tcx: TyCtxt<'tcx>) {
        let mut visitor = HirVisitor::new(self.crate_dir.clone(), tcx);
        tcx.hir_walk_toplevel_module(&mut visitor);
        let result = visitor.move_result();

        let mut ret: Vec<FnBlocks> = vec![];
        for data in result {
            let VisitorData {
                // id,
                fn_name,
                name_with_impl,
                encoded_name,
                doc,
                has_ret,
                mod_info,
                visible,
                fn_source,
                basic_blocks,
                cond_map,
            } = data;
            let mut fn_blocks: Vec<MyBlock> = vec![];
            let blocks: &rustc_middle::mir::BasicBlocks<'_> = &basic_blocks;
            let pre_blocks = blocks.predecessors();
            let mut suc_infos: BTreeMap<BasicBlock, Vec<BasicBlock>> = BTreeMap::new();
            for i in 0..=blocks.len() - 1 {
                suc_infos.insert(BasicBlock::from_usize(i), vec![]);
            }
            for (block, data) in blocks.iter_enumerated() {
                let statements = data.statements.clone();
                let terminator = data.terminator.clone().unwrap();
                let mut block_pre_blocks: Vec<BasicBlock> = vec![];
                for b in &pre_blocks[block] {
                    block_pre_blocks.push(*b);
                    suc_infos.get_mut(b).unwrap().push(block);
                }
                let a_block = MyBlock {
                    block_name: block,
                    statements,
                    terminator,
                    pre_blocks: block_pre_blocks,
                    suc_blocks: vec![],
                };
                fn_blocks.push(a_block);
            }
            for block in fn_blocks.iter_mut() {
                block.suc_blocks = suc_infos.get(&block.block_name).unwrap().clone();
            }
            let a_fn_block = FnBlocks::new(
                // id,
                fn_name,
                name_with_impl,
                encoded_name,
                doc,
                has_ret,
                fn_source,
                mod_info,
                visible,
                blocks.start_node(),
                fn_blocks,
                blocks.dominators().clone(),
                tcx.sess.source_map(),
                cond_map.clone(),
            );
            ret.push(a_fn_block);
        }

        let mut name_map = BTreeMap::new();
        for mut block in ret {
            // info!("Start analysis for {}", block.id);
            info!(
                "Start analysis for {} with encoded name {}",
                block.name, block.encoded_name
            );
            block.mir_out(&self.crate_dir);
            block.dump_cfg_to_dot(&self.crate_dir);
            let result = block.iterative_dfs();
            if result {
                // info!("Dump condition chains to json");
                block.dump_to_json(&self.crate_dir);
                name_map.insert(block.name, block.encoded_name);
            }
        }
        // info!("Dump name map to json");
        let name_map_path = self.crate_dir.join("brinfo/name_map.json");
        if name_map_path.exists() {
            let old_map: BTreeMap<String, String> =
                serde_json::from_str(&fs::read_to_string(&name_map_path).unwrap()).unwrap();
            name_map.extend(old_map);
        }
        let buf = serde_json::to_string_pretty(&name_map).unwrap();
        fs::write(name_map_path, buf).unwrap();
    }
}
