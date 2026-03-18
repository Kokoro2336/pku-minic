//! IR Lowering from Mid IR to Lower IR.

use super::config::PARAM_REG_MAX_NUM;
use crate::base::Type;
use crate::frontend::ast::Literal;
use crate::ir::lower::*;
use crate::ir::machine::{Data, FReg, MOperand, MType, Reg, RoData, Slot, XReg};
use crate::ir::mid::*;
use crate::utils::bitset::BitSet;
use crate::utils::r#match::match_minor;
use crate::utils::worklist::*;

use rustc_hash::FxHashMap;

pub struct Lowering {
    ir: IR,
    builder: LBuilder,
    lower_ir: LowerIR,

    /// Temporary Map between FuncId -> LFuncId
    func_map: Vec<LOperand>,
    /// Temporary Map between IR Global -> LGlobal
    global_map: Vec<LOperand>,
    /// Temporary Map between BBId -> LBasicBlock
    block_map: Vec<LOperand>,
    /// IR OpId -> VirtId. Remember that NOT EVERY LOp has a mapping to its vreg in value_map,
    /// since some of them produce temp vreg.
    value_map: Vec<LOperand>,
    /// Param Idx -> SlotId/VirtId
    param_map: Vec<LOperand>,

    /// Worklist
    worklist: Worklist<usize, BitSet>,
    processed: BitSet,

    /// Move instruction buffer for Phi
    /// OpId -> BBId
    phis: Vec<(usize, usize)>,
}

macro_rules! match_rd {
    (
        target: $target:expr,

        op_with_rds: [ $($op_with_rd:ident),* $(,)? ],
        // Match arms.
        rd_arm: $SrcRd:ident($rd:ident) => $rd_body:block,

        // Handwritten fallback branches (captured by tt)
        fallback: { $($rest:tt)* }
    ) => {
        match $target {
            // Unroll the rd arms.
            $(
                $SrcRd::$op_with_rd { rd: $rd, .. } => $rd_body,
            )*
            // Unroll the rest handwritten branches.
            $($rest)*
        }
    };
}

impl Lowering {
    pub fn new(ir: IR) -> Self {
        Self {
            ir,
            lower_ir: LowerIR::new(),
            builder: LBuilder::new(),
            func_map: Vec::new(),
            global_map: Vec::new(),
            block_map: Vec::new(),
            value_map: Vec::new(),
            param_map: Vec::new(),
            worklist: Worklist::new(),
            processed: BitSet::new(),
            phis: Vec::new(),
        }
    }

    fn legalize_imm(&mut self, imm: LOperand) -> LOperand {
        const INT_IMM_MAX: i32 = 2047;
        const INT_IMM_MIN: i32 = -2048;

        match_minor!(
            target: imm,
            minor_arms: {
                LOperand::IntImm(imm) => {
                    if !(INT_IMM_MIN..=INT_IMM_MAX).contains(&imm) {
                        // create a new LoadIntImm instruction and return the LOpId.
                        let vreg_id = self.alloc_vreg(VirtReg {
                            defs: vec![],
                            phys: None,
                        });
                        let lop_id = self.create(LOp::new(
                            Type::Int.into(),
                            vec![],
                            LOpData::LoadIntImm {
                                rd: LOperand::Undef,
                                imm,
                            },
                        ));
                        self.bind(lop_id, vreg_id.clone());
                        vreg_id
                    } else {
                        LOperand::IntImm(imm)
                    }
                },
                LOperand::FloatImm(imm) => {
                    // Float can never reside in immediate field of any instrucitons,
                    // So we always create a new LoadFloatImm instruction and return the LOpId.
                    let vreg_id = self.alloc_vreg(VirtReg {
                        defs: vec![],
                        phys: None,
                    });
                    let lop_id = self.create(LOp::new(
                        Type::Float.into(),
                        vec![],
                        LOpData::LoadFloatImm {
                            rd: LOperand::Undef,
                            imm,
                        },
                    ));
                    self.bind(lop_id, vreg_id.clone());
                    vreg_id
                }
            },
            uni_ops: [LOperand::Undef, LOperand::Virt, LOperand::Phys, LOperand::Func, LOperand::BB, LOperand::Inst, LOperand::Data, LOperand::Slot, LOperand::RoData],
            other_patterns: [],
            uni_arm: {
                unreachable!("Only IntImm and FloatImm can be immediats, but got {:?}", imm)
            }
        )
    }

    /// Getter
    fn get(&mut self, operand: Operand) -> LOperand {
        crate::debug::info!("get {:?}", operand);
        match operand {
            Operand::Global(id) => self.global_map[id].clone(),
            Operand::BB(id) => self.block_map[id].clone(),
            Operand::Func(id) => self.func_map[id].clone(),
            // When getting an IR value, we get Vreg of LOp.
            Operand::Value(id) => {
                let lop_id = self.value_map[id].clone();

                // If the operand is a data or slot operand, return it directly.
                if matches!(
                    lop_id,
                    LOperand::Data(_) | LOperand::Slot(_) | LOperand::RoData(_)
                ) {
                    return lop_id;
                }

                let current_function = self.builder.current_function.expect("No current function");
                let lop = &self.lower_ir.funcs[current_function].dfg[lop_id.clone()];

                match_rd! {
                    target: &lop.data,
                    op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Uitofp, Zext, Load, LoadFloatImm, LoadIntImm, Move],
                    rd_arm: LOpData(rd) => {
                        rd.clone()
                    },
                    fallback: {
                        // For other LOpData which doesn't have rd field (e.g. Call and Store), we return Undef.
                        LOpData::Store {..}
                        | LOpData::Call {..}
                        | LOpData::Br {..}
                        | LOpData::Jump {..}
                        | LOpData::Ret => unreachable!("Only LOp with rd field can be mapped to IR value, but got {:?}", lop.data),
                    }
                }
            }
            Operand::Param { idx, .. } => self.param_map[idx].clone(),

            // Legalize immediats when getting 'em.
            Operand::Bool(imm) => self.legalize_imm(LOperand::IntImm(imm as i32)),
            Operand::Int(imm) => self.legalize_imm(LOperand::IntImm(imm)),
            Operand::Float(imm) => self.legalize_imm(LOperand::FloatImm(imm)),
            Operand::Undefined => LOperand::Undef,
        }
    }

    fn get_op_type(&self, operand: Operand) -> Type {
        let current_function = self.builder.current_function.expect("No current function");
        match operand {
            Operand::Global(id) => self.ir.globals[id].typ.clone(),
            Operand::BB(_) => unreachable!("BB operand should not be used in get_op_type"),
            Operand::Func(id) => self.ir.funcs[id].typ.clone(),
            Operand::Value(id) => {
                let op = &self.ir.funcs[current_function].dfg[id];
                op.typ.clone()
            }
            Operand::Param { idx, .. } => match &self.ir.funcs[current_function].typ {
                Type::Function { param_types, .. } => param_types[idx].clone(),
                _ => unreachable!("Only function type should be in the function arena"),
            },
            Operand::Bool(_) => Type::Bool,
            Operand::Int(_) => Type::Int,
            Operand::Float(_) => Type::Float,
            Operand::Undefined => Type::Void,
        }
    }

    /// Setter
    fn set(&mut self, operand: Operand, value: LOperand) {
        match operand {
            Operand::Value(id) => self.value_map[id] = value,
            Operand::Global(id) => self.global_map[id] = value,
            Operand::BB(id) => self.block_map[id] = value,
            Operand::Func(id) => self.func_map[id] = value,
            Operand::Param { idx, .. } => self.param_map[idx] = value,
            Operand::Bool(_) | Operand::Int(_) | Operand::Float(_) | Operand::Undefined => (),
        }
    }

    fn init(&mut self, idx: usize) {
        self.builder.set_current_func(Some(idx));

        // Clear the maps.
        self.block_map.clear();
        self.value_map.clear();
        self.param_map.clear();
        self.worklist.clear();
        self.processed.clear();
        self.phis.clear();

        // Resize the maps.
        self.block_map
            .resize(self.ir.funcs[idx].cfg.len(), LOperand::Undef);
        self.value_map
            .resize(self.ir.funcs[idx].dfg.len(), LOperand::Undef);
        let param_num = match &self.ir.funcs[idx].typ {
            Type::Function { param_types, .. } => param_types.len(),
            _ => unreachable!("Only function type should be in the function arena"),
        };
        self.param_map.resize(param_num, LOperand::Undef);
    }

    // ========== Scafolding for mapping IR entities to LIR entities ==========

    #[inline(always)]
    fn alloc_and_map_func(&mut self, func_id: Operand, lfunc: LFunction) -> LOperand {
        let lfunc_id = self.lower_ir.funcs.alloc(lfunc);
        self.set(func_id, LOperand::Func(lfunc_id));
        LOperand::Func(lfunc_id)
    }

    #[inline(always)]
    fn alloc_and_map_data(
        &mut self,
        global_id: Operand,
        name: Option<String>,
        data: Data,
    ) -> LOperand {
        let data_id = match name {
            Some(name) => self.lower_ir.data_info.insert(data, name),
            None => self.lower_ir.data_info.alloc(data),
        };
        self.set(global_id, LOperand::Data(data_id));
        LOperand::Data(data_id)
    }

    #[inline(always)]
    fn alloc_and_map_rodata(
        &mut self,
        global_id: Operand,
        name: Option<String>,
        rodata: RoData,
    ) -> LOperand {
        let rodata_id = match name {
            Some(name) => self.lower_ir.rodata_info.insert(rodata, name),
            None => self.lower_ir.rodata_info.alloc(rodata),
        };
        self.set(global_id, LOperand::Data(rodata_id));
        LOperand::Data(rodata_id)
    }

    #[inline(always)]
    fn alloc_and_map_slot(&mut self, alloc_id: Operand, slot: Slot) -> LOperand {
        crate::debug::info!("Map alloc {:?} to slot {:?}]", alloc_id, slot);
        let func_id = self.builder.current_function.expect("No current function");
        let lfunc = &mut self.lower_ir.funcs[func_id];
        let slot_id = lfunc.frame_info.alloc(slot);
        self.set(alloc_id, LOperand::Slot(slot_id));
        LOperand::Slot(slot_id)
    }

    #[inline(always)]
    fn alloc_and_map_block(&mut self, bb_id: Operand, lbb: LBasicBlock) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lbb_id = self.lower_ir.funcs[func_id].cfg.alloc(lbb);
        self.set(bb_id, LOperand::BB(lbb_id));
        LOperand::BB(lbb_id)
    }

    /// When creating LOp which produces a value that can be mapped to IR's value, you'd better use this.
    #[inline(always)]
    fn alloc_and_map_lop(&mut self, op_id: Operand, lop: LOp) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lop_id = self.lower_ir.funcs[func_id].dfg.alloc(lop);
        self.set(op_id, LOperand::Inst(lop_id));
        LOperand::Inst(lop_id)
    }

    // ========== Scafolding for temporary values' mapping ========

    // ========== Atomic operations ==========

    /// When creating LOp which produces a temp value, you'd better use this.
    #[inline(always)]
    fn create(&mut self, lop: LOp) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lop_id = self.lower_ir.funcs[func_id].dfg.alloc(lop);
        LOperand::Inst(lop_id)
    }

    #[inline(always)]
    fn alloc_vreg(&mut self, vreg: VirtReg) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let vreg_id = self.lower_ir.funcs[func_id].vregs.alloc(vreg);
        LOperand::Virt(vreg_id)
    }

    #[inline(always)]
    fn alloc_slot(&mut self, slot: Slot) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let slot_id = self.lower_ir.funcs[func_id].frame_info.alloc(slot);
        LOperand::Slot(slot_id)
    }

    #[inline(always)]
    fn alloc_rodata(&mut self, rodata: RoData) -> LOperand {
        let rodata_id = self.lower_ir.rodata_info.alloc(rodata);
        LOperand::Data(rodata_id)
    }

    fn bind(&mut self, lop_id: LOperand, vreg_id: LOperand) {
        let data = &mut self.lower_ir.funcs
            [self.builder.current_function.expect("No current function")]
        .dfg[lop_id.clone()]
        .data;

        match_rd! {
            target: data,
            op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Uitofp, Zext, Load, Move, LoadFloatImm, LoadIntImm],
            rd_arm: LOpData(rd) => {
                *rd = vreg_id.clone();
            },
            fallback: {
                // Only Move can be binded with vreg, since other LOp with rd field are not created for temp values.
                LOpData::Br {..}
                | LOpData::Jump {..}
                | LOpData::Store {..}
                | LOpData::Call {..}
                | LOpData::Ret => unreachable!("Only Move can be binded with vreg, but got {:?}", data),
            }
        }

        let vreg = &mut self.lower_ir.funcs
            [self.builder.current_function.expect("No current function")]
        .vregs[vreg_id];
        vreg.defs.push(lop_id);
    }

    fn get_param_regs(param_types: &[Type]) -> Vec<Reg> {
        let mut x_params = XReg::get_param_regs();
        let mut f_params = FReg::get_param_regs();

        param_types
            .iter()
            .map(|param_type| match param_type {
                Type::Float => Reg::F(f_params.remove(0)),
                Type::Bool | Type::Int | Type::Pointer { .. } => Reg::X(x_params.remove(0)),
                Type::Array { .. } | Type::Function { .. } | Type::Void | Type::Char => {
                    unreachable!("Array, Function, Void and Char type should not be directly passed as parameters")
                }
            })
            .collect::<Vec<Reg>>()
    }

    // ======== Lowering Logic ========

    /// TODO: Might be replaced by kaguya.
    fn lower_op(&mut self, op_id: Operand) {
        let func_id = self.builder.current_function.expect("No current function");
        let (typ, attrs, data) = {
            let op = &self.ir.funcs[func_id].dfg[op_id.clone()];
            (
                self.get_op_type(op_id.clone()),
                op.attrs.clone(),
                op.data.clone(),
            )
        };
        crate::debug::info!("Lowering {:?} {:?}", op_id, data);

        // Translate attrs first.
        let lattr = attrs
            .iter()
            .filter_map(|attr| match attr {
                Attr::Name(_) | Attr::FuncName(_) | Attr::GlobalArray { .. } => match attr {
                    Attr::FuncName(name) | Attr::Name(name) => Some(LAttr::Name(name.clone())),
                    Attr::GlobalArray { .. } | Attr::OldIdx(_) | Attr::Promotion => None,
                },
                Attr::OldIdx(_) | Attr::Promotion => None,
            })
            .collect();

        macro_rules! lower_ops_match {
            (
                // target to match
                target: $target:expr,

                // list of bin op
                bin_ops: [ $($bin_op:ident),* $(,)? ],

                // list of un op
                un_ops: [ $($un_op:ident),* $(,)? ],

                // other handwritten branches
                fallback: { $($rest:tt)* }
            ) => {
                match $target {
                    $(
                        OpData::$bin_op { lhs, rhs } => {
                            let lhs = self.get(lhs.clone());
                            let rhs = self.get(rhs.clone());
                            self.alloc_and_map_lop(op_id.clone(), LOp::new(
                                typ.clone().into(),
                                lattr,
                                LOpData::$bin_op {
                                    rd: LOperand::Undef,
                                    lhs,
                                    rhs,
                                },
                            ));
                        },
                    )*
                    $(
                        OpData::$un_op { value } => {
                            let value = self.get(value.clone());
                            self.alloc_and_map_lop(op_id.clone(), LOp::new(
                                typ.clone().into(),
                                lattr,
                                LOpData::$un_op {
                                    rd: LOperand::Undef,
                                    value,
                                },
                            ));
                        },
                    )*
                    $($rest)*
                }
            };
        }

        lower_ops_match! {
            target: data,
            bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe],
            un_ops: [Sitofp, Fptosi, Uitofp, Zext],
            fallback: {
                OpData::Br {
                    cond,
                    then_bb,
                    else_bb,
                } => {
                    let cond = self.get(cond.clone());
                    let then_bb = self.get(then_bb.clone());
                    let else_bb = self.get(else_bb.clone());
                    self.alloc_and_map_lop(op_id.clone(),
                        LOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Br {
                                cond,
                                then_bb,
                                else_bb,
                            }
                        )
                    );
                },
                OpData::Jump { target_bb } => {
                    let target_bb = self.get(target_bb.clone());
                    self.alloc_and_map_lop(
                        op_id.clone(),
                        LOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Jump {
                                target_bb,
                            },
                        ),
                    );
                },
                OpData::Load { addr } => {
                    let addr = self.get(addr.clone());
                    let vreg_id = self.alloc_vreg(VirtReg {
                        defs: vec![],
                        phys: None,
                    });
                    let lop_id = self.alloc_and_map_lop(
                        op_id.clone(),
                        LOp::new(
                            typ.clone().into(),
                            lattr,
                            LOpData::Load {
                                rd: LOperand::Undef,
                                addr,
                            },
                        ),
                    );
                    self.bind(lop_id, vreg_id);
                },
                OpData::Store { addr, value } => {
                    let addr = self.get(addr.clone());
                    let value = self.get(value.clone());
                    self.alloc_and_map_lop(
                        op_id.clone(),
                        LOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Store {
                                addr,
                                value,
                            },
                        ),
                    );
                },
                OpData::Call { func, args } => {
                    // Create move instructions for args
                    let func_type = self.get_op_type(func.clone());
                    let param_types = match &func_type {
                        Type::Function { param_types, .. } => param_types.clone(),
                        _ => unreachable!("Only function type can be called"),
                    };
                    let mut param_regs = Self::get_param_regs(
                        &param_types[..param_types.len().min(PARAM_REG_MAX_NUM as usize)]
                    );
                    for (idx, arg) in args.iter().enumerate() {
                        let arg_typ = self.get_op_type(arg.clone());
                        if idx < PARAM_REG_MAX_NUM as usize {
                            let vreg_id = self.alloc_vreg(VirtReg {
                                defs: vec![],
                                phys: Some(param_regs.remove(0)),
                            });
                            let arg = self.get(arg.clone());
                            let move_lop_id = self.create(LOp::new(
                                arg_typ.clone().into(),
                                vec![],
                                LOpData::Move {
                                    rd: LOperand::Undef,
                                    src: arg,
                                },
                            ));
                            self.bind(move_lop_id, vreg_id);
                        } else {
                            let slot_id = self.alloc_slot(Slot::Param {
                                size: arg_typ.size(),
                                align: arg_typ.align(),
                            });
                            let arg = self.get(arg.clone());
                            self.create(LOp::new(
                                Type::Void.into(),
                                vec![],
                                LOpData::Store {
                                    addr: slot_id.clone(),
                                    value: arg,
                                },
                            ));
                        }
                    }
                    // Create call instruction

                    let func = self.get(func.clone());
                    self.create(LOp::new(
                        // Since call doesn't produce a value in Lower IR, the type should be void.
                        Type::Void.into(),
                        vec![],
                        LOpData::Call {
                            func,
                        },
                    ));

                    // If the function returns a value, we create a move and bind the original VReg.
                    if typ != Type::Void {
                        let vreg_id = self.alloc_vreg(VirtReg {
                            defs: vec![],
                            phys: None,
                        });
                        let move_lop_id = self.alloc_and_map_lop(
                            op_id.clone(),
                            LOp::new(
                                typ.clone().into(),
                                vec![],
                                LOpData::Move {
                                    rd: LOperand::Undef,
                                    src: vreg_id.clone(),
                                },
                            ),
                        );
                        self.bind(move_lop_id, vreg_id.clone());
                    }
                }
                OpData::Phi { .. } => {
                    // Defer the processing of phis util the rest operations all have their LOp.
                    // Record the move instruction for Phi elimination later.
                    let current_block = self.builder.current_block.clone().expect("No current block");
                    self.phis.push((op_id.get_op_id(), current_block.get_bb_id()));
                }
                OpData::Alloca(typ) => {
                    // For Alloca, we need to allocate stack space in the function's frame.
                    self.alloc_and_map_slot(Operand::Value(op_id.get_op_id()), Slot::Local {
                        size: typ.size(),
                        align: typ.align(),
                    });
                }
                OpData::GEP { base, indices } => {
                    // GEP is only used for array in SysY.
                    let typ = self.get_op_type(base.clone());
                    let base_typ = match &typ {
                        Type::Pointer { base } => (**base).clone(),
                        _ => unreachable!("Only array type can be the base of GEP"),
                    };
                    // Truncate the first index of indices
                    let indices = if indices.len() > 1 {
                        indices[1..].to_vec()
                    } else {
                        vec![]
                    };

                    // Initialize the current base address with the base pointer.
                    let mut current_lop_id = self.get(base.clone());
                    for (dim, index) in indices.iter().enumerate() {
                        match &base_typ {
                            Type::Array { .. } => {
                                let mul_vreg_id = self.alloc_vreg(VirtReg {
                                    defs: vec![],
                                    phys: None,
                                });

                                let mul_lop = LOp::new(
                                    MType::U64,
                                    vec![],
                                    LOpData::MulI {
                                        rd: mul_vreg_id,
                                        lhs: self.get(index.clone()),
                                        rhs: LOperand::IntImm(base_typ.subarr_size(dim) as i32),
                                    },
                                );

                                let mul_lop_id = self.create(
                                    mul_lop,
                                );

                                // If the end of loop reached, bind the VReg of GEP to the current instruction.
                                let add_lop =
                                        LOp::new(
                                            MType::U64,
                                            vec![],
                                            LOpData::AddI {
                                                rd: LOperand::Undef,
                                                lhs: current_lop_id.clone(),
                                                rhs: mul_lop_id.clone(),
                                            },
                                        );

                                if dim == indices.len() - 1 {
                                    self.alloc_and_map_lop(
                                        op_id.clone(), add_lop
                                    );
                                } else {
                                    let add_lop_id = self.create(
                                        add_lop,
                                    );
                                    // Update current base address.
                                    current_lop_id = add_lop_id;
                                }
                            }
                            _ => {
                                let rhs = self.get(index.clone());
                                let mul_op = self.create(
                                    LOp::new(
                                        MType::U64,
                                        vec![],
                                        LOpData::MulI {
                                            rd: LOperand::Undef,
                                            lhs: LOperand::IntImm(base_typ.size() as i32),
                                            rhs,
                                        },
                                    ),
                                );

                                // If the pointee is scalar, the iteration will only has one step.
                                // We don't need to update current_lop_id, and we can directly bind the vreg of GEP to the Add.
                                self.alloc_and_map_lop(
                                    op_id.clone(),
                                    LOp::new(
                                        MType::U64,
                                        vec![],
                                        LOpData::AddI {
                                            rd: LOperand::Undef,
                                            lhs: current_lop_id.clone(),
                                            rhs: mul_op,
                                        },
                                    ),
                                );
                            }
                        }
                    }

                    // If the truncated indices is empty, we need to map the GEP to the base pointer's LOp InstId directly.
                    if indices.is_empty() {
                        crate::debug::info!("current_op_id: {:?}", current_lop_id);
                        let target_id = match_minor!(
                            target: current_lop_id,
                            minor_arms: {
                                LOperand::Virt(id) => self.lower_ir.funcs[func_id].vregs[id].defs[0].clone(),
                            },
                            uni_ops: [LOperand::Data, LOperand::Phys, LOperand::Slot, LOperand::BB, LOperand::Func, LOperand::Inst, LOperand::Undef, LOperand::IntImm, LOperand::FloatImm, LOperand::RoData],
                            other_patterns: [],
                            uni_arm: {
                                current_lop_id
                            }
                        );
                        self.set(Operand::Value(op_id.get_op_id()), target_id);
                    }
                }
                OpData::Ret { value } => {
                    if let Some(value) = value {
                        let value = self.get(value.clone());
                        self.create(LOp::new(
                            typ.into(),
                            vec![],
                            LOpData::Move {
                                rd: LOperand::Undef,
                                src: value,
                            },
                        ));
                    }
                    // Ret itself never binds with any value.
                    self.create(
                        LOp::new(Type::Void.into(), vec![], LOpData::Ret),
                    );
                }

                OpData::GlobalAlloca(_) | OpData::Declare { .. } => {
                    unreachable!("GlobalAlloca and Declare should have been handled in global lowering")
                }
            }
        }
    }

    /// Lowering the blocks in BFS order starting from the entry block.
    fn lower_bbs(&mut self) {
        let func_id = self.builder.current_function.expect("No current function");

        while let Some(bb_id) = self.worklist.pop_front() {
            if self.processed.contains(bb_id) {
                continue;
            }
            self.processed.insert(bb_id);

            if bb_id == self.ir.funcs[func_id].cfg.entry.expect("No entry block") {
                let func = &self.ir.funcs[func_id];

                self.alloc_and_map_func(
                    Operand::Func(self.builder.current_function.expect("No current function")),
                    LFunction::new(func.name.clone()),
                );

                // Create prologue.
                let func = &self.ir.funcs[func_id];
                let lentry = self.alloc_and_map_block(
                    Operand::BB(func.cfg.entry.expect("No entry block")),
                    LBasicBlock::new(),
                );

                let func = &self.ir.funcs[func_id];
                let param_types = match &func.typ {
                    Type::Function { param_types, .. } => param_types.clone(),
                    _ => unreachable!("Only function type should be in the function arena"),
                };

                self.builder.set_current_block(lentry);
                let mut params_reg = Self::get_param_regs(
                    &param_types[..param_types.len().min(PARAM_REG_MAX_NUM as usize)],
                );

                // Create moves and stack slots for parameters.
                for (idx, param_typ) in param_types.iter().enumerate() {
                    if idx < PARAM_REG_MAX_NUM as usize {
                        let vreg_id = self.alloc_vreg(VirtReg {
                            defs: vec![],
                            phys: None,
                        });

                        let lop_id = self.create(LOp::new(
                            (*param_typ).clone().into(),
                            vec![],
                            LOpData::Move {
                                // The rd will be filled by LBuilder::create().
                                rd: LOperand::Undef,
                                src: LOperand::Phys(params_reg.remove(0)),
                            },
                        ));
                        self.bind(lop_id, vreg_id.clone());

                        // Manually map the param to the vreg.
                        self.param_map[idx] = vreg_id;
                    } else {
                        let (size, align) = match &param_typ {
                            Type::Int | Type::Float => (param_typ.size(), param_typ.align()),
                            Type::Pointer { .. } => (param_typ.size(), param_typ.align()),
                            Type::Bool
                            | Type::Char
                            | Type::Void
                            | Type::Array { .. }
                            | Type::Function { .. } => {
                                unreachable!("Void type should not be passed as parameter")
                            }
                        };
                        let slot_id = self.alloc_slot(Slot::Param { size, align });
                        // Manually map the param to the slot.
                        self.param_map[idx] = slot_id;
                    }
                }
            }

            // The first iteration: Create Lower IR instructions
            let lbb_id = self.get(Operand::BB(bb_id));
            self.builder.set_current_block(lbb_id);
            let bb = &self.ir.funcs[func_id].cfg[bb_id];
            let cur = bb.cur.clone();

            // Lower the IR operations.
            for op_id in cur {
                self.lower_op(op_id);
            }

            // push successors to the worklist for later processing.
            let func = &self.ir.funcs[func_id];
            let entry_bb = &self.ir.funcs[func_id].cfg[func.cfg.entry.expect("No entry block")];
            let succs = entry_bb.succs.clone();
            for succ in succs {
                self.worklist.push_back(succ.get_bb_id());
            }
        }
    }

    fn resort_moves(&mut self, mut move_lop_ids: Vec<LOperand>) -> Vec<LOperand> {
        let mut new = vec![];
        let mut edges: Vec<(usize, usize)> = vec![];

        // Compute in-degree of each move.
        for move_lop_id in move_lop_ids.iter_mut() {
            let move_lop = &self.lower_ir.funcs
                [self.builder.current_function.expect("No current function")]
            .dfg[move_lop_id.clone()];
            let edge = match move_lop.data.clone() {
                LOpData::Move { src, rd } => {
                    if let (LOperand::Virt(_), LOperand::Virt(_)) = (src.clone(), rd.clone()) {
                        (src.get_virt_id(), rd.get_virt_id())
                    } else {
                        continue;
                    }
                }
                _ => unreachable!("Only Move LOp should be in the move_lop_ids"),
            };
            edges.push(edge);
        }

        // Schedule the moves
        // Schedule those with no out-degree first.
        let mut old_len = new.len();
        loop {
            for move_lop_id in move_lop_ids.iter_mut() {
                let move_lop = &self.lower_ir.funcs
                    [self.builder.current_function.expect("No current function")]
                .dfg[move_lop_id.clone()];
                let edge = match move_lop.data.clone() {
                    LOpData::Move { src, rd } => {
                        if let (LOperand::Virt(_), LOperand::Virt(_)) = (src.clone(), rd.clone()) {
                            (src.get_virt_id(), rd.get_virt_id())
                        } else {
                            continue;
                        }
                    }
                    _ => unreachable!("Only Move LOp should be in the move_lop_ids"),
                };
                if !edges.contains(&edge) {
                    new.push(move_lop_id.clone());
                    // Remove those edges starting from the source of the current move.
                    edges.retain(|(src, _)| *src != edge.0);
                }
            }

            if new.len() == old_len && !edges.is_empty() {
                // If there is a cycle, we can break it by inserting a temporary move.
                // Choose the first edge in the cycle to break.
                let (from, _) = *edges.first().unwrap();
                let temp_vreg_id = self.lower_ir.funcs
                    [self.builder.current_function.expect("No current function")]
                .vregs
                .alloc(VirtReg {
                    defs: vec![],
                    phys: None,
                });
                let temp_lop_id = self.builder.create(
                    &mut self.lower_ir,
                    self.builder.current_function,
                    LOp::new(
                        MType::U64,
                        vec![],
                        LOpData::Move {
                            rd: LOperand::Virt(temp_vreg_id),
                            src: LOperand::Virt(from),
                        },
                    ),
                );
                // We don't need to add the new move to edges.
                new.push(temp_lop_id.clone());
                // Replace the move from `from` to `temp`.
                for move_lop_id in move_lop_ids.iter_mut() {
                    let move_lop = &mut self.lower_ir.funcs
                        [self.builder.current_function.expect("No current function")]
                    .dfg[move_lop_id.clone()];
                    if let LOpData::Move { src, .. } = move_lop.data.clone() {
                        if src == LOperand::Virt(from) {
                            match &mut move_lop.data {
                                LOpData::Move { src, .. } => {
                                    *src = LOperand::Virt(temp_vreg_id);
                                }
                                _ => unreachable!("Only Move LOp should be in the move_lop_ids"),
                            }
                        }
                    }
                }
                for edge in edges.iter_mut() {
                    if edge.0 == from {
                        edge.0 = temp_vreg_id;
                    }
                }
                // After breaking the cycle, we continue to schedule the moves in the next iteration.
            } else if edges.is_empty() {
                break;
            }
            old_len = new.len();
        }

        new
    }

    fn create_trampoline(&mut self, edge: (usize, usize), new: Vec<LOperand>) {
        let (from, to) = (LOperand::BB(edge.0), LOperand::BB(edge.1));
        let tramp_id = self
            .builder
            .create_new_block(&mut self.lower_ir, self.builder.current_function);

        let from_bb = &mut self.lower_ir.funcs
            [self.builder.current_function.expect("No current function")]
        .cfg[from.clone()];
        let from_term_id = from_bb
            .cur
            .last()
            .expect("No terminator in the from block")
            .clone();
        let from_term = &self.lower_ir.funcs
            [self.builder.current_function.expect("No current function")]
        .dfg[from_term_id.clone()];

        // Update the control flow
        let new_lop = match from_term.data.clone() {
            LOpData::Br {
                cond,
                then_bb,
                else_bb,
            } => {
                if then_bb == to {
                    LOp::new(
                        Type::Void.into(),
                        vec![],
                        LOpData::Br {
                            cond,
                            then_bb: tramp_id.clone(),
                            else_bb,
                        },
                    )
                } else if else_bb == to {
                    LOp::new(
                        Type::Void.into(),
                        vec![],
                        LOpData::Br {
                            cond,
                            then_bb,
                            else_bb: tramp_id.clone(),
                        },
                    )
                } else {
                    unreachable!(
                        "The edge to be replaced should be in the terminator of the from block"
                    )
                }
            }
            LOpData::Jump { target_bb } => {
                if target_bb == to {
                    LOp::new(
                        Type::Void.into(),
                        vec![],
                        LOpData::Jump {
                            target_bb: tramp_id.clone(),
                        },
                    )
                } else {
                    unreachable!(
                        "The edge to be replaced should be in the terminator of the from block"
                    )
                }
            }
            _ => unreachable!("The terminator of the from block should be either Br or Jump"),
        };

        let current_function = self.builder.current_function;
        self.lower_ir.replace_op(
            &mut self.builder,
            current_function,
            from_term_id,
            from,
            new_lop,
        );

        // Insert terminator and the moves for phi elimination.
        {
            let mut guard = LBuilderGuard::new(&mut self.builder);
            guard.set_current_block(tramp_id.clone());

            self.lower_ir.funcs[current_function.expect("No current function")].cfg
                [tramp_id.clone()]
            .cur
            .extend(new);

            let jump_lop = LOp::new(Type::Void.into(), vec![], LOpData::Jump { target_bb: to });
            guard.create(&mut self.lower_ir, current_function, jump_lop);
        }
    }

    fn lower_global(&mut self) {
        // Pre-allocate global objects.
        for global in self.ir.globals.ids() {
            let global_op = &self.ir.globals[global];
            match global_op.data.clone() {
                OpData::GlobalAlloca(_) => {
                    let res = global_op.attrs.iter().find_map(|attr| match attr {
                        Attr::GlobalArray {
                            name,
                            mutable,
                            typ,
                            values,
                        } => Some((name.clone(), *mutable, typ.clone(), values.clone())),
                        _ => None,
                    });
                    if let Some((name, mutable, typ, values)) = res {
                        let values = match values {
                            Some(values) => values.iter().map(|v| match v {
                                Literal::Int(i) => MOperand::IntImm(*i),
                                Literal::Float(f) => MOperand::FloatImm(*f),
                                Literal::String(s) => unimplemented!(
                                    "String literal in global array initializer is not supported yet: {}",
                                    s
                                ),
                            }).collect(),
                            // If global array has no initializer, we need to fill it with default values according to the type.
                            None => match &typ {
                                Type::Int
                                | Type::Bool => vec![MOperand::IntImm(0)],
                                Type::Float => vec![MOperand::FloatImm(0.0)],
                                Type::Pointer { .. } => unimplemented!("Uninitialized global pointer is not supported yet"),
                                Type::Array { base, dims } => {
                                    let base_value = match &**base {
                                        Type::Int
                                        | Type::Bool => MOperand::IntImm(0),
                                        Type::Float => MOperand::FloatImm(0.0),
                                        Type::Pointer { .. } => unimplemented!("Uninitialized global pointer is not supported yet"),
                                        Type::Array { .. } => unimplemented!("Multi-dimensional array without initializer is not supported yet"),
                                        Type::Function { .. } | Type::Void | Type::Char => unreachable!("Function, Void and Char type should not be in the global array"),
                                    };
                                    vec![base_value.clone(); dims.iter().product::<u32>() as usize]
                                }
                                Type::Function {..}
                                | Type::Void
                                | Type::Char => unreachable!("Function type should not be in the global array"),
                            }
                        };

                        if mutable {
                            let data = Data::new(typ, values);
                            self.alloc_and_map_data(Operand::Global(global), Some(name), data);
                        } else {
                            let rodata = RoData::new(typ, values);
                            self.alloc_and_map_rodata(Operand::Global(global), Some(name), rodata);
                        }
                    }
                }
                OpData::Declare { .. } => { /*Ignore it*/ }
                _ => unreachable!("Only global alloca and declare should be in the global arena"),
            }
        }
        // Pre-allocate functions.
        for func_id in self.ir.funcs.ids() {
            let func = &self.ir.funcs[func_id];
            self.alloc_and_map_func(Operand::Func(func_id), LFunction::new(func.name.clone()));
        }
    }

    pub fn run(&mut self) -> LowerIR {
        self.func_map.resize(self.ir.funcs.len(), LOperand::Undef);
        self.global_map
            .resize(self.ir.globals.len(), LOperand::Undef);
        self.lower_global();

        // Pre-allocate functions.
        for func_id in self.ir.funcs.collect_internal() {
            self.init(func_id);

            // Pre-allocate basic blocks.
            let func = &self.ir.funcs[func_id];
            let entry = func.cfg.entry.expect("No entry block");
            for bb_id in func.cfg.ids() {
                self.alloc_and_map_block(Operand::BB(bb_id), LBasicBlock::new());
            }

            self.worklist.push_back(entry);
            self.lower_bbs();

            // Process phis.
            let mut phi_moves: FxHashMap<(usize, (usize, usize)), Vec<LOperand>> =
                FxHashMap::default();
            for (phi_id, phi_bb_id) in std::mem::take(&mut self.phis) {
                let (typ, phi_op_data) = {
                    let op = &self.ir.funcs[func_id].dfg[Operand::Value(phi_id)];
                    (self.get_op_type(Operand::Value(phi_id)), op.data.clone())
                };

                if let OpData::Phi { incomings } = phi_op_data {
                    for incoming in incomings {
                        let (value, bb_id) = match incoming {
                            PhiIncoming::Data { value, bb } => (value, bb),
                            PhiIncoming::None => continue,
                        };

                        let incoming_vreg_id = self.get(value.clone());
                        let move_lop = LOp::new(
                            typ.clone().into(),
                            vec![],
                            LOpData::Move {
                                rd: LOperand::Undef,
                                src: incoming_vreg_id,
                            },
                        );

                        // The moves will be binded to the same VReg allocated to Phi instruction previously.
                        let move_lop_id = self.alloc_and_map_lop(Operand::Value(phi_id), move_lop);
                        // Record the move_lop_id for later resorting and trampoline insertion.
                        phi_moves
                            .entry((phi_id, (bb_id.get_bb_id(), phi_bb_id)))
                            .or_default()
                            .push(move_lop_id);
                    }
                } else {
                    unreachable!("Only Phi should be in the phis map");
                }
            }

            // Refinement: reschedule the Moves generated by Phis and create trampolines.
            for key in phi_moves.keys().cloned() {
                let move_lop_ids = phi_moves[&key].clone();
                let resorted_moves = self.resort_moves(move_lop_ids);
                let (_, edge) = key;
                self.create_trampoline(edge, resorted_moves);
            }
        }

        std::mem::take(&mut self.lower_ir)
    }
}
