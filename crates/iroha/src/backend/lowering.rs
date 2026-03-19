//! IR Lowering from Mid IR to Lower IR.

use yachiyo::ast::Literal;
use yachiyo::base::Type;
use yachiyo::config::PARAM_REG_MAX_NUM;
use yachiyo::ir::back::*;
use yachiyo::ir::mid::*;
use yachiyo::utils::bitset::BitSet;
use yachiyo::utils::r#match::match_minor;
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

    fn legalize_imm(&mut self, imm: BOperand) -> BOperand {
        const INT_IMM_MAX: i32 = 2047;
        const INT_IMM_MIN: i32 = -2048;

        match_minor! {
            target: imm,
            minor_arms: {
                BOperand::IntImm(imm) => {
                    if !(INT_IMM_MIN..=INT_IMM_MAX).contains(&imm) {
                        // create a new LoadIntImm instruction and return the LOpId.
                        let vreg_id = self.alloc_vreg(VirtReg::default());
                        let lop_id = self.create(BOp::new(
                            Type::Int.into(),
                            vec![],
                            LOpData::LoadIntImm {
                                rd: BOperand::Undef,
                                imm,
                            }
                            .into(),
                        ));
                        self.bind(lop_id, vreg_id.clone());
                        vreg_id
                    } else {
                        BOperand::IntImm(imm)
                    }
                },
                BOperand::FloatImm(imm) => {
                    // Float can never reside in immediate field of any instrucitons,
                    // So we always create a new LoadFloatImm instruction and return the LOpId.
                    let vreg_id = self.alloc_vreg(VirtReg::default());
                    let lop_id = self.create(BOp::new(
                        Type::Float.into(),
                        vec![],
                        LOpData::LoadFloatImm {
                            rd: BOperand::Undef,
                            imm,
                        }
                        .into(),
                    ));
                    self.bind(lop_id, vreg_id.clone());
                    vreg_id
                }
            },
            uni_ops: [BOperand::Undef, BOperand::Reg, BOperand::Func, BOperand::BB, BOperand::Inst, BOperand::Data, BOperand::Slot, BOperand::RoData],
            other_patterns: [],
            uni_arm: {
                unreachable!("Only IntImm and FloatImm can be immediats, but got {:?}", imm)
            }
        }
    }

    /// Getter
    fn get(&mut self, operand: Operand) -> BOperand {
        yachiyo::debug::info!("get {:?}", operand);
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
                    BOperand::Data(_) | BOperand::Slot(_) | BOperand::RoData(_)
                ) {
                    return lop_id;
                }

                let current_function = self.builder.current_function.expect("No current function");
                let bop = &self.lower_ir.funcs[current_function].dfg[lop_id.clone()];
                let lop_data = match &bop.data {
                    BOpData::L(l_op) => l_op,
                    BOpData::M(_) => unreachable!("MOp should not be mapped to IR value"),
                };

                match_rd! {
                    target: lop_data,
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
                        | LOpData::Ret => unreachable!("Only LOp with rd field can be mapped to IR value, but got {:?}", lop_data),
                    }
                }
            }
            Operand::Param { idx, .. } => self.param_map[idx].clone(),

            // Legalize immediats when getting 'em.
            Operand::Bool(imm) => self.legalize_imm(BOperand::IntImm(imm as i32)),
            Operand::Int(imm) => self.legalize_imm(BOperand::IntImm(imm)),
            Operand::Float(imm) => self.legalize_imm(BOperand::FloatImm(imm)),
            Operand::Undefined => BOperand::Undef,
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
            .resize(self.ir.funcs[idx].cfg.len(), BOperand::Undef);
        self.value_map
            .resize(self.ir.funcs[idx].dfg.len(), BOperand::Undef);
        let param_num = match &self.ir.funcs[idx].typ {
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
        self.set(global_id, BOperand::Data(rodata_id));
        BOperand::Data(rodata_id)
    }

    #[inline(always)]
    fn alloc_and_map_slot(&mut self, alloc_id: Operand, slot: Slot) -> BOperand {
        yachiyo::debug::info!("Map alloc {:?} to slot {:?}]", alloc_id, slot);
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
    fn alloc_and_map_lop(&mut self, op_id: Operand, lop: BOp) -> BOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lop_id = self.lower_ir.funcs[func_id].dfg.alloc(lop);
        self.set(op_id, BOperand::Inst(lop_id));
        BOperand::Inst(lop_id)
    }

    // ========== Scafolding for temporary values' mapping ========

    // ========== Atomic operations ==========

    /// When creating LOp which produces a temp value, you'd better use this.
    #[inline(always)]
    fn create(&mut self, lop: BOp) -> BOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lop_id = self.lower_ir.funcs[func_id].dfg.alloc(lop);
        BOperand::Inst(lop_id)
    }

    #[inline(always)]
    fn alloc_vreg(&mut self, vreg: VirtReg) -> BOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let vreg_id = self.lower_ir.funcs[func_id].vregs.alloc(vreg);
        BOperand::Reg(Reg::Virt(vreg_id))
    }

    #[inline(always)]
    fn alloc_slot(&mut self, slot: Slot) -> BOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let slot_id = self.lower_ir.funcs[func_id].frame_info.alloc(slot);
        BOperand::Slot(slot_id)
    }

    fn bind(&mut self, lop_id: BOperand, reg: BOperand) {
        let data = &mut self.lower_ir.funcs
            [self.builder.current_function.expect("No current function")]
        .dfg[lop_id.clone()]
        .data;
        let lop_data = match data {
            BOpData::L(l_op) => l_op,
            BOpData::M(_) => unreachable!("MOp should not be mapped to IR value"),
        };

        match_rd! {
            target: lop_data,
            op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Uitofp, Zext, Load, Move, LoadFloatImm, LoadIntImm],
            rd_arm: LOpData(rd) => {
                *rd = reg.clone();
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

        if matches!(reg, BOperand::Reg(Reg::Virt(_))) {
            let vreg = &mut self.lower_ir.funcs
                [self.builder.current_function.expect("No current function")]
            .vregs[reg];
            vreg.defs.push(lop_id);
        }
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
        yachiyo::debug::info!("Lowering {:?} {:?}", op_id, data);

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
                            let lhs = self.get(lhs.clone());
                            let rhs = self.get(rhs.clone());
                            self.alloc_and_map_lop(op_id.clone(), BOp::new(
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
                            let value = self.get(value.clone());
                            self.alloc_and_map_lop(op_id.clone(), BOp::new(
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
                    let target_bb = self.get(target_bb.clone());
                    self.alloc_and_map_lop(
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
                    let addr = self.get(addr.clone());
                    let vreg_id = self.alloc_vreg(VirtReg::default());
                    let lop_id = self.alloc_and_map_lop(
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
                    self.bind(lop_id, vreg_id);
                },
                OpData::Store { addr, value } => {
                    let addr = self.get(addr.clone());
                    let value = self.get(value.clone());
                    self.alloc_and_map_lop(
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
                            let phys_reg = param_regs.remove(0);
                            let arg = self.get(arg.clone());
                            let move_lop_id = self.create(BOp::new(
                                arg_typ.clone().into(),
                                vec![],
                                LOpData::Move {
                                    rd: BOperand::Undef,
                                    src: arg,
                                }
                                .into(),
                            ));
                            // Bind to physical register directly.
                            self.bind(move_lop_id, BOperand::Reg(phys_reg));
                        } else {
                            let slot_id = self.alloc_slot(Slot::Arg {
                                size: arg_typ.size(),
                                align: arg_typ.align(),
                                offset: 0, // We will calculate the offset in the stack frame layout phase.
                            });
                            let arg = self.get(arg.clone());
                            self.create(BOp::new(
                                Type::Void.into(),
                                vec![],
                                LOpData::Store {
                                    addr: slot_id.clone(),
                                    value: arg,
                                }
                                .into(),
                            ));
                        }
                    }
                    // Create call instruction

                    let func = self.get(func.clone());
                    self.create(BOp::new(
                        // Since call doesn't produce a value in Lower IR, the type should be void.
                        Type::Void.into(),
                        vec![],
                        LOpData::Call {
                            func,
                        }
                        .into(),
                    ));

                    // If the function returns a value, we create a move and bind the original VReg.
                    if typ != Type::Void {
                        let vreg_id = self.alloc_vreg(VirtReg::default());
                        let move_lop_id = self.alloc_and_map_lop(
                            op_id.clone(),
                            BOp::new(
                                typ.clone().into(),
                                vec![],
                                LOpData::Move {
                                    rd: BOperand::Undef,
                                    src: vreg_id.clone(),
                                }
                                .into(),
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
                    let mut current_lop_id = self.get(base.clone());
                    for (dim, index) in indices.iter().enumerate() {
                        match &base_typ {
                            Type::Array { .. } => {
                                let mul_vreg_id = self.alloc_vreg(VirtReg::default());

                                let mul_lop = BOp::new(
                                    BType::U64,
                                    vec![],
                                    LOpData::MulI {
                                        rd: mul_vreg_id,
                                        lhs: self.get(index.clone()),
                                        rhs: BOperand::IntImm(base_typ.subarr_size(dim) as i32),
                                    }
                                    .into(),
                                );

                                let mul_lop_id = self.create(
                                    mul_lop,
                                );

                                // If the end of loop reached, bind the VReg of GEP to the current instruction.
                                let add_lop =
                                        BOp::new(
                                            BType::U64,
                                            vec![],
                                            LOpData::AddI {
                                                rd: BOperand::Undef,
                                                lhs: current_lop_id.clone(),
                                                rhs: mul_lop_id.clone(),
                                            }
                                            .into(),
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
                                self.alloc_and_map_lop(
                                    op_id.clone(),
                                    BOp::new(
                                        BType::U64,
                                        vec![],
                                        LOpData::AddI {
                                            rd: BOperand::Undef,
                                            lhs: current_lop_id.clone(),
                                            rhs: mul_op,
                                        }
                                        .into(),
                                    ),
                                );
                            }
                        }
                    }

                    // If the truncated indices is empty, we need to map the GEP to the base pointer's LOp InstId directly.
                    if indices.is_empty() {
                        yachiyo::debug::info!("current_op_id: {:?}", current_lop_id);
                        let target_id = match_minor!(
                            target: current_lop_id,
                            minor_arms: {
                                BOperand::Reg(Reg::Virt(id)) => self.lower_ir.funcs[func_id].vregs[id].defs[0].clone(),
                                BOperand::Reg(_) => unreachable!("Only VirtReg can be the source of GEP, but got physical register"),
                            },
                            uni_ops: [BOperand::Data, BOperand::Slot, BOperand::BB, BOperand::Func, BOperand::Inst, BOperand::Undef, BOperand::IntImm, BOperand::FloatImm, BOperand::RoData],
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
                        self.create(BOp::new(
                            typ.into(),
                            vec![],
                            LOpData::Move {
                                rd: BOperand::Undef,
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
                    BFunction::new(func.name.clone()),
                );

                // Create prologue.
                let func = &self.ir.funcs[func_id];
                let lentry = self.alloc_and_map_block(
                    Operand::BB(func.cfg.entry.expect("No entry block")),
                    BBasicBlock::default(),
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
                        let vreg_id = self.alloc_vreg(VirtReg::default());

                        let lop_id = self.create(BOp::new(
                            (*param_typ).clone().into(),
                            vec![],
                            LOpData::Move {
                                // The rd will be filled by BBuilder::create().
                                rd: BOperand::Undef,
                                src: BOperand::Reg(params_reg.remove(0)),
                            }
                            .into(),
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
                        let slot_id = self.alloc_slot(Slot::Param {
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

    fn resort_moves(&mut self, mut move_lop_ids: Vec<BOperand>) -> Vec<BOperand> {
        let mut new = vec![];
        let mut edges: Vec<(usize, usize)> = vec![];

        // Compute in-degree of each move.
        for move_lop_id in move_lop_ids.iter_mut() {
            let move_bop = &self.lower_ir.funcs
                [self.builder.current_function.expect("No current function")]
            .dfg[move_lop_id.clone()];

            let move_lop_data = match move_bop.data.clone() {
                BOpData::L(l_op) => l_op,
                BOpData::M(_) => unreachable!("Only LOp should be in the move_lop_ids"),
            };
            let edge = match move_lop_data {
                LOpData::Move { src, rd } => {
                    if let (BOperand::Reg(Reg::Virt(src_id)), BOperand::Reg(Reg::Virt(rd_id))) =
                        (src.clone(), rd.clone())
                    {
                        (src_id, rd_id)
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
                let move_bop = &self.lower_ir.funcs
                    [self.builder.current_function.expect("No current function")]
                .dfg[move_lop_id.clone()];

                let move_lop_data = match move_bop.data.clone() {
                    BOpData::L(l_op) => l_op,
                    BOpData::M(_) => unreachable!("Only LOp should be in the move_lop_ids"),
                };
                let edge = match move_lop_data {
                    LOpData::Move { src, rd } => {
                        if let (BOperand::Reg(Reg::Virt(src_id)), BOperand::Reg(Reg::Virt(rd_id))) =
                            (src.clone(), rd.clone())
                        {
                            (src_id, rd_id)
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
                .alloc(VirtReg::default());
                let temp_lop_id = self.builder.create(
                    &mut self.lower_ir,
                    self.builder.current_function,
                    BOp::new(
                        BType::U64,
                        vec![],
                        LOpData::Move {
                            rd: BOperand::Reg(Reg::Virt(temp_vreg_id)),
                            src: BOperand::Reg(Reg::Virt(from)),
                        }
                        .into(),
                    ),
                );
                // We don't need to add the new move to edges.
                new.push(temp_lop_id.clone());
                // Replace the move from `from` to `temp`.
                for move_lop_id in move_lop_ids.iter_mut() {
                    let move_bop = &mut self.lower_ir.funcs
                        [self.builder.current_function.expect("No current function")]
                    .dfg[move_lop_id.clone()];
                    let move_lop_data = match move_bop.data.clone() {
                        BOpData::L(l_op) => l_op,
                        BOpData::M(_) => unreachable!("Only LOp should be in the move_lop_ids"),
                    };
                    if let LOpData::Move { src, .. } = move_lop_data {
                        if src == BOperand::Reg(Reg::Virt(from)) {
                            match &mut move_bop.data {
                                BOpData::L(LOpData::Move { src, .. }) => {
                                    *src = BOperand::Reg(Reg::Virt(temp_vreg_id));
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

    fn create_trampoline(&mut self, edge: (usize, usize), new: Vec<BOperand>) {
        let (from, to) = (BOperand::BB(edge.0), BOperand::BB(edge.1));
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
                            then_bb: tramp_id.clone(),
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
                            else_bb: tramp_id.clone(),
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
                            target_bb: tramp_id.clone(),
                        }
                        .into(),
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
            let mut guard = BBuilderGuard::new(&mut self.builder);
            guard.set_current_block(tramp_id.clone());

            self.lower_ir.funcs[current_function.expect("No current function")].cfg
                [tramp_id.clone()]
            .cur
            .extend(new);

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
                        let values = match values {
                            Some(values) => values.iter().map(|v| match v {
                                Literal::Int(i) => BOperand::IntImm(*i),
                                Literal::Float(f) => BOperand::FloatImm(*f),
                                Literal::String(s) => unimplemented!(
                                    "String literal in global array initializer is not supported yet: {}",
                                    s
                                ),
                            }).collect(),
                            // If global array has no initializer, we need to fill it with default values according to the type.
                            None => match &typ {
                                Type::Int
                                | Type::Bool => vec![BOperand::IntImm(0)],
                                Type::Float => vec![BOperand::FloatImm(0.0)],
                                Type::Pointer { .. } => unimplemented!("Uninitialized global pointer is not supported yet"),
                                Type::Array { base, dims } => {
                                    let base_value = match &**base {
                                        Type::Int
                                        | Type::Bool => BOperand::IntImm(0),
                                        Type::Float => BOperand::FloatImm(0.0),
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
            self.alloc_and_map_func(Operand::Func(func_id), BFunction::new(func.name.clone()));
        }
    }

    pub fn run(&mut self) -> BackIR {
        self.func_map.resize(self.ir.funcs.len(), BOperand::Undef);
        self.global_map
            .resize(self.ir.globals.len(), BOperand::Undef);
        self.lower_global();

        // Pre-allocate functions.
        for func_id in self.ir.funcs.collect_internal() {
            self.init(func_id);

            // Pre-allocate basic blocks.
            let func = &self.ir.funcs[func_id];
            let entry = func.cfg.entry.expect("No entry block");
            for bb_id in func.cfg.ids() {
                self.alloc_and_map_block(Operand::BB(bb_id), BBasicBlock::default());
            }

            self.worklist.push_back(entry);
            self.lower_bbs();

            // Process phis.
            let mut phi_moves: FxHashMap<(usize, usize), Vec<BOperand>> = FxHashMap::default();
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
                        let move_lop = BOp::new(
                            typ.clone().into(),
                            vec![],
                            LOpData::Move {
                                rd: BOperand::Undef,
                                src: incoming_vreg_id,
                            }
                            .into(),
                        );

                        // The moves will be binded to the same VReg allocated to Phi instruction previously.
                        let move_lop_id = self.alloc_and_map_lop(Operand::Value(phi_id), move_lop);
                        // Record the move_lop_id for later resorting and trampoline insertion.
                        phi_moves
                            .entry((bb_id.get_bb_id(), phi_bb_id))
                            .or_default()
                            .push(move_lop_id);
                    }
                } else {
                    unreachable!("Only Phi should be in the phis map");
                }
            }

            // Refinement: reschedule the Moves generated by Phis and create trampolines.
            for edge in phi_moves.keys().cloned() {
                let move_lop_ids = phi_moves[&edge].clone();
                let resorted_moves = self.resort_moves(move_lop_ids);
                self.create_trampoline(edge, resorted_moves);
            }
        }

        std::mem::take(&mut self.lower_ir)
    }
}
