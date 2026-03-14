//! IR Lowering from Mid IR to Lower IR.

use super::config::PARAM_REG_MAX_NUM;
use crate::base::Type;
use crate::frontend::ast::Literal;
use crate::ir::lower::*;
use crate::ir::machine::{Data, FReg, MType, Reg, Slot, XReg};
use crate::ir::mid::*;

use rustc_hash::FxHashMap;

/// TODO: In lowering we need to do:
/// 1. Lower the GEP
/// 2. Lower the function call (handle the argument passing and return value passing according to the calling convention)
/// 3. Phi Elimination (SSA to non-SSA)
/// 4.
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
    /// IR OpId -> VirtId.
    value_map: Vec<LOperand>,
    /// Param Idx -> SlotId/VirtId
    param_map: Vec<LOperand>,

    /// Move instruction buffer for Phi
    /// Edge(BBId, BBId) -> Move InstId.
    phi_moves: FxHashMap<(usize, usize), Vec<LOperand>>,
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
            phi_moves: FxHashMap::default(),
        }
    }

    /// Getter
    fn get(&self, operand: Operand) -> LOperand {
        match operand {
            Operand::Global(id) => self.global_map[id].clone(),
            Operand::BB(id) => self.block_map[id].clone(),
            Operand::Func(id) => self.func_map[id].clone(),
            Operand::Value(id) => self.value_map[id].clone(),
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

    #[inline(always)]
    fn alloc_and_map_vreg(&mut self, op_id: Operand, phys: Option<Reg>) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lfunc = &mut self.lower_ir.funcs[func_id];
        let vreg_id = lfunc.vregs.alloc(VirtReg {
            inst_id: LOperand::Undef,
            phys,
        });
        let vreg_op = LOperand::Virt(vreg_id);
        self.set(op_id, vreg_op.clone());
        vreg_op
    }

    #[inline(always)]
    fn alloc_vreg(&mut self, op_id: Operand, phys: Option<Reg>) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lfunc = &mut self.lower_ir.funcs[func_id];
        let vreg_id = lfunc.vregs.alloc(VirtReg {
            inst_id: LOperand::Undef,
            phys,
        });
        let vreg_op = LOperand::Virt(vreg_id);
        self.set(op_id, vreg_op.clone());
        vreg_op
    }

    #[inline(always)]
    fn create_and_bind_vreg_by_op_id(&mut self, op_id: Operand, lop: LOp) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");

        let lop_id = self
            .builder
            .create(&mut self.lower_ir, self.builder.current_function, lop);
        // Bind LOp to VReg.
        let vreg_id = self.get(op_id.clone());
        // Bind vreg with lop.
        let lop = &mut self.lower_ir.funcs[func_id].dfg[lop_id.clone()];
        lop.vreg = vreg_id.clone();
        let vreg = &mut self.lower_ir.funcs[func_id].vregs[vreg_id.clone()];
        vreg.inst_id = lop_id;

        vreg_id
    }

    #[inline(always)]
    fn param_alloc_slot(&mut self, typ: Type, idx: usize) -> LOperand {
        let func_id = self.builder.current_function.expect("No current function");
        let lfunc = &mut self.lower_ir.funcs[func_id];
        let slot_id = lfunc.frame_info.alloc(Slot::new(typ));
        self.param_map[idx] = LOperand::Slot(slot_id);
        LOperand::Slot(slot_id)
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

    fn op_to_lop(&self, op: &Op) -> LOp {
        let lop_data = self.op_data_to_lop_data(&op.data);
        let lattr = op.attrs
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
        LOp::new(op.typ.clone().into(), lattr, lop_data)
    }

    fn op_data_to_lop_data(&self, op_data: &OpData) -> LOpData {
        match op_data {
            OpData::AddI { lhs, rhs } => LOpData::AddI {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::SubI { lhs, rhs } => LOpData::SubI {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::MulI { lhs, rhs } => LOpData::MulI {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::DivI { lhs, rhs } => LOpData::DivI {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::ModI { lhs, rhs } => LOpData::ModI {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::SNe { lhs, rhs } => LOpData::SNe {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::SEq { lhs, rhs } => LOpData::SEq {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::SGt { lhs, rhs } => LOpData::SGt {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::SLt { lhs, rhs } => LOpData::SLt {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::SGe { lhs, rhs } => LOpData::SGe {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::SLe { lhs, rhs } => LOpData::SLe {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::Xor { lhs, rhs } => LOpData::Xor {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::Shl { lhs, rhs } => LOpData::Shl {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::Shr { lhs, rhs } => LOpData::Shr {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::Sar { lhs, rhs } => LOpData::Sar {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::AddF { lhs, rhs } => LOpData::AddF {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::SubF { lhs, rhs } => LOpData::SubF {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::MulF { lhs, rhs } => LOpData::MulF {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::DivF { lhs, rhs } => LOpData::DivF {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::ONe { lhs, rhs } => LOpData::ONe {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::OEq { lhs, rhs } => LOpData::OEq {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::OGt { lhs, rhs } => LOpData::OGt {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::OLt { lhs, rhs } => LOpData::OLt {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::OGe { lhs, rhs } => LOpData::OGe {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::OLe { lhs, rhs } => LOpData::OLe {
                lhs: self.get(lhs.clone()),
                rhs: self.get(rhs.clone()),
            },
            OpData::Sitofp { value } => LOpData::Sitofp {
                value: self.get(value.clone()),
            },
            OpData::Fptosi { value } => LOpData::Fptosi {
                value: self.get(value.clone()),
            },
            OpData::Uitofp { value } => LOpData::Uitofp {
                value: self.get(value.clone()),
            },
            OpData::Zext { value } => LOpData::Zext {
                value: self.get(value.clone()),
            },
            OpData::Br {
                cond,
                then_bb,
                else_bb,
            } => LOpData::Br {
                cond: self.get(cond.clone()),
                then_bb: self.get(then_bb.clone()),
                else_bb: self.get(else_bb.clone()),
            },
            OpData::Jump { target_bb } => LOpData::Jump {
                target_bb: self.get(target_bb.clone()),
            },
            OpData::Load { addr } => LOpData::Load {
                addr: self.get(addr.clone()),
            },
            OpData::Store { addr, value } => LOpData::Store {
                addr: self.get(addr.clone()),
                value: self.get(value.clone()),
            },
            _ => unreachable!(
                "Unsupported OpData for direct translation, should be handled separately: {:?}",
                op_data
            ),
        }
    }

    fn lower(&mut self) {
        let func = &self.ir.funcs[self.builder.current_function.expect("No current function")];
        let lfunc_typ = match &func.typ {
            Type::Function {
                param_types,
                return_type,
            } => MType::Function {
                return_type: Box::new((**return_type).clone().into()),
                param_types: param_types.iter().map(|t| (*t).clone().into()).collect(),
            },
            _ => unreachable!("Only function type should be in the function arena"),
        };

        self.lower_ir
            .funcs
            .alloc(LFunction::new(func.name.clone(), lfunc_typ));

        // Create prologue.
        let lentry = self.alloc_and_map_block(
            Operand::BB(func.cfg.entry.expect("No entry block")),
            LBasicBlock::new(),
        );

        let func = &self.ir.funcs[self.builder.current_function.expect("No current function")];
        let param_types = match &func.typ {
            Type::Function { param_types, .. } => param_types.clone(),
            _ => unreachable!("Only function type should be in the function arena"),
        };

        self.builder.set_current_block(lentry);
        let mut params_reg =
            Self::get_param_regs(&param_types[..param_types.len().min(PARAM_REG_MAX_NUM as usize)]);

        let func_id = self.builder.current_function.expect("No current function");
        for (idx, param_typ) in param_types.iter().enumerate() {
            if idx < PARAM_REG_MAX_NUM as usize {
                let func_id = self.builder.current_function.expect("No current function");
                let vreg_id = self.lower_ir.funcs[func_id].vregs.alloc(VirtReg {
                    inst_id: LOperand::Undef,
                    phys: Some(params_reg.remove(0)),
                });
                let lop = LOp::new(
                    (*param_typ).clone().into(),
                    vec![],
                    LOpData::Move {
                        src: LOperand::Virt(vreg_id),
                    },
                );
                let lop_id =
                    self.builder
                        .create(&mut self.lower_ir, self.builder.current_function, lop);
                self.lower_ir.funcs[func_id].vregs[vreg_id].inst_id = lop_id;
                self.param_map[idx] = LOperand::Virt(vreg_id);
            } else {
                self.param_alloc_slot(param_typ.clone(), idx);
            }
        }

        // Pre-allocate basic blocks.
        let func = &self.ir.funcs[func_id];
        let entry = func.cfg.entry.expect("No entry block");
        for bb_id in func.cfg.collect() {
            // Skip the entry block since it's already allocated.
            if bb_id == entry {
                continue;
            }
            let lbb_id = self.alloc_and_map_block(Operand::BB(bb_id), LBasicBlock::new());

            self.builder.set_current_block(lbb_id);
            let bb = &self.ir.funcs[func_id].cfg[bb_id];
            let cur = bb.cur.clone();

            // Pre-allocate Instructions.
            for op_id in cur {
                let op = &self.ir.funcs[func_id].dfg[op_id.clone()];
                // If the instruction has a return value, pre-allocate a VReg for it.
                if op.typ != Type::Void {
                    let phys = match op.data {
                        // Allocate fixed physical register for Call.
                        OpData::Call { .. } => Some(match op.typ {
                            Type::Float => Reg::F(FReg::Fa0),
                            Type::Bool | Type::Int | Type::Pointer { .. } => Reg::X(XReg::A0),
                            Type::Array { .. }
                            | Type::Function { .. }
                            | Type::Void
                            | Type::Char => {
                                unreachable!("Array, Function, Void and Char type should not be directly returned")
                            }
                        }),
                        _ => None,
                    };
                    self.alloc_vreg(op_id, phys);
                }
            }
        }

        // The second iteration: Create Lower IR instructions
        for (bb_id, lbb_id) in self.block_map.clone().into_iter().enumerate() {
            self.builder.set_current_block(lbb_id);
            let bb = &self.ir.funcs[func_id].cfg[bb_id];
            let cur = bb.cur.clone();

            for op_id in cur {
                let op_data = {
                    let op = &self.ir.funcs[func_id].dfg[op_id.clone()];
                    op.data.clone()
                };
                match op_data {
                    OpData::Call { func, args } => {
                        // Create move instructions for args
                        for param_reg in args.iter() {
                            let move_lop = LOp::new(
                                Type::Int.into(),
                                vec![],
                                LOpData::Move {
                                    src: self.get(param_reg.clone()),
                                },
                            );
                            self.create_and_bind_vreg_by_op_id(op_id.clone(), move_lop);
                        }
                        // Create call instruction
                        let op = &self.ir.funcs[func_id].dfg[op_id.clone()];
                        self.create_and_bind_vreg_by_op_id(
                            op_id.clone(),
                            LOp::new(
                                op.typ.clone().into(),
                                vec![],
                                LOpData::Call {
                                    func: self.get(func),
                                },
                            ),
                        );
                    }
                    OpData::Phi { incomings } => {
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
                                    src: incoming_vreg_id,
                                },
                            );
                            // The moves will be binded to the same VReg allocated to Phi instruction previously.
                            let move_lop_id =
                                self.create_and_bind_vreg_by_op_id(op_id.clone(), move_lop);
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
                            _ => unreachable!("Only Value and Global can be the base of GEP"),
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
                                    let mul_vreg_id =
                                        self.lower_ir.funcs[func_id].vregs.alloc(VirtReg {
                                            inst_id: LOperand::Undef,
                                            phys: None,
                                        });
                                    let mut mul_lop = LOp::new(
                                        MType::U64,
                                        vec![],
                                        LOpData::MulI {
                                            lhs: self.get(index.clone()),
                                            rhs: LOperand::IntImm(base_typ.subarr_size(dim) as i32),
                                        },
                                    );
                                    mul_lop.vreg = LOperand::Virt(mul_vreg_id);

                                    let mul_lop_id = self.builder.create(
                                        &mut self.lower_ir,
                                        self.builder.current_function,
                                        mul_lop,
                                    );
                                    self.lower_ir.funcs[func_id].vregs[mul_vreg_id].inst_id =
                                        mul_lop_id.clone();

                                    let add_op = self.builder.create(
                                        &mut self.lower_ir,
                                        self.builder.current_function,
                                        LOp::new(
                                            MType::U64,
                                            vec![],
                                            LOpData::AddI {
                                                lhs: current_lop_id.clone(),
                                                rhs: mul_lop_id.clone(),
                                            },
                                        ),
                                    );

                                    // Update current base address.
                                    current_lop_id = add_op;
                                    // If the end of loop reached, bind the VReg of GEP to the current instruction.
                                    if dim == indices.len() - 1 {
                                        self.lower_ir.funcs[func_id].vregs[mul_vreg_id].inst_id =
                                            current_lop_id.clone();
                                    }
                                }
                                _ => {
                                    let mul_op = self.create_and_bind_vreg_by_op_id(
                                        op_id.clone(),
                                        LOp::new(
                                            MType::U64,
                                            vec![],
                                            LOpData::MulI {
                                                lhs: LOperand::IntImm(
                                                    base_typ.size_in_bytes() as i32
                                                ),
                                                rhs: self.get(index.clone()),
                                            },
                                        ),
                                    );
                                    // If the pointee is scalar, the iteration will only has one step.
                                    // We don't need to update current_lop_id, and we can directly bind the vreg of GEP to the Add.
                                    self.create_and_bind_vreg_by_op_id(
                                        op_id.clone(),
                                        LOp::new(
                                            MType::U64,
                                            vec![],
                                            LOpData::AddI {
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
                            let value_op = self.get(value.clone());
                            let typ = &self.lower_ir.funcs[func_id].dfg[value_op].typ;

                            let move_lop = LOp::new(
                                typ.clone(),
                                vec![],
                                LOpData::Move {
                                    src: self.get(value.clone()),
                                },
                            );
                            self.create_and_bind_vreg_by_op_id(op_id.clone(), move_lop);
                        }
                        self.create_and_bind_vreg_by_op_id(
                            op_id.clone(),
                            LOp::new(Type::Void.into(), vec![], LOpData::Ret),
                        );
                    }
                    _ => {
                        let op = &self.ir.funcs[func_id].dfg[op_id.clone()];
                        let lop = self.op_to_lop(op);
                        self.create_and_bind_vreg_by_op_id(op_id, lop);
                    }
                }
            }

            // The third iteration: Reschedule the Moves generated by Phis.
            for edge in self.phi_moves.keys().cloned().collect::<Vec<_>>() {
                let move_lop_ids = self.phi_moves[&edge].clone();
                let resorted_moves = self.resort_moves(move_lop_ids);
                self.create_trampoline(edge, resorted_moves);
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
                LOpData::Move { src } => {
                    if let LOperand::Virt(_) = src {
                        (src.get_virt_id(), move_lop.vreg.get_virt_id())
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
                    LOpData::Move { src } => {
                        if let LOperand::Virt(_) = src {
                            (src.get_virt_id(), move_lop.vreg.get_virt_id())
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
                let (from, _) = *edges.iter().next().unwrap();
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
                    if let LOpData::Move { src } = move_lop.data.clone() {
                        if src == LOperand::Virt(from) {
                            match &mut move_lop.data {
                                LOpData::Move { src } => {
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
            self.lower();
        }

        std::mem::take(&mut self.lower_ir)
    }
}
