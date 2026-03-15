//! IR Lowering from Mid IR to Lower IR.

use super::config::PARAM_REG_MAX_NUM;
use crate::base::Type;
use crate::frontend::ast::Literal;
use crate::ir::lower::*;
use crate::ir::machine::{Data, FReg, MType, Reg, Slot, XReg};
use crate::ir::mid::*;
use crate::utils::bitset::BitSet;
use crate::utils::worklist::*;

use rustc_hash::FxHashMap;

/// TODO: In lowering we need to do:
/// 1. Lower the GEP
/// 2. Lower the function call (handle the argument passing and return value passing according to the calling convention)
/// 3. Phi Elimination (SSA to non-SSA)
///
/// And Lowering should not has any ISA-specific transfromation except ABI adaptation.
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
    /// Edge(BBId, BBId) -> Move InstId.
    phi_moves: FxHashMap<(usize, usize), Vec<LOperand>>,
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
            phi_moves: FxHashMap::default(),
        }
    }

    /// Getter
    fn get(&self, operand: Operand) -> LOperand {
        match operand {
            Operand::Global(id) => self.global_map[id].clone(),
            Operand::BB(id) => self.block_map[id].clone(),
            Operand::Func(id) => self.func_map[id].clone(),
            // When getting an IR value, we get Vreg of LOp.
            Operand::Value(id) => {
                let lop_id = self.value_map[id].clone();
                let lop = &self.lower_ir.funcs
                    [self.builder.current_function.expect("No current function")]
                .dfg[lop_id.clone()];

                match_rd! {
                    target: &lop.data,
                    op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Uitofp, Zext, Load, Move],
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

            Operand::Bool(imm) => LOperand::IntImm(imm as i32),
            Operand::Int(imm) => LOperand::IntImm(imm),
            Operand::Float(imm) => LOperand::FloatImm(imm),
            Operand::Undefined => LOperand::Undef,
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
        self.func_map.clear();
        self.global_map.clear();
        self.value_map.clear();
        self.param_map.clear();
        self.worklist.clear();
        self.processed.clear();
        self.phi_moves.clear();

        // Resize the maps.
        self.func_map
            .resize(self.ir.funcs.collect().len(), LOperand::Undef);
        self.global_map
            .resize(self.ir.globals.collect().len(), LOperand::Undef);
        self.block_map
            .resize(self.ir.funcs[idx].cfg.collect().len(), LOperand::Undef);
        self.value_map
            .resize(self.ir.funcs[idx].dfg.collect().len(), LOperand::Undef);
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
    fn alloc_and_map_data(&mut self, global_id: Operand, typ: Type) -> LOperand {
        let data_id = self.lower_ir.data_info.alloc(Data::new(typ));
        self.set(global_id, LOperand::Data(data_id));
        LOperand::Data(data_id)
    }

    #[inline(always)]
    fn alloc_and_map_slot(&mut self, alloc_id: Operand, slot: Slot) -> LOperand {
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

    fn bind(&mut self, lop_id: LOperand, vreg_id: LOperand) {
        let data = &mut self.lower_ir.funcs
            [self.builder.current_function.expect("No current function")]
        .dfg[lop_id.clone()]
        .data;

        match_rd! {
            target: data,
            op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Uitofp, Zext, Load, Move],
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
        vreg.inst_id = lop_id;
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

    /// TODO: Might be replaced by rewriting system.
    fn lower_op(&mut self, op_id: Operand) {
        let func_id = self.builder.current_function.expect("No current function");
        let (typ, attrs, data) = {
            let op = &self.ir.funcs[func_id].dfg[op_id.clone()];
            (op.typ.clone(), op.attrs.clone(), op.data.clone())
        };

        // Translate attrs first.
        let lattr = attrs
            .iter()
            .filter_map(|attr| match attr {
                Attr::Name(_)
                | Attr::FuncName(_)
                | Attr::GlobalArray { .. } => {
                    match attr {
                        Attr::FuncName(name)
                        | Attr::Name(name) => Some(LAttr::Name(name.clone())),
                        Attr::GlobalArray {
                            name,
                            mutable,
                            typ,
                            values,
                        } => Some(LAttr::GlobalArray {
                            name: name.clone(),
                            mutable: *mutable,
                            typ: typ.clone(),
                            values: values.as_ref().map(|vals| vals.iter().map(|v| match v {
                                Literal::Int(i) => LOperand::IntImm(*i),
                                Literal::Float(f) => LOperand::FloatImm(*f),
                                Literal::String(_) => unreachable!("String literal should not be in the global array initializer"),
                            }).collect()),
                        }),
                        Attr::OldIdx(_)
                        | Attr::Promotion => None,
                    }
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
                            self.alloc_and_map_lop(op_id.clone(), LOp::new(
                                typ.clone().into(),
                                lattr,
                                LOpData::$bin_op {
                                    rd: LOperand::Undef,
                                    lhs: self.get(lhs.clone()),
                                    rhs: self.get(rhs.clone()),
                                },
                            ));
                        },
                    )*
                    $(
                        OpData::$un_op { value } => {
                            self.alloc_and_map_lop(op_id.clone(), LOp::new(
                                typ.clone().into(),
                                lattr,
                                LOpData::$un_op {
                                    rd: LOperand::Undef,
                                    value: self.get(value.clone()),
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
                    self.alloc_and_map_lop(op_id.clone(),
                        LOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Br {
                                cond: self.get(cond.clone()),
                                then_bb: self.get(then_bb.clone()),
                                else_bb: self.get(else_bb.clone()),
                            }
                        )
                    );
                },
                OpData::Jump { target_bb } => {
                    self.alloc_and_map_lop(
                        op_id.clone(),
                        LOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Jump {
                                target_bb: self.get(target_bb.clone()),
                            },
                        ),
                    );
                },
                OpData::Load { addr } => {
                    self.alloc_and_map_lop(
                        op_id.clone(),
                        LOp::new(
                            typ.clone().into(),
                            lattr,
                            LOpData::Load {
                                rd: LOperand::Undef,
                                addr: self.get(addr.clone()),
                            },
                        ),
                    );
                },
                OpData::Store { addr, value } => {
                    self.alloc_and_map_lop(
                        op_id.clone(),
                        LOp::new(
                            Type::Void.into(),
                            lattr,
                            LOpData::Store {
                                addr: self.get(addr.clone()),
                                value: self.get(value.clone()),
                            },
                        ),
                    );
                },
                OpData::Call { func, args } => {
                    // Create move instructions for args
                    let mut param_regs = Self::get_param_regs(match &self.ir.funcs[func_id].typ {
                        Type::Function { param_types, .. } => param_types,
                        _ => unreachable!("Only function type should be in the function arena"),
                    });
                    for (idx, arg) in args.iter().enumerate() {
                        let arg_typ = self.ir.funcs[func_id].dfg[arg.clone()].typ.clone();
                        if idx < PARAM_REG_MAX_NUM as usize {
                            let vreg_id = self.alloc_vreg(VirtReg {
                                inst_id: LOperand::Undef,
                                phys: Some(param_regs.remove(0)),
                            });
                            let move_lop_id = self.create(LOp::new(
                                arg_typ.clone().into(),
                                vec![],
                                LOpData::Move {
                                    rd: LOperand::Undef,
                                    src: self.get(arg.clone()),
                                },
                            ));
                            self.bind(move_lop_id, vreg_id);
                        } else {
                            let slot_id = self.alloc_slot(Slot::Param {
                                size: arg_typ.size_in_bytes(),
                                align: arg_typ.align_in_bytes(),
                            });
                            self.create(LOp::new(
                                Type::Void.into(),
                                vec![],
                                LOpData::Store {
                                    addr: slot_id.clone(),
                                    value: self.get(arg.clone()),
                                },
                            ));
                        }
                    }
                    // Create call instruction

                    self.create(LOp::new(
                        // Since call doesn't produce a value in Lower IR, the type should be void.
                        Type::Void.into(),
                        vec![],
                        LOpData::Call {
                            func: self.get(func.clone()),
                        },
                    ));

                    // If the function returns a value, we create a move and bind the original VReg.
                    if typ != Type::Void {
                        let vreg_id = self.alloc_vreg(VirtReg {
                            inst_id: LOperand::Undef,
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
                OpData::Phi { incomings } => {
                    // TODO: Handle the case when phi's incoming are not created yet.
                    // For Phi, we need to create move instructions for each incoming edge.
                    for incoming in incomings {
                        let (value, bb_id) = match incoming {
                            PhiIncoming::Data { value, bb } => (value, bb),
                            PhiIncoming::None => continue,
                        };
                        let incoming_vreg_id = self.get(value.clone());
                        let op = &self.ir.funcs[func_id].dfg[op_id.clone()];
                        let move_lop = LOp::new(
                            op.typ.clone().into(),
                            vec![],
                            LOpData::Move {
                                rd: LOperand::Undef,
                                src: incoming_vreg_id,
                            },
                        );
                        // The moves will be binded to the same VReg allocated to Phi instruction previously.
                        let move_lop_id = self.alloc_and_map_lop(op_id.clone(), move_lop);
                        // Record the move instruction for Phi elimination later.
                        let edge = (bb_id.get_bb_id(), op_id.get_op_id());
                        self.phi_moves.entry(edge).or_default().push(move_lop_id);
                    }
                }
                OpData::Alloca(typ) => {
                    // For Alloca, we need to allocate stack space in the function's frame.
                    self.alloc_and_map_slot(Operand::Value(op_id.get_op_id()), Slot::new(typ));
                }
                OpData::GEP { base, indices } => {
                    // GEP is only used for array in SysY.
                    let typ = match base {
                        Operand::Value(id) => {
                            let value = &self.ir.funcs[func_id].dfg[id];
                            value.typ.clone()
                        }
                        Operand::Global(id) => {
                            let global_op = &self.ir.globals[id];
                            global_op.typ.clone()
                        }
                        _ => {
                            unreachable!("Only Value and Global can be the base of GEP")
                        }
                    };
                    let base_typ = match &typ {
                        Type::Pointer { base } => (**base).clone(),
                        _ => unreachable!("Only array type can be the base of GEP"),
                    };

                    // Initialize the current base address with the base pointer.
                    let mut current_lop_id = self.get(base.clone());
                    for (dim, index) in indices.iter().enumerate() {
                        match &base_typ {
                            Type::Array { .. } => {
                                let mul_vreg_id = self.alloc_vreg(VirtReg {
                                    inst_id: LOperand::Undef,
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
                                    let add_lop_id = self.create(
                                        add_lop,
                                    );

                                    // Update current base address.
                                    current_lop_id = add_lop_id;
                                } else {
                                    self.alloc_and_map_lop(
                                        op_id.clone(), add_lop
                                    );
                                }
                            }
                            _ => {
                                let mul_op = self.create(
                                    LOp::new(
                                        MType::U64,
                                        vec![],
                                        LOpData::MulI {
                                            rd: LOperand::Undef,
                                            lhs: LOperand::IntImm(base_typ.size_in_bytes() as i32),
                                            rhs: self.get(index.clone()),
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
                }
                OpData::Ret { value } => {
                    if let Some(value) = value {
                        self.create(LOp::new(
                            typ.into(),
                            vec![],
                            LOpData::Move {
                                rd: LOperand::Undef,
                                src: self.get(value.clone()),
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
                            inst_id: LOperand::Undef,
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
                            Type::Int | Type::Float => {
                                (param_typ.size_in_bytes(), param_typ.align_in_bytes())
                            }
                            Type::Pointer { .. } => {
                                (param_typ.size_in_bytes(), param_typ.align_in_bytes())
                            }
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
                    inst_id: LOperand::Undef,
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
        for global in self.ir.globals.collect() {
            let global_op = &self.ir.globals[global];
            match global_op.data.clone() {
                OpData::GlobalAlloca(typ) => {
                    self.alloc_and_map_data(Operand::Global(global), typ);
                }
                OpData::Declare { .. } => { /*Ignore it*/ }
                _ => unreachable!("Only global alloca and declare should be in the global arena"),
            }
        }
    }

    pub fn run(&mut self) -> LowerIR {
        self.lower_global();

        // Pre-allocate functions.
        for func_id in self.ir.funcs.collect_internal() {
            self.init(func_id);

            // Pre-allocate basic blocks.
            let func = &self.ir.funcs[func_id];
            let entry = func.cfg.entry.expect("No entry block");
            for bb_id in func.cfg.collect() {
                self.alloc_and_map_block(Operand::BB(bb_id), LBasicBlock::new());
            }

            self.worklist.push_back(entry);
            self.lower_bbs();

            // Refinement: reschedule the Moves generated by Phis and create trampolines.
            for edge in self.phi_moves.keys().cloned().collect::<Vec<_>>() {
                let move_lop_ids = self.phi_moves[&edge].clone();
                let resorted_moves = self.resort_moves(move_lop_ids);
                self.create_trampoline(edge, resorted_moves);
            }
        }

        std::mem::take(&mut self.lower_ir)
    }
}
