//! IR Lowering from Mid IR to Lower IR.

use yachiyo::ast::Literal;
use yachiyo::base::Type;
use yachiyo::config::PARAM_REG_MAX_NUM;
use yachiyo::ir::back::*;
use yachiyo::ir::mid::*;
use yachiyo::utils::r#match::match_some;
use yachiyo::utils::set::BitSet;
use yachiyo::utils::worklist::*;

use rustc_hash::FxHashMap;

pub struct Lowering {
    ir: IR,
    builder: BBuilder,
    lower_ir: BackIR,

    /// Temporary Map between FuncId -> LFuncId
    func_map: Vec<BOperand>,
    /// Temporary Map between IR Global -> LGlobal
    global_map: Vec<BOperand>,
    /// Temporary Map between BBId -> BBasicBlock
    block_map: Vec<BOperand>,
    /// IR OpId -> VirtId. Remember that NOT EVERY LOp has a mapping to its vreg in value_map,
    /// since some of them produce temp vreg.
    value_map: Vec<BOperand>,
    /// Param Idx -> SlotId/VirtId
    param_map: Vec<BOperand>,

    /// Worklist
    worklist: Worklist<usize, BitSet>,
    processed: BitSet,

    /// Move instruction buffer for Phi
    /// OpId -> BBId
    phis: Vec<(Operand, Operand)>,
}

impl Lowering {
    pub fn new(ir: IR) -> Self {
        Lowering {
            ir,
            builder: BBuilder::default(),
            lower_ir: BackIR::default(),
            func_map: vec![],
            global_map: vec![],
            block_map: vec![],
            value_map: vec![],
            param_map: vec![],
            worklist: Worklist::new(),
            processed: BitSet::new(),
            phis: vec![],
        }
    }

    /// Getter
    fn get(&mut self, operand: Operand, lop_typ: Option<LOpType>) -> BOperand {
        let res = match operand {
            Operand::Global(id) => self.global_map[id],
            Operand::BB(id) => self.block_map[id],
            Operand::Func(id) => self.func_map[id],
            // When getting an IR value, we get Vreg of LOp.
            Operand::Value(id) => self.value_map[id],
            Operand::Param { idx, .. } => self.param_map[idx],

            // Legalize immediats when getting 'em.
            Operand::Bool(imm) => BOperand::IntImm(imm as i32),
            Operand::Int(imm) => BOperand::IntImm(imm),
            Operand::Float(imm) => BOperand::FloatImm(imm),
            Operand::Undefined => BOperand::Undef,
        };

        let mut filter = |boperand: BOperand| -> BOperand {
            const INT_IMM_MAX: i32 = 2047;
            const INT_IMM_MIN: i32 = -2048;

            match_some! {
                target: boperand,
                enu: BOperand,
                minor_arms: {
                    BOperand::IntImm(imm) => if lop_typ.unwrap() == LOpType::Move {
                        // If force loading required, create a new LoadIntImm instruction and return the LOpId.
                        let lop_id = self.create(BOp::new(
                            Type::Int.into(),
                            vec![],
                            LOpData::LoadIntImm {
                                rd: BOperand::Undef,
                                imm,
                            }
                            .into(),
                        ));
                        self.get_rd(lop_id).expect("LoadIntImm should produce a value")
                    } else {
                        if !(INT_IMM_MIN..=INT_IMM_MAX).contains(&imm) {
                            // create a new LoadIntImm instruction and return the LOpId.
                            let lop_id = self.create(BOp::new(
                                Type::Int.into(),
                                vec![],
                                LOpData::LoadIntImm {
                                    rd: BOperand::Undef,
                                    imm,
                                }
                                .into(),
                            ));
                            self.get_rd(lop_id).expect("LoadIntImm should produce a value")
                        } else {
                            BOperand::IntImm(imm)
                        }
                    },
                    BOperand::FloatImm(imm) => {
                        // Float can never reside in immediate field of any instrucitons,
                        // So we always create a new LoadFloatImm instruction and return the LOpId.
                        let lop_id = self.create(BOp::new(
                            Type::Float.into(),
                            vec![],
                            LOpData::LoadFloatImm {
                                rd: BOperand::Undef,
                                imm: f32::from_bits(imm),
                            }
                            .into(),
                        ));
                        self.get_rd(lop_id).expect("LoadFloatImm should produce a value")
                    },
                    // Non-load ops using a mem space must load it first.
                    BOperand::Data(_)
                    | BOperand::RoData(_)
                    | BOperand::Bss(_)
                    | BOperand::Slot(_) => {
                        if lop_typ.unwrap() == LOpType::Load {
                            return boperand;
                        }

                        let typ = self.get_op_type(operand.clone());
                        let lop_id = self.create(BOp::new(
                            typ.into(),
                            vec![],
                            LOpData::Load {
                                rd: BOperand::Undef,
                                addr: boperand,
                            }
                            .into(),
                        ));
                        self.get_rd(lop_id).expect("Load should produce a value")
                    },
                    BOperand::Inst(_) => unreachable!("Inst should never be used as an operand in get()"),
                },
                uni_ops: [Undef, Reg, Func, BB, Extern],
                uni_arm: {
                    boperand
                }
            }
        };

        filter(res)
    }

    #[inline(always)]
    fn get_rd(&mut self, bop_id: BOperand) -> Option<BOperand> {
        let func_id = self.builder.current_function.expect("No current function");
        self.lower_ir.get_rd(Some(func_id), bop_id).cloned()
    }

    #[inline(always)]
    fn get_spilled_arg_offsets(&mut self, func_id: Operand, func_typ: &Type) -> Vec<BOperand> {
        let lfunc_id = self.get(func_id.clone(), None);
        self.lower_ir.funcs[lfunc_id.get_func_id()].frame_info.get_spilled_arg_offsets(lfunc_id, func_typ)
    }

    fn get_current_func(&self) -> Operand {
        let lfunc_id = self.builder.current_function.expect("No current function");

        self.func_map
            .iter()
            .enumerate()
            .find(|(_, op)| match op {
                BOperand::Func(id) => *id == lfunc_id.get_func_id(),
                _ => false,
            })
            .map(|(i, _)| Operand::Func(i))
            .expect("Current function not found in func_map")
    }

    fn get_op_type(&self, operand: Operand) -> Type {
        let current_function = self.get_current_func();

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
    fn set(&mut self, operand: Operand, value: BOperand) {
        match operand {
            Operand::Value(id) => self.value_map[id] = value,
            Operand::Global(id) => self.global_map[id] = value,
            Operand::BB(id) => self.block_map[id] = value,
            Operand::Func(id) => self.func_map[id] = value,
            Operand::Param { idx, .. } => self.param_map[idx] = value,
            Operand::Bool(_) | Operand::Int(_) | Operand::Float(_) | Operand::Undefined => (),
        }
    }

    fn init(&mut self, func_id: Operand) {
        let lfunc_id = self.get(func_id.clone(), None);
        self.builder.set_current_func(lfunc_id);

        // Clear the maps.
        self.block_map.clear();
        self.value_map.clear();
        self.param_map.clear();
        self.worklist.clear();
        self.processed.clear();
        self.phis.clear();

        // Resize the maps.
        self.block_map
            .resize(self.ir.funcs[func_id.clone()].cfg.len(), BOperand::Undef);
        self.value_map
            .resize(self.ir.funcs[func_id.clone()].dfg.len(), BOperand::Undef);
        let param_num = match &self.ir.funcs[func_id.clone()].typ {
            Type::Function { param_types, .. } => param_types.len(),
            _ => unreachable!("Only function type should be in the function arena"),
        };
        self.param_map.resize(param_num, BOperand::Undef);
    }

    // ========== Scafolding for mapping IR entities to LIR entities ==========

    #[inline(always)]
    fn alloc_and_map_func(&mut self, func_id: Operand, lfunc: BFunction) -> BOperand {
        let lfunc_id = self.lower_ir.funcs.alloc(lfunc);
        self.set(func_id, BOperand::Func(lfunc_id));
        BOperand::Func(lfunc_id)
    }

    #[inline(always)]
    fn alloc_and_map_data(
        &mut self,
        global_id: Operand,
        name: Option<String>,
        data: Data,
    ) -> BOperand {
        let data_id = match name {
            Some(name) => self.lower_ir.data_info.insert(data, name),
            None => self.lower_ir.data_info.alloc(data),
        };
        self.set(global_id, BOperand::Data(data_id));
        BOperand::Data(data_id)
    }

    #[inline(always)]
    fn alloc_and_map_rodata(
        &mut self,
        global_id: Operand,
        name: Option<String>,
        rodata: RoData,
    ) -> BOperand {
        let rodata_id = match name {
            Some(name) => self.lower_ir.rodata_info.insert(rodata, name),
            None => self.lower_ir.rodata_info.alloc(rodata),
        };
        self.set(global_id, BOperand::RoData(rodata_id));
        BOperand::RoData(rodata_id)
    }

    #[inline(always)]
    fn alloc_and_map_bss(
        &mut self,
        global_id: Operand,
        name: Option<String>,
        bss: Bss,
    ) -> BOperand {
        let bss_id = match name {
            Some(name) => self.lower_ir.bss_info.insert(bss, name),
            None => self.lower_ir.bss_info.alloc(bss),
        };
        self.set(global_id, BOperand::Bss(bss_id));
        BOperand::Bss(bss_id)
    }

    #[inline(always)]
    fn alloc_and_map_slot(&mut self, alloc_id: Operand, slot: Slot) -> BOperand {
        let func_id = self.builder.current_function.expect("No current function");

        let lfunc = &mut self.lower_ir.funcs[func_id];
        let slot_id = lfunc.frame_info.alloc(slot);
        self.set(alloc_id, BOperand::Slot(slot_id));
        BOperand::Slot(slot_id)
    }

    #[inline(always)]
    fn alloc_and_map_block(&mut self, bb_id: Operand, lbb: BBasicBlock) -> BOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lbb_id = self.lower_ir.funcs[func_id].cfg.alloc(lbb);
        self.set(bb_id, BOperand::BB(lbb_id));
        BOperand::BB(lbb_id)
    }

    /// When creating LOp which produces a value that can be mapped to IR's value, you'd better use this.
    #[inline(always)]
    fn create_and_map_lop(&mut self, op_id: Operand, lop: BOp) -> BOperand {
        let lop_id = self.create(lop);
        let lop_rd = self.get_rd(lop_id);
        if let Some(lop_rd) = lop_rd {
            self.set(op_id, lop_rd);
        }
        lop_id
    }

    #[inline(always)]
    fn create_and_map_param(&mut self, param_idx: usize, lop: BOp) -> BOperand {
        let lop_id = self.create(lop);
        let lop_rd = self.get_rd(lop_id).expect("Param should produce a value");
        self.param_map[param_idx] = lop_rd;
        lop_id
    }

    // ========== Scafolding for temporary values' mapping ========

    // ========== Atomic operations ==========

    /// When creating LOp which produces a temp value, you'd better use this.
    #[inline(always)]
    fn create(&mut self, lop: BOp) -> BOperand {
        self.builder
            .create(&mut self.lower_ir, self.builder.current_function, lop)
    }

    #[inline(always)]
    fn alloc_slot(&mut self, slot: Slot) -> BOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let slot_id = self.lower_ir.funcs[func_id].frame_info.alloc(slot);
        BOperand::Slot(slot_id)
    }

    #[inline(always)]
    fn alloc_vreg(&mut self, vreg: VirtReg) -> BOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let vreg_id = self.lower_ir.funcs[func_id].vregs.alloc(vreg);
        BOperand::Reg(Reg::Virt(vreg_id))
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
    fn lower_op(&mut self, op_id: Operand, bb_id: Operand) {
        let func_id = self.get_current_func();

        let (typ, attrs, data) = {
            let op = &self.ir.funcs[func_id.clone()].dfg[op_id.clone()];
            yachiyo::debug::info!("{:?}: Lowering op {:?}", op_id, op);
            (
                self.get_op_type(op_id.clone()),
                op.attrs.clone(),
                op.data.clone(),
            )
        };

        // Translate attrs first.
        let lattr = attrs
            .iter()
            .filter_map(|attr| match attr {
                Attr::Name(_) | Attr::FuncName(_) | Attr::GlobalArray { .. } => match attr {
                    Attr::FuncName(name) | Attr::Name(name) => Some(BAttr::Name(name.clone())),
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
                            let lhs = self.get(
                                lhs.clone(),
                                Some(LOpType::$bin_op),
                            );
                            let rhs = self.get(
                                rhs.clone(),
                                Some(LOpType::$bin_op),
                            );
                            self.create_and_map_lop(op_id.clone(), BOp::new(
                                typ.clone().into(),
                                lattr,
                                LOpData::$bin_op {
                                    rd: BOperand::Undef,
                                    lhs,
                                    rhs,
                                }
                                .into(),
                            ));
                        },
                    )*
                    $(
                        OpData::$un_op { value } => {
                            let value = self.get(
                                value.clone(),
                                Some(LOpType::$un_op),
                            );
                            self.create_and_map_lop(op_id.clone(), BOp::new(
                                typ.clone().into(),
                                lattr,
                                LOpData::$un_op {
                                    rd: BOperand::Undef,
                                    value,
                                }
                                .into(),
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
            un_ops: [Sitofp, Fptosi],
            fallback: {
                // For bool -> float, we replace it with int -> float.
                OpData::Uitofp { value } => {
                    let value = self.get(
                        value.clone(),
                        Some(LOpType::Sitofp),
                    );
                    self.create_and_map_lop(op_id.clone(), BOp::new(
                        typ.clone().into(),
                        lattr,
                        LOpData::Sitofp {
                            rd: BOperand::Undef,
                            value,
                        }
                        .into(),
                    ));
                },
                // For bool -> int, we don't genrate any instruction, just map the value.
                OpData::Zext { value } => {
                    let vreg_id = self.get(
                        value.clone(),
                        Some(LOpType::Move),
                    );
                    self.set(op_id.clone(), vreg_id);
                },
                OpData::Br {
                    cond,
                    then_bb,
                    else_bb,
                } => {
                    let cond = self.get(
                        cond.clone(),
                        Some(LOpType::Br),
                    );
                    let then_bb = self.get(then_bb.clone(), None);
                    let else_bb = self.get(else_bb.clone(), None);
                    self.create_and_map_lop(op_id.clone(),
                        BOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Br {
                                cond,
                                then_bb,
                                else_bb,
                            }
                            .into(),
                        )
                    );
                },
                OpData::Jump { target_bb } => {
                    let target_bb = self.get(target_bb.clone(), None);
                    self.create_and_map_lop(
                        op_id.clone(),
                        BOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Jump {
                                target_bb,
                            }
                            .into(),
                        ),
                    );
                },
                OpData::Load { addr } => {
                    let addr = self.get(
                        addr.clone(),
                        Some(LOpType::Load),
                    );
                    self.create_and_map_lop(
                        op_id.clone(),
                        BOp::new(
                            typ.clone().into(),
                            lattr,
                            LOpData::Load {
                                rd: BOperand::Undef,
                                addr,
                            }
                            .into(),
                        ),
                    );
                },
                OpData::Store { addr, value } => {
                    let addr = self.get(
                        addr.clone(),
                        Some(LOpType::Store),
                    );
                    let value = self.get(
                        value.clone(),
                        Some(LOpType::Store),
                    );
                    self.create_and_map_lop(
                        op_id.clone(),
                        BOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Store {
                                addr,
                                value,
                            }
                            .into(),
                        ),
                    );
                },
                OpData::Call { func, args } => {
                    // Create move instructions for args
                    let func_type = self.get_op_type(func.clone());
                    let (param_types, ret_typ) = match &func_type {
                        Type::Function { param_types, return_type } => (param_types.clone(), *return_type.clone()),
                        _ => unreachable!("Only function type can be called"),
                    };
                    let mut param_regs = Self::get_param_regs(
                        &param_types[..param_types.len().min(PARAM_REG_MAX_NUM as usize)]
                    );
                    // Get the spilled arg offsets for this call.
                    let spilled_arg_offsets = self.get_spilled_arg_offsets(func.clone(), &func_type);

                    for (idx, arg) in args.iter().enumerate() {
                        let arg_typ = self.get_op_type(arg.clone());
                        if idx < PARAM_REG_MAX_NUM as usize {
                            let arg = self.get(
                                arg.clone(),
                                Some(LOpType::Move),
                            );
                            self.create(BOp::new(
                                arg_typ.clone().into(),
                                vec![],
                                LOpData::Move {
                                    rd: BOperand::Reg(param_regs.remove(0)),
                                    src: arg,
                                }
                                .into(),
                            ));
                        } else {
                            let slot_id = spilled_arg_offsets[idx - PARAM_REG_MAX_NUM as usize];
                            let arg = self.get(
                                arg.clone(),
                                Some(LOpType::Store),
                            );
                            self.create(BOp::new(
                                Type::Void.into(),
                                vec![],
                                LOpData::Store {
                                    addr: slot_id,
                                    value: arg,
                                }
                                .into(),
                            ));
                        }
                    }

                    // Create call instruction
                    let func = self.get(func.clone(), None);
                    self.create(BOp::new(
                        // Since call doesn't produce a value in Lower IR, the type should be void.
                        Type::Void.into(),
                        vec![],
                        LOpData::Call {
                            func,
                        }
                        .into(),
                    ));

                    // If the function returns a value, we create a move from physical register.
                    if typ != Type::Void {
                        let phys_reg = match ret_typ {
                            Type::Float => Reg::F(FReg::Fa0),
                            Type::Bool | Type::Int | Type::Pointer { .. } => Reg::X(XReg::A0),
                            Type::Array { .. } | Type::Function { .. } | Type::Void | Type::Char => {
                                unreachable!("Array, Function, Void and Char type should not be directly returned")
                            }
                        };
                        self.create_and_map_lop(
                            op_id.clone(),
                            BOp::new(
                                typ.clone().into(),
                                vec![],
                                LOpData::Move {
                                    rd: BOperand::Undef,
                                    src: BOperand::Reg(phys_reg),
                                }
                                .into(),
                            ),
                        );
                    }
                }
                OpData::Phi { .. } => {
                    // Defer the processing of phis util the rest operations all have their LOp.
                    // Record the move instruction for Phi elimination later.
                    self.phis.push((op_id, bb_id));
                }
                OpData::Alloca(typ) => {
                    // For Alloca, we need to allocate stack space in the function's frame.
                    self.alloc_and_map_slot(Operand::Value(op_id.get_op_id()), Slot::Local {
                        size: typ.size(),
                        align: typ.align(),
                        offset: 0, // We will calculate the offset in the stack frame layout phase.
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
                    let mut current_lop_vreg_id = self.get(
                        base.clone(),
                        Some(LOpType::AddI),
                    );
                    for (dim, index) in indices.iter().enumerate() {
                        match &base_typ {
                            Type::Array { .. } => {
                                let index = self.get(
                                    index.clone(),
                                    Some(LOpType::MulI),
                                );
                                let mul_lop_id = self.create(BOp::new(
                                    BType::U64,
                                    vec![],
                                    LOpData::MulI {
                                        rd: BOperand::Undef,
                                        lhs: index,
                                        rhs: BOperand::IntImm(base_typ.subarr_size(dim) as i32),
                                    }
                                    .into(),
                                ));

                                // If the end of loop reached, bind the VReg of GEP to the current instruction.
                                let add_lop =
                                        BOp::new(
                                            BType::U64,
                                            vec![],
                                            LOpData::AddI {
                                                rd: BOperand::Undef,
                                                lhs: current_lop_vreg_id,
                                                rhs: self.get_rd(mul_lop_id).expect("Mul should produce a value"),
                                            }
                                            .into(),
                                        );

                                if dim == indices.len() - 1 {
                                    self.create_and_map_lop(
                                        op_id.clone(), add_lop
                                    );
                                } else {
                                    let add_lop_id = self.create(
                                        add_lop,
                                    );
                                    // Update current base address.
                                    current_lop_vreg_id = self.get_rd(add_lop_id).expect("Add should produce a value");
                                }
                            }
                            _ => {
                                let rhs = self.get(
                                    index.clone(),
                                    Some(LOpType::MulI),
                                );
                                let mul_op = self.create(
                                    BOp::new(
                                        BType::U64,
                                        vec![],
                                        LOpData::MulI {
                                            rd: BOperand::Undef,
                                            lhs: BOperand::IntImm(base_typ.size() as i32),
                                            rhs,
                                        }
                                        .into(),
                                    ),
                                );

                                // If the pointee is scalar, the iteration will only has one step.
                                // We don't need to update current_lop_id, and we can directly bind the vreg of GEP to the Add.
                                let mul_op_vreg_id = self.get_rd(mul_op).expect("Mul should produce a value");
                                self.create_and_map_lop(
                                    op_id.clone(),
                                    BOp::new(
                                        BType::U64,
                                        vec![],
                                        LOpData::AddI {
                                            rd: BOperand::Undef,
                                            lhs: current_lop_vreg_id,
                                            rhs: mul_op_vreg_id,
                                        }
                                        .into(),
                                    ),
                                );
                            }
                        }
                    }

                    // If the truncated indices is empty, we need to map the GEP to the base pointer's LOp InstId directly.
                    if indices.is_empty() {
                        let target_id = self.get(
                            base.clone(),
                            Some(LOpType::Move),
                        );
                        self.set(op_id, target_id);
                    }
                }
                OpData::Ret { value } => {
                    if let Some(value) = value {
                        let move_typ = self.get_op_type(value.clone());
                        let value = self.get(
                            value,
                            Some(LOpType::Move),
                        );
                        self.create(BOp::new(
                            move_typ.clone().into(),
                            vec![],
                            LOpData::Move {
                                rd: BOperand::Reg(match move_typ {
                                    Type::Float => Reg::F(FReg::Fa0),
                                    Type::Bool | Type::Int | Type::Pointer { .. } => Reg::X(XReg::A0),
                                    Type::Array { .. } | Type::Function { .. } | Type::Void | Type::Char => {
                                        unreachable!("Array, Function, Void and Char type should not be directly returned")
                                    }
                                }),
                                src: value,
                            }
                            .into(),
                        ));
                    }
                    // Ret itself never binds with any value.
                    self.create(
                        BOp::new(Type::Void.into(), vec![], LOpData::Ret.into()),
                    );
                }

                OpData::GlobalAlloca(_) | OpData::Declare { .. } => {
                    unreachable!("GlobalAlloca and Declare should have been handled in global lowering")
                }
            }
        }
    }

    /// Lowering the blocks in BFS order starting from the entry block.
    fn lower_bbs(&mut self, func_id: Operand) {
        while let Some(bb_id) = self.worklist.pop_front() {
            if self.processed.contains(bb_id) {
                continue;
            }
            self.processed.insert(bb_id);

            if bb_id
                == self.ir.funcs[func_id.clone()]
                    .cfg
                    .entry
                    .expect("No entry block")
            {
                let lentry = self.get(Operand::BB(bb_id), None);

                let func = &self.ir.funcs[func_id.clone()];
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
                        self.create_and_map_param(
                            idx,
                            BOp::new(
                                (*param_typ).clone().into(),
                                vec![],
                                LOpData::Move {
                                    // The rd will be filled by BBuilder::create().
                                    rd: BOperand::Undef,
                                    src: BOperand::Reg(params_reg.remove(0)),
                                }
                                .into(),
                            ),
                        );
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
                        let slot_id = self.alloc_slot(Slot::Param {
                            index: idx as u32,
                            size,
                            align,
                            offset: 0,
                        });
                        // Manually map the param to the slot.
                        self.param_map[idx] = slot_id;
                    }
                }
            }

            // The first iteration: Create Lower IR instructions
            let lbb_id = self.get(Operand::BB(bb_id), None);
            self.builder.set_current_block(lbb_id);
            let bb = &self.ir.funcs[func_id.clone()].cfg[bb_id];
            let cur = bb.cur.clone();

            // Lower the IR operations.
            for op_id in cur {
                self.lower_op(op_id, Operand::BB(bb_id));
            }

            // push successors to the worklist for later processing.
            let bb = &self.ir.funcs[func_id.clone()].cfg[bb_id];
            let succs = bb.succs.clone();
            for (succ, _) in succs {
                self.worklist.push_back(succ.get_bb_id());
            }
        }
    }

    /// Return (LOpId, BBId)
    fn resort_moves(&mut self, mut move_lop_ids: Vec<BOperand>) -> Vec<(BOperand, BOperand)> {
        // Scheduled moves.
        let mut new = vec![];
        // In degree of src in each move.
        let mut out_degree: FxHashMap<BOperand, usize> = FxHashMap::default();
        let func_id = self.builder.current_function.expect("No current function");

        // Compute out-degree of each move.
        for move_lop_id in move_lop_ids.iter() {
            let move_bop = &self.lower_ir.funcs[func_id].dfg[*move_lop_id];

            let move_lop_data: LOpData = move_bop.data.clone().into();
            match move_lop_data {
                LOpData::Move { src, .. } => {
                    *out_degree.entry(src).or_insert(0) += 1;
                }
                _ => unreachable!("Only Move LOp should be in the move_lop_ids"),
            };
        }

        // Schedule the moves
        let mut changed;
        loop {
            changed = false;
            move_lop_ids.retain(|move_lop_id| {
                let move_bop = &self.lower_ir.funcs[func_id].dfg[*move_lop_id];

                let move_lop_data: LOpData = move_bop.data.clone().into();
                // Schedule those moves whose rd's out-degree is 0.
                match move_lop_data {
                    LOpData::Move { src, rd } => {
                        if (out_degree.contains_key(&rd) && out_degree[&rd] == 0)
                            // If rd isn't in out_degree, that indicates rd is not the src of any move, we can also schedule this move too.
                            || !out_degree.contains_key(&rd)
                        {
                            new.push((
                                *move_lop_id,
                                self.builder.current_block.expect("No current block"),
                            ));
                            // Decrease the out-degree of the src of this move.
                            *out_degree.get_mut(&src).unwrap() -= 1;
                            changed = true;
                            // This item shouldn't stay in move_lop_ids
                            false
                        } else {
                            // This item should stay in move_lop_ids for the next iteration.
                            true
                        }
                    }
                    _ => unreachable!("Only Move LOp should be in the move_lop_ids"),
                }
            });

            if !changed && !move_lop_ids.is_empty() {
                // If there is a cycle, we can break it by inserting a temporary move.
                // Choose the last edge in the cycle to break.
                let move_lop_id = move_lop_ids.pop().unwrap();
                let move_bop = &self.lower_ir.funcs
                    [self.builder.current_function.expect("No current function")]
                .dfg[move_lop_id];
                let (move_lop_data, typ): (LOpData, BType) =
                    (move_bop.data.clone().into(), move_bop.typ.clone());

                match move_lop_data {
                    LOpData::Move { src, rd } => {
                        // For now we don't care where it's created, we will move 'em to trampolines later.
                        let src_temp_id = self.create(BOp::new(
                            typ.clone(),
                            vec![],
                            LOpData::Move {
                                rd: BOperand::Undef,
                                src,
                            }
                            .into(),
                        ));
                        let temp_vreg_id = self
                            .get_rd(src_temp_id)
                            .expect("Move should produce a value");

                        let temp_rd_id = self.create(BOp::new(
                            typ,
                            vec![],
                            LOpData::Move {
                                rd,
                                src: temp_vreg_id,
                            }
                            .into(),
                        ));

                        // Schedule the move from temp to rd first
                        new.push((
                            src_temp_id,
                            self.builder.current_block.expect("No current block"),
                        ));
                        *out_degree.entry(src).or_insert(0) -= 1;

                        // Update temp's out-degree
                        *out_degree.entry(temp_vreg_id).or_insert(0) += 1;
                        move_lop_ids.push(temp_rd_id);
                        // Schedule again.
                    }
                    _ => unreachable!("Only Move LOp should be in the move_lop_ids"),
                }
                // After breaking the cycle, we continue to schedule the moves in the next iteration.
            } else if move_lop_ids.is_empty() {
                break;
            }
        }

        new
    }

    fn create_trampoline(&mut self, edge: (Operand, Operand), new: Vec<(BOperand, BOperand)>) {
        let (from, to) = (self.get(edge.0, None), self.get(edge.1, None));
        let tramp_id = self
            .builder
            .create_new_block(&mut self.lower_ir, self.builder.current_function);

        let from_bb = &mut self.lower_ir.funcs
            [self.builder.current_function.expect("No current function")]
        .cfg[from];
        let from_term_id = *from_bb.cur.last().expect("No terminator in the from block");
        let from_term = &self.lower_ir.funcs
            [self.builder.current_function.expect("No current function")]
        .dfg[from_term_id];

        let from_term_data = match from_term.data.clone() {
            BOpData::L(l_op) => l_op,
            BOpData::M(_) => unreachable!("Only LOp can be the terminator of a block"),
        };

        // Update the control flow
        let new_lop = match from_term_data {
            LOpData::Br {
                cond,
                then_bb,
                else_bb,
            } => {
                if then_bb == to {
                    BOp::new(
                        Type::Void.into(),
                        vec![],
                        LOpData::Br {
                            cond,
                            then_bb: tramp_id,
                            else_bb,
                        }
                        .into(),
                    )
                } else if else_bb == to {
                    BOp::new(
                        Type::Void.into(),
                        vec![],
                        LOpData::Br {
                            cond,
                            then_bb,
                            else_bb: tramp_id,
                        }
                        .into(),
                    )
                } else {
                    unreachable!(
                        "The edge to be replaced should be in the terminator of the from block"
                    )
                }
            }
            LOpData::Jump { target_bb } => {
                if target_bb == to {
                    BOp::new(
                        Type::Void.into(),
                        vec![],
                        LOpData::Jump {
                            target_bb: tramp_id,
                        }
                        .into(),
                    )
                } else {
                    unreachable!(
                        "The edge to be replaced should be in the terminator of the from block"
                    )
                }
            }
            _ => unreachable!(
                "The terminator of the from block should be either Br or Jump: {:?}",
                from_term_data
            ),
        };

        let current_function = self.builder.current_function;
        self.lower_ir.replace_op_rauw(
            &mut self.builder,
            current_function,
            from_term_id,
            from,
            new_lop,
        );

        // Insert terminator and the moves for phi elimination.
        {
            let mut guard = BBuilderGuard::new(&mut self.builder);
            guard.set_current_block(tramp_id);

            // Move the moves to the trampoline block's end.
            for (move_lop_id, move_bb_id) in new {
                self.lower_ir.move_op_to_bb_at(
                    current_function,
                    move_lop_id,
                    move_bb_id,
                    tramp_id,
                    None,
                );
            }

            let jump_lop = BOp::new(
                Type::Void.into(),
                vec![],
                LOpData::Jump { target_bb: to }.into(),
            );
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
                        let (typ, values) = match values {
                            Some(values) => {
                                let typ = match typ { 
                                    Type::Bool => Type::Int,
                                    Type::Array {..}
                                    | Type::Int | Type::Float | Type::Pointer { .. } => typ,
                                    Type::Function { .. }
                                    | Type::Void
                                    | Type::Char => unreachable!("Function, Void and Char type should not be in the global array"),
                                };

                                (typ, Some(values.iter().map(|v| match v {
                                    Literal::Int(i) => BOperand::IntImm(*i),
                                    Literal::Float(f) => BOperand::FloatImm(f.to_bits()),
                                    Literal::String(s) => unimplemented!(
                                        "String literal in global array initializer is not supported yet: {}",
                                        s
                                    ),
                                }).collect()))
                            }
                            // If global array has no initializer, we move it to .bss
                            None => match &typ {
                                Type::Bool => (Type::Int, None),
                                Type::Int | Type::Array { .. } | Type::Float => (typ, None),
                                Type::Pointer { .. } => unimplemented!(
                                    "Uninitialized global pointer is not supported yet"
                                ),
                                Type::Function { .. } | Type::Void | Type::Char => {
                                    unreachable!("Function type should not be in the global array")
                                }
                            },
                        };

                        if mutable {
                            if let Some(values) = values {
                                // For initialized mutable global, we allocate it in .data section.
                                let data = Data::new(typ, values);
                                self.alloc_and_map_data(Operand::Global(global), Some(name), data);
                            } else {
                                // For uninitialized mutable global, we allocate it in .bss section.
                                let bss = Bss::new(typ);
                                self.alloc_and_map_bss(Operand::Global(global), Some(name), bss);
                            }
                        } else {
                            if let Some(values) = values {
                                // For initialized immutable global, we allocate it in .rodata section.
                                let rodata = RoData::new(typ, values);
                                self.alloc_and_map_rodata(
                                    Operand::Global(global),
                                    Some(name),
                                    rodata,
                                );
                            } else {
                                // For uninitialized immutable global, we fill its init values manually and allocate it in .rodata section.
                                let values = match &typ {
                                    Type::Int | Type::Bool => {
                                        vec![BOperand::IntImm(0)]
                                    }
                                    Type::Float => {
                                        vec![BOperand::FloatImm(0.0f32.to_bits())]
                                    }
                                    Type::Pointer { .. } => unimplemented!(
                                        "Uninitialized global pointer is not supported yet"
                                    ),
                                    Type::Array { base, dims } => {
                                        let base_value = match &**base {
                                        Type::Int
                                        | Type::Bool => BOperand::IntImm(0),
                                        Type::Float => BOperand::FloatImm(0.0f32.to_bits()),
                                        Type::Pointer { .. } => unimplemented!("Uninitialized global pointer is not supported yet"),
                                        Type::Array { .. } => unimplemented!("Multi-dimensional array without initializer is not supported yet"),
                                        Type::Function { .. } | Type::Void | Type::Char => unreachable!("Function, Void and Char type should not be in the global array"),
                                    };
                                        vec![base_value; dims.iter().product::<u32>() as usize]
                                    }
                                    Type::Function { .. } | Type::Void | Type::Char => {
                                        unreachable!(
                                            "Function type should not be in the global array"
                                        )
                                    }
                                };
                                let rodata = RoData::new(typ, values);
                                self.alloc_and_map_rodata(
                                    Operand::Global(global),
                                    Some(name),
                                    rodata,
                                );
                            }
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
            let name = func.name.clone();
            if func.is_external {
                // Simply map external functions to Extern operand with the function name.
                self.set(Operand::Func(func_id), BOperand::Extern(String::leak(name)));
            } else {
                self.alloc_and_map_func(Operand::Func(func_id), BFunction::new(name));
            }
        }
    }

    pub fn run(&mut self) -> BackIR {
        self.func_map.resize(self.ir.funcs.len(), BOperand::Undef);
        self.global_map
            .resize(self.ir.globals.len(), BOperand::Undef);
        self.lower_global();

        for func_id in self.ir.funcs.collect_internal() {
            let func_id = Operand::Func(func_id);
            self.init(func_id.clone());

            // Pre-allocate basic blocks.
            let func = &self.ir.funcs[func_id.clone()];
            let entry = func.cfg.entry.expect("No entry block");
            for bb_id in func.cfg.ids() {
                self.alloc_and_map_block(Operand::BB(bb_id), BBasicBlock::default());
            }

            // Since phis can produce back edges,
            // We need to pre-allocate a VReg for all Phi instructions and bind them with vregs.
            self.ir
                .get_all_ops(Some(func_id.clone()), OpType::Phi)
                .into_iter()
                .for_each(|phi_id| {
                    let typ = self.get_op_type(phi_id.clone());
                    let phi_vreg_id = self.alloc_vreg(VirtReg::new(typ.into()));
                    // Phi is eliminated, without BOp mapped to Phi. So we map it to vreg directly.
                    self.set(phi_id, phi_vreg_id);
                });

            self.worklist.push_back(entry);
            self.lower_bbs(func_id.clone());

            // Process phis.
            let mut phi_moves: FxHashMap<(Operand, Operand), Vec<BOperand>> = FxHashMap::default();
            for (phi_id, phi_bb_id) in std::mem::take(&mut self.phis) {
                let (typ, phi_op_data) = {
                    let op = &self.ir.funcs[func_id.clone()].dfg[phi_id.clone()];
                    (self.get_op_type(phi_id.clone()), op.data.clone())
                };

                if let OpData::Phi { incomings } = phi_op_data {
                    for incoming in incomings {
                        let (value, bb_id) = match incoming {
                            PhiIncoming::Data {
                                value: Operand::Undefined,
                                ..
                            }
                            | PhiIncoming::None => {
                                // If the incoming value is undefined, we can simply skip it since it won't be used in the later codegen.
                                continue;
                            }
                            PhiIncoming::Data { value, bb } => (value, bb),
                        };

                        // Set current block
                        let lfunc_id = self.get(func_id.clone(), None);
                        let lbb_id = self.get(bb_id.clone(), None);
                        self.builder.set_current_block(lbb_id);

                        // Set current inst, or potential LoadIntImm will be generated at the end of BB.
                        let lterm_id = {
                            let bb = &self.lower_ir.funcs[lfunc_id].cfg[lbb_id];
                            *bb.cur.last().expect("No terminator in the block")
                        };
                        self.builder.set_current_inst(lterm_id);

                        let incoming_vreg_id = self.get(value.clone(), Some(LOpType::Move));

                        let move_lop = BOp::new(
                            typ.clone().into(),
                            vec![BAttr::PhiMove],
                            LOpData::Move {
                                rd: self.get(phi_id.clone(), Some(LOpType::Move)),
                                src: incoming_vreg_id,
                            }
                            .into(),
                        );

                        // The moves will be binded to the same VReg allocated to Phi instruction previously.
                        // For now we don't care where it's created, we will move 'em to trampolines later.
                        let move_lop_id = self.create_and_map_lop(phi_id.clone(), move_lop);
                        // Record the move_lop_id for later resorting and trampoline insertion.
                        phi_moves
                            .entry((bb_id, phi_bb_id.clone()))
                            .or_default()
                            .push(move_lop_id);
                    }
                } else {
                    unreachable!("Only Phi should be in the phis map");
                }
            }

            // Refinement: reschedule the Moves generated by Phis and create trampolines.
            for edge in phi_moves.keys().cloned() {
                // Set current block
                let lfunc_id = self.get(func_id.clone(), None);
                let bb_id = edge.0.clone();
                let lbb_id = self.get(bb_id, None);
                self.builder.set_current_block(lbb_id);

                // Set current inst
                let lterm_id = {
                    let bb = &self.lower_ir.funcs[lfunc_id].cfg[lbb_id];
                    *bb.cur.last().expect("No terminator in the block")
                };
                self.builder.set_current_inst(lterm_id);

                let move_lop_ids = phi_moves[&edge].clone();
                let resorted_moves = self.resort_moves(move_lop_ids);
                self.create_trampoline(edge, resorted_moves);
            }
        }

        std::mem::take(&mut self.lower_ir)
    }
}
