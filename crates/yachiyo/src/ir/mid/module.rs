//! Module definition of IR.

use crate::base::Type;
use crate::ir::mid::{
  BasicBlock, Builder, BuilderGuard, Function, Op, OpData, OpType, Operand, PhiIncoming, CG, DFG,
};
use crate::utils::{match_some, Arena, ArenaItem};

use std::ops::{Deref, DerefMut, Index, IndexMut};

#[derive(Debug, Clone, Default)]
pub struct Globals(DFG);

impl Deref for Globals {
  type Target = DFG;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for Globals {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

impl Index<Operand> for Globals {
  type Output = Op;

  fn index(&self, index: Operand) -> &Self::Output {
    match index {
      Operand::Global(index) => &self.0[index],
      _ => panic!("Globals index: expected a global operand, got {:?}", index),
    }
  }
}

impl IndexMut<Operand> for Globals {
  fn index_mut(&mut self, index: Operand) -> &mut Self::Output {
    match index {
      Operand::Global(index) => &mut self.0[index],
      _ => panic!(
        "Globals index_mut: expected a global operand, got {:?}",
        index
      ),
    }
  }
}

impl Index<usize> for Globals {
  type Output = Op;

  fn index(&self, index: usize) -> &Self::Output {
    &self.0[index]
  }
}

impl IndexMut<usize> for Globals {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.0[index]
  }
}

impl Globals {
  /// Replace an operand use inside a global op without maintaining use-def lists.
  pub fn replace_use(&mut self, op_tuple: (Operand, usize), old: Operand, new: Operand) -> bool {
    let (op_id, operand_idx) = op_tuple;
    let op_id = match op_id {
      Operand::Global(op_id) => op_id,
      _ => return false,
    };

    let src_tuples = DFG::match_src_tuple_mut(&mut self.0[op_id].data);
    let mut replaced = false;
    for (src, idx) in src_tuples {
      if *src == old && idx == operand_idx {
        *src = new;
        replaced = true;
      }
    }
    replaced
  }
}

/// GlobalId -> (UserId, operand_idx)
#[derive(Default, Debug, Clone)]
pub struct GlobalUses(Vec<Vec<(Operand, usize)>>);

impl Index<Operand> for GlobalUses {
  type Output = Vec<(Operand, usize)>;

  fn index(&self, index: Operand) -> &Self::Output {
    match index {
      Operand::Global(index) => &self.0[index],
      _ => panic!(
        "GlobalUses index: expected a global operand, got {:?}",
        index
      ),
    }
  }
}

impl IndexMut<Operand> for GlobalUses {
  fn index_mut(&mut self, index: Operand) -> &mut Self::Output {
    match index {
      Operand::Global(index) => &mut self.0[index],
      _ => panic!(
        "GlobalUses index_mut: expected a global operand, got {:?}",
        index
      ),
    }
  }
}

impl GlobalUses {
  fn ensure_global(&mut self, global: Operand) {
    let Operand::Global(global_id) = global else {
      return;
    };

    if global_id >= self.0.len() {
      self.0.resize_with(global_id + 1, Vec::new);
    }
  }

  pub fn add_use(&mut self, global: Operand, user_tuple: (Operand, usize)) {
    let Operand::Global(_) = global else {
      return;
    };

    self.ensure_global(global);
    self[global].push(user_tuple);
  }

  pub fn remove_use(&mut self, global: Operand, user_tuple: (Operand, usize)) {
    let Operand::Global(_) = global else {
      return;
    };

    self.ensure_global(global);
    let users = &mut self[global];
    if let Some(pos) = users.iter().position(|x| *x == user_tuple) {
      users.swap_remove(pos);
    } else {
      panic!(
        "Use {:?}: not found in users of global {:?}",
        user_tuple, global
      );
    }
  }

  pub fn users(&self, global: Operand) -> &[(Operand, usize)] {
    let Operand::Global(global_id) = global else {
      return &[];
    };

    self.0.get(global_id).map(Vec::as_slice).unwrap_or(&[])
  }

  pub fn users_mut(&mut self, global: Operand) -> &mut Vec<(Operand, usize)> {
    let Operand::Global(_) = global else {
      panic!(
        "GlobalUses users_mut: expected Global operand, got {:?}",
        global
      );
    };

    self.ensure_global(global);
    &mut self[global]
  }
}

#[derive(Debug, Clone, Default)]
pub struct FuncsGlobalUses(Vec<GlobalUses>);

impl Index<Operand> for FuncsGlobalUses {
  type Output = GlobalUses;

  fn index(&self, index: Operand) -> &Self::Output {
    match index {
      Operand::Func(index) => &self.0[index],
      _ => panic!(
        "FuncsGlobalUses index: expected a function operand, got {:?}",
        index
      ),
    }
  }
}

impl IndexMut<Operand> for FuncsGlobalUses {
  fn index_mut(&mut self, index: Operand) -> &mut Self::Output {
    match index {
      Operand::Func(index) => &mut self.0[index],
      _ => panic!(
        "FuncsGlobalUses index_mut: expected a function operand, got {:?}",
        index
      ),
    }
  }
}

impl FuncsGlobalUses {
  fn ensure_func(&mut self, func: Operand) {
    let Operand::Func(func_id) = func else {
      return;
    };

    if func_id >= self.0.len() {
      self.0.resize_with(func_id + 1, GlobalUses::default);
    }
  }

  pub fn add_use(&mut self, func: Operand, global: Operand, user_tuple: (Operand, usize)) {
    self.ensure_func(func);
    self[func].add_use(global, user_tuple);
  }

  pub fn remove_use(&mut self, func: Operand, global: Operand, user_tuple: (Operand, usize)) {
    self.ensure_func(func);
    self[func].remove_use(global, user_tuple);
  }

  pub fn users(&self, func: Option<Operand>, global: Operand) -> &[(Operand, usize)] {
    match func {
      Some(Operand::Func(func_id)) => self
        .0
        .get(func_id)
        .map(|global_uses| global_uses.users(global))
        .unwrap_or(&[]),
      Some(func) => panic!(
        "FuncsGlobalUses users: expected a function operand, got {:?}",
        func
      ),
      None => panic!("FuncsGlobalUses users: cannot borrow users across all functions"),
    }
  }

  pub fn users_mut(&mut self, func: Operand, global: Operand) -> &mut Vec<(Operand, usize)> {
    let Operand::Func(_) = func else {
      panic!(
        "FuncsGlobalUses users_mut: expected a function operand, got {:?}",
        func
      );
    };

    self.ensure_func(func);
    self[func].users_mut(global)
  }

  pub fn has_users(&self, func: Option<Operand>, global: Operand) -> bool {
    match func {
      Some(_) => !self.users(func, global).is_empty(),
      None => self
        .0
        .iter()
        .any(|global_uses| !global_uses.users(global).is_empty()),
    }
  }

  pub fn clear_func(&mut self, func: Operand) {
    let Operand::Func(func_id) = func else {
      return;
    };

    if func_id < self.0.len() {
      self.0[func_id] = GlobalUses::default();
    }
  }
}

#[derive(Debug, Clone)]
pub struct IR {
  /// Including:
  /// 1. global variables
  /// 2. SysY library functions
  pub globals: Globals,
  /// FuncId -> GlobalId -> (UserId, operand_idx)
  global_uses: FuncsGlobalUses,
  /// global funcs
  pub funcs: CG,
}

impl IR {
  pub fn new() -> Self {
    Self {
      globals: Globals::default(),
      global_uses: FuncsGlobalUses::default(),
      funcs: CG::new(),
    }
  }

  pub fn add_uses(&mut self, current_function: Option<Operand>, op: Operand) {
    let src_tuples = self
      .get_src_tuple(current_function, op)
      .into_iter()
      .map(|(src, idx)| (*src, idx))
      .collect::<Vec<(Operand, usize)>>();
    for (src, idx) in src_tuples {
      self.add_use(current_function, src, (op, idx));
    }
  }

  pub fn remove_uses(&mut self, current_function: Option<Operand>, op: Operand) {
    let src_tuples = self
      .get_src_tuple(current_function, op)
      .into_iter()
      .map(|(src, idx)| (*src, idx))
      .collect::<Vec<(Operand, usize)>>();
    for (src, idx) in src_tuples {
      self.remove_use(current_function, src, (op, idx));
    }
  }

  pub fn add_use(
    &mut self,
    current_function: Option<Operand>,
    used: Operand,
    user_tuple: (Operand, usize),
  ) {
    match used {
      Operand::Value(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function].dfg.add_use(used, user_tuple);
      }
      Operand::Param(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function]
          .params
          .add_use(used, user_tuple);
      }
      Operand::Global(_) => {
        let current_function = current_function
          .unwrap_or_else(|| panic!("IR add_use: global use without current function"));
        self.global_uses.add_use(current_function, used, user_tuple);
      }
      Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Undefined
      | Operand::Func(_)
      | Operand::BB(_) => {}
    }
  }

  pub fn remove_use(
    &mut self,
    current_function: Option<Operand>,
    used: Operand,
    user_tuple: (Operand, usize),
  ) {
    match used {
      Operand::Value(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function]
          .dfg
          .remove_use(used, user_tuple);
      }
      Operand::Param(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function]
          .params
          .remove_use(used, user_tuple);
      }
      Operand::Global(_) => {
        let current_function = current_function
          .unwrap_or_else(|| panic!("IR remove_use: global use without current function"));
        self
          .global_uses
          .remove_use(current_function, used, user_tuple);
      }
      Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Undefined
      | Operand::Func(_)
      | Operand::BB(_) => {}
    }
  }

  pub fn replace_use(
    &mut self,
    current_function: Option<Operand>,
    op_tuple: (Operand, usize),
    old: Operand,
    new: Operand,
  ) {
    let (user, operand_idx) = op_tuple;
    let replaced = match user {
      Operand::Value(_) => {
        let current_function = current_function.unwrap();
        let src_tuples = self.funcs[current_function].get_src_tuple_mut(user);
        let mut replaced = false;
        for (src, idx) in src_tuples {
          if *src == old && idx == operand_idx {
            *src = new;
            replaced = true;
          }
        }
        replaced
      }
      Operand::Global(_) => self.globals.replace_use(op_tuple, old, new),
      _ => false,
    };

    if !replaced {
      panic!(
        "IR replace_use: use tuple ({:?}, {}) for {:?} not found",
        user, operand_idx, old
      );
    }

    self.remove_use(current_function, old, op_tuple);
    self.add_use(current_function, new, op_tuple);
  }

  pub fn users(&self, current_function: Option<Operand>, operand: Operand) -> &[(Operand, usize)] {
    match operand {
      Operand::Value(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function].dfg.users(operand)
      }
      Operand::Param(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function].params.users(operand)
      }
      Operand::Global(_) => self.global_uses.users(current_function, operand),
      Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Undefined
      | Operand::Func(_)
      | Operand::BB(_) => &[],
    }
  }

  pub fn users_mut(
    &mut self,
    current_function: Option<Operand>,
    operand: Operand,
  ) -> &mut Vec<(Operand, usize)> {
    match operand {
      Operand::Value(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function].dfg.users_mut(operand)
      }
      Operand::Param(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function].params.users_mut(operand)
      }
      Operand::Global(_) => {
        let current_function = current_function
          .unwrap_or_else(|| panic!("IR users_mut: global users without current function"));
        self.global_uses.users_mut(current_function, operand)
      }
      Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Undefined
      | Operand::Func(_)
      | Operand::BB(_) => panic!("IR users_mut: operand {:?} does not have users", operand),
    }
  }

  pub fn has_users(&self, current_function: Option<Operand>, operand: Operand) -> bool {
    match operand {
      Operand::Global(_) => self.global_uses.has_users(current_function, operand),
      Operand::Value(_) | Operand::Param(_) => !self.users(current_function, operand).is_empty(),
      Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Undefined
      | Operand::Func(_)
      | Operand::BB(_) => false,
    }
  }

  pub fn clear_uses(&mut self, current_function: Option<Operand>) {
    let current_function = current_function.unwrap();
    let func = &mut self.funcs[current_function];
    for item in func.dfg.storage.iter_mut() {
      if let ArenaItem::Data(op) = item {
        op.users.clear();
      }
    }
    func.params.clear_uses();
    self.global_uses.clear_func(current_function);
  }

  pub fn replace_some_uses(
    &mut self,
    current_function: Option<Operand>,
    old: Operand,
    new: Operand,
    user_list: Vec<(Operand, usize)>,
  ) {
    let users = self.users(current_function, old).to_vec();
    for user in user_list {
      if !users.contains(&user) {
        panic!(
          "IR replace_some_uses: user {:?} not found in users of operand {:?}",
          user, old
        );
      }
      self.replace_use(current_function, user, old, new);
    }
  }

  pub fn replace_all_uses(
    &mut self,
    current_function: Option<Operand>,
    old: Operand,
    new: Operand,
  ) {
    if current_function.is_none() {
      match old {
        Operand::Global(_) => {
          let users_by_func = self
            .global_uses
            .0
            .iter()
            .enumerate()
            .filter_map(|(func_id, global_uses)| {
              let users = global_uses.users(old).to_vec();
              (!users.is_empty()).then_some((Operand::Func(func_id), users))
            })
            .collect::<Vec<_>>();

          for (func_id, users) in users_by_func {
            for use_op in users {
              self.replace_use(Some(func_id), use_op, old, new);
            }
          }
        }
        Operand::Int(_)
        | Operand::Float(_)
        | Operand::Bool(_)
        | Operand::Undefined
        | Operand::Func(_)
        | Operand::BB(_) => {}
        Operand::Value(_) | Operand::Param(_) => {
          panic!(
            "IR replace_all_uses: function-local operand {:?} requires current function",
            old
          );
        }
      }
      return;
    }

    let users = self.users(current_function, old).to_vec();
    for use_op in users {
      self.replace_use(current_function, use_op, old, new);
    }
  }

  pub fn add_control_flow(&mut self, current_function: Option<Operand>, op: Operand, bb: Operand) {
    let current_function = current_function.unwrap();
    let func = &mut self.funcs[current_function];
    let (cfg, dfg) = (&mut func.cfg, &mut func.dfg);
    let data = dfg[op.get_op_id()].data.clone();

    match_some! {
        target: data,
        enu: OpData,
        minor_arms: {
            OpData::Br {
                then_bb, else_bb, ..
            } => {
                cfg.add_pred(then_bb, (bb, op));
                cfg.add_succ(bb, (then_bb, op));

                cfg.add_pred(else_bb, (bb, op));
                cfg.add_succ(bb, (else_bb, op));
            }
            OpData::Jump { target_bb } => {
                cfg.add_pred(target_bb, (bb, op));
                cfg.add_succ(bb, (target_bb, op));
            }
        },
        uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, GlobalAlloca, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Declare],
        uni_arm: {}
    }
  }

  pub fn remove_control_flow(&mut self, current_function: Option<Operand>, op: Operand) {
    let current_function = current_function.unwrap();
    let bb = self.funcs[current_function].op_to_bb[op];
    assert!(
      matches!(bb, Operand::BB(_)),
      "IR remove_control_flow: instruction {:?} is not mapped to a block",
      op
    );
    let func = &mut self.funcs[current_function];
    let (cfg, dfg) = (&mut func.cfg, &mut func.dfg);
    let data = dfg[op.get_op_id()].data.clone();

    match_some! {
        target: data,
        enu: OpData,
        minor_arms: {
            OpData::Br {
                then_bb, else_bb, ..
            } => {
                cfg.remove_pred(then_bb, (bb, op));
                cfg.remove_succ(bb, (then_bb, op));
                cfg.remove_pred(else_bb, (bb, op));
                cfg.remove_succ(bb, (else_bb, op));
            }
            OpData::Jump { target_bb } => {
                cfg.remove_pred(target_bb, (bb, op));
                cfg.remove_succ(bb, (target_bb, op));
            }
        },
        uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, GlobalAlloca, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Declare],
        uni_arm: {}
    }
  }

  pub fn create(
    &mut self,
    builder: &Builder,
    current_function: Option<Operand>,
    op: Op,
  ) -> Operand {
    match_some! {
        target: op.data,
        enu: OpData,
        minor_arms: {
            OpData::GlobalAlloca(_) | OpData::Declare { .. } => {
                let op_id = self.globals.alloc(op);
                Operand::Global(op_id)
            }
        },
        uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Jump, Br],
        uni_arm: {
          let current_function =
            current_function.unwrap();
          let current_block = builder
            .current_block
            .unwrap_or_else(|| panic!("IR create: current_block is None"));
          let op_id = {
            let func = &mut self.funcs[current_function];
            let (cfg, dfg) = (&mut func.cfg, &mut func.dfg);

            let new_id = dfg.alloc(op);
            let current_block_id = current_block.get_bb_id();
            let bb = &mut cfg[current_block_id];

            let op_id = if let Some(current_inst) = &builder.current_inst {
              let pos = bb
                .cur
                .iter()
                .position(|id| id.get_op_id() == current_inst.get_op_id())
                .unwrap();
              let op_id = Operand::Value(new_id);
              bb.cur.insert(pos, op_id);
              op_id
            } else {
              let op_id = Operand::Value(new_id);
              bb.cur.push(op_id);
              op_id
            };
            func.op_to_bb[op_id] = current_block;
            op_id
          };

            self.add_uses(Some(current_function), op_id);
            self.add_control_flow(Some(current_function), op_id, current_block);
            op_id
        }
    }
  }

  pub fn create_at_head(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    op: Op,
  ) -> Operand {
    let bb_id = match &builder.current_block {
      Some(block) => block.get_bb_id(),
      None => unreachable!(),
    };

    let inst_id = {
      let current_function = current_function.unwrap();
      let cfg = &mut self.funcs[current_function].cfg;
      let bb = &cfg[bb_id];
      if bb.cur.is_empty() {
        None
      } else {
        Some(bb.cur[0])
      }
    };

    builder.set_before_inst(self, current_function, inst_id);
    self.create(builder, current_function, op)
  }

  pub fn set_before_term(&mut self, builder: &mut Builder, current_function: Option<Operand>) {
    let current_function = current_function.unwrap();
    let current_block = builder
      .current_block
      .unwrap_or_else(|| panic!("IR set_before_term: current_block is None"));

    let term_id = {
      let func = &self.funcs[current_function];
      let bb = &func.cfg[current_block];
      bb.cur
        .iter()
        .copied()
        .find(|op_id| func.dfg[*op_id].data.is_terminator())
        .unwrap_or_else(|| {
          panic!(
            "IR set_before_term: block {:?} has no terminator",
            current_block
          )
        })
    };

    builder.set_before_inst(self, Some(current_function), Some(term_id));
  }

  pub fn create_before_term(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    op: Op,
  ) -> Operand {
    self.set_before_term(builder, current_function);
    self.create(builder, current_function, op)
  }

  pub fn create_new_block(&mut self, current_function: Option<Operand>) -> Operand {
    let current_function = current_function.unwrap();
    let cfg = &mut self.funcs[current_function].cfg;
    let bb_id = cfg.alloc(BasicBlock::default());
    Operand::BB(bb_id)
  }

  pub fn get_all_ops(&self, current_function: Option<Operand>, op_typ: OpType) -> Vec<Operand> {
    let dfg = &self.funcs[current_function.unwrap()].dfg;
    dfg
      .storage
      .iter()
      .enumerate()
      .filter_map(|(idx, item)| {
        if let ArenaItem::Data(node) = item {
          if node.is(op_typ) {
            Some(Operand::Value(idx))
          } else {
            None
          }
        } else {
          None
        }
      })
      .collect::<Vec<Operand>>()
  }

  pub fn get_all_ops_in_block(
    &self,
    current_function: Option<Operand>,
    block: Operand,
    op_typ: OpType,
  ) -> Vec<Operand> {
    let (cfg, dfg) = (
      &self.funcs[current_function.unwrap()].cfg,
      &self.funcs[current_function.unwrap()].dfg,
    );

    let bb_id = block.get_bb_id();
    let bb = &cfg[bb_id];

    let mut ops = Vec::new();
    for inst in &bb.cur {
      let data = &dfg[inst.get_op_id()];
      if data.is(op_typ) {
        ops.push(*inst);
      }
    }
    ops
  }

  pub fn get_all_non_phi_in_block(
    &mut self,
    current_function: Option<Operand>,
    block: Operand,
  ) -> Vec<Operand> {
    let current_function = current_function.unwrap();
    let func = &self.funcs[current_function];
    let (cfg, dfg) = (&func.cfg, &func.dfg);

    let bb_id = block.get_bb_id();
    let bb = &cfg[bb_id];

    let mut ops = Vec::new();
    for inst in &bb.cur {
      let data = &dfg[inst.get_op_id()];
      if !data.is(OpType::Phi) {
        ops.push(*inst);
      }
    }
    ops
  }

  pub fn remove_op(&mut self, current_function: Option<Operand>, op: Operand) -> Op {
    if matches!(op, Operand::Global(_)) {
      assert!(!self.has_users(None, op));
      let removed_op = self.globals.remove(op.get_global_id());
      return removed_op;
    }

    let current_function = current_function.unwrap();
    let bb_id = self.funcs[current_function].op_to_bb[op];
    assert!(
      matches!(bb_id, Operand::BB(_)),
      "IR remove_op: instruction {:?} is not mapped to a block",
      op
    );

    self.remove_uses(Some(current_function), op);
    self.remove_control_flow(Some(current_function), op);

    let func = &mut self.funcs[current_function];
    let (cfg, dfg, op_to_bb) = (&mut func.cfg, &mut func.dfg, &mut func.op_to_bb);

    let op_id = op.get_op_id();
    let bb_idx = bb_id.get_bb_id();
    let bb = &mut cfg[bb_idx];

    if let Some(pos) = bb.cur.iter().position(|id| id.get_op_id() == op_id) {
      bb.cur.remove(pos);
    } else {
      panic!(
        "IR remove_op: instruction {:?} not found in block {:?}",
        op, bb_idx
      );
    }

    op_to_bb[op] = Operand::default();
    let removed_op = dfg.remove(op_id);
    assert!(removed_op.users.is_empty());
    removed_op
  }

  pub fn replace_op(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    op_id: Operand,
    new_op: Op,
  ) -> Operand {
    let current_function = current_function.unwrap();
    let bb_id = self.funcs[current_function].op_to_bb[op_id];
    assert!(
      matches!(bb_id, Operand::BB(_)),
      "IR replace_op: instruction {:?} is not mapped to a block",
      op_id
    );

    let pos = {
      let cfg = &mut self.funcs[current_function].cfg;
      let bb = &cfg[bb_id];
      bb.cur
        .iter()
        .position(|id| id.get_op_id() == op_id.get_op_id())
        .unwrap()
    };

    let next_inst = {
      let cfg = &mut self.funcs[current_function].cfg;
      let bb = &cfg[bb_id.get_bb_id()];
      bb.cur.get(pos + 1).cloned()
    };

    let mut guard = BuilderGuard::new(builder);
    {
      guard.set_current_block(bb_id);
      // Create new instruction first.
      guard.set_before_inst(self, Some(current_function), next_inst);
      let new_op_id = guard.create(self, Some(current_function), new_op);
      // RAUW
      self.replace_all_uses(Some(current_function), op_id, new_op_id);
      // Remove old instruction.
      self.remove_op(Some(current_function), op_id);
      new_op_id
    }
  }

  pub fn move_op_to_bb_at(
    &mut self,
    current_function: Option<Operand>,
    op: Operand,
    new_bb: Operand,
    pos: Option<Operand>,
  ) {
    let current_function = current_function.unwrap();
    let func = &mut self.funcs[current_function];
    let (cfg, op_to_bb) = (&mut func.cfg, &mut func.op_to_bb);

    let op_id = op.get_op_id();
    let old_bb = op_to_bb[op];
    let old_bb_id = old_bb.get_bb_id();

    let old_bb_ref = &mut cfg[old_bb_id];
    if let Some(pos) = old_bb_ref.cur.iter().position(|id| id.get_op_id() == op_id) {
      old_bb_ref.cur.remove(pos);
    } else {
      panic!(
        "IR move_op_to_bb_at: instruction {:?} not found in old_bb {:?}",
        op, old_bb
      );
    }

    let new_bb_id = new_bb.get_bb_id();
    let new_bb_ref = &mut cfg[new_bb_id];
    if let Some(pos) = pos {
      let pos_id = pos.get_op_id();
      if let Some(pos) = new_bb_ref
        .cur
        .iter()
        .position(|id| id.get_op_id() == pos_id)
      {
        new_bb_ref.cur.insert(pos, op);
      } else {
        panic!(
          "IR move_op_to_bb_at: instruction {:?} not found in new_bb {:?}",
          pos, new_bb
        );
      }
    } else {
      new_bb_ref.cur.push(op);
    }

    op_to_bb[op] = new_bb;
  }

  pub fn redirect_bb(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    operand: Operand,
    old_bb: Operand,
    new_bb: Operand,
  ) -> Operand {
    let current_function = current_function.unwrap();
    let term_id = match operand {
      Operand::BB(_) => {
        let term_id = self.funcs[current_function].cfg[operand]
          .cur
          .iter()
          .rev()
          .copied()
          .find(|op_id| {
            self.funcs[current_function].dfg[*op_id]
              .data
              .is_terminator()
          })
          .unwrap_or_else(|| panic!("IR redirect_bb: block {:?} has no terminator", operand));
        term_id
      }
      Operand::Value(_) => {
        let op_data = &self.funcs[current_function].dfg[operand].data;
        if !matches!(op_data, OpData::Br { .. } | OpData::Jump { .. }) {
          panic!("IR redirect_bb: expected Br or Jump, got {:?}", op_data);
        }
        operand
      }
      _ => panic!(
        "IR redirect_bb: expected BB or Value operand, got {:?}",
        operand
      ),
    };

    let Op {
      typ, attrs, data, ..
    } = self.funcs[current_function].dfg[term_id].clone();
    let new_data = match data {
      OpData::Br {
        cond,
        mut then_bb,
        mut else_bb,
      } => {
        let mut matched = false;
        if then_bb == old_bb {
          then_bb = new_bb;
          matched = true;
        }
        if else_bb == old_bb {
          else_bb = new_bb;
          matched = true;
        }
        if !matched {
          panic!(
            "IR redirect_bb: branch {:?} does not target {:?}",
            term_id, old_bb
          );
        }
        OpData::Br {
          cond,
          then_bb,
          else_bb,
        }
      }
      OpData::Jump { target_bb } => {
        if target_bb != old_bb {
          panic!(
            "IR redirect_bb: jump {:?} targets {:?}, not {:?}",
            term_id, target_bb, old_bb
          );
        }
        OpData::Jump { target_bb: new_bb }
      }
      _ => unreachable!("IR redirect_bb: checked terminator kind above"),
    };

    self.replace_op(
      builder,
      Some(current_function),
      term_id,
      Op::new(typ, attrs, new_data),
    )
  }

  pub fn split_block_before(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    current_block: Operand,
    split_point: Option<Operand>,
  ) -> Operand {
    self.split_block(builder, current_function, current_block, split_point, false)
  }

  pub fn split_block_after(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    current_block: Operand,
    split_point: Option<Operand>,
  ) -> Operand {
    self.split_block(builder, current_function, current_block, split_point, true)
  }

  fn split_block(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    current_block: Operand,
    split_point: Option<Operand>,
    after: bool,
  ) -> Operand {
    let current_function = current_function.unwrap();
    let cur = self.funcs[current_function].cfg[current_block].cur.clone();
    let term_id = *cur
      .last()
      .unwrap_or_else(|| panic!("IR split_block: block {:?} is empty", current_block));
    if !self.funcs[current_function].dfg[term_id]
      .data
      .is_terminator()
    {
      panic!(
        "IR split_block: final instruction {:?} in {:?} is not a terminator",
        term_id, current_block
      );
    }

    let split_pos = if let Some(split_point) = split_point {
      cur
        .iter()
        .position(|op_id| *op_id == split_point)
        .unwrap_or_else(|| {
          panic!(
            "IR split_block: split point {:?} not found in block {:?}",
            split_point, current_block
          )
        })
    } else {
      cur.len()
    };

    let moved_ops = cur
      .iter()
      .enumerate()
      .filter_map(|(idx, op_id)| {
        let op = &self.funcs[current_function].dfg[*op_id];
        if *op_id == term_id || op.is(OpType::Phi) || op.is(OpType::Ret) {
          return None;
        }

        let should_move = if split_point.is_none() {
          false
        } else if after {
          idx > split_pos
        } else {
          idx >= split_pos
        };
        should_move.then_some(*op_id)
      })
      .collect::<Vec<_>>();

    let new_bb = self.create_new_block(Some(current_function));
    {
      let func = &mut self.funcs[current_function];
      func.cfg[current_block]
        .cur
        .retain(|op_id| !moved_ops.contains(op_id));
      func.cfg[new_bb].cur.extend(moved_ops.iter().copied());
      for op_id in &moved_ops {
        func.op_to_bb[*op_id] = new_bb;
      }
    }

    let copied_term = self.funcs[current_function].dfg[term_id].clone();
    let old_succs = match &copied_term.data {
      OpData::Br {
        then_bb, else_bb, ..
      } => {
        let mut succs = vec![*then_bb];
        if else_bb != then_bb {
          succs.push(*else_bb);
        }
        succs
      }
      OpData::Jump { target_bb } => vec![*target_bb],
      OpData::Ret { .. } => vec![],
      _ => unreachable!("Unexpected terminator kind: {:?}", copied_term.data),
    };

    {
      let mut guard = BuilderGuard::new(builder);
      guard.set_current_block(new_bb);
      guard.set_before_inst(self, Some(current_function), None);
      guard.create(self, Some(current_function), copied_term);
    }

    self.replace_op(
      builder,
      Some(current_function),
      term_id,
      Op::new(Type::Void, vec![], OpData::Jump { target_bb: new_bb }),
    );

    for succ in old_succs {
      let phis = self.get_all_ops_in_block(Some(current_function), succ, OpType::Phi);
      for phi in phis {
        let value = match &self.funcs[current_function].dfg[phi].data {
          OpData::Phi { incomings } => incomings.iter().find_map(|incoming| {
            if let PhiIncoming::Data { value, bb } = incoming {
              (*bb == current_block).then_some(*value)
            } else {
              None
            }
          }),
          _ => unreachable!("IR split_block: expected phi"),
        };
        if let Some(value) = value {
          self.slay_phi_incoming(Some(current_function), phi, current_block);
          self.append_phi_incoming(Some(current_function), phi, value, new_bb);
        }
      }
    }

    new_bb
  }

  pub fn get_src_tuple(
    &self,
    current_function: Option<Operand>,
    op_id: Operand,
  ) -> Vec<(&Operand, usize)> {
    match op_id {
      Operand::Value(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function].get_src_tuple(op_id)
      }
      Operand::Global(_) => DFG::match_src_tuple(&self.globals[op_id].data),
      _ => vec![],
    }
  }

  pub fn get_src(&self, current_function: Option<Operand>, op_id: Operand) -> Vec<&Operand> {
    self
      .get_src_tuple(current_function, op_id)
      .into_iter()
      .map(|(src, _)| src)
      .collect()
  }

  pub fn get_src_tuple_mut(
    &mut self,
    current_function: Option<Operand>,
    op_id: Operand,
  ) -> Vec<(&mut Operand, usize)> {
    match op_id {
      Operand::Value(_) => {
        let current_function = current_function.unwrap();
        self.funcs[current_function].get_src_tuple_mut(op_id)
      }
      Operand::Global(_) => DFG::match_src_tuple_mut(&mut self.globals[op_id].data),
      _ => vec![],
    }
  }

  pub fn get_src_mut(
    &mut self,
    current_function: Option<Operand>,
    op_id: Operand,
  ) -> Vec<&mut Operand> {
    let current_function = current_function.unwrap();
    self.funcs[current_function].get_src_mut(op_id)
  }

  /// This function update the phi incoming which has an allocated slot.
  pub fn add_phi_incoming(
    &mut self,
    current_function: Option<Operand>,
    phi: Operand,
    idx: usize,
    value: Operand,
    bb: Operand,
  ) {
    let current_function = current_function.unwrap();
    let phi_id = phi.get_op_id();

    let old_incoming = match &self.funcs[current_function].dfg[phi_id].data {
      OpData::Phi { incomings } => incomings[idx].clone(),
      _ => unreachable!(),
    };

    if let PhiIncoming::Data { value, .. } = old_incoming {
      self.remove_use(Some(current_function), value, (phi, idx));
    }

    if let OpData::Phi { incomings } = &mut self.funcs[current_function].dfg[phi_id].data {
      incomings[idx] = PhiIncoming::Data { value, bb };
    } else {
      unreachable!()
    }
    self.add_use(Some(current_function), value, (phi, idx));
  }

  /// This function append a new phi incoming and return the new incoming index.
  pub fn append_phi_incoming(
    &mut self,
    current_function: Option<Operand>,
    phi: Operand,
    value: Operand,
    bb: Operand,
  ) {
    let current_function = current_function.unwrap();
    let phi_id = phi.get_op_id();

    let idx = if let OpData::Phi { incomings } = &mut self.funcs[current_function].dfg[phi_id].data
    {
      incomings.push(PhiIncoming::Data { value, bb });
      incomings.len() - 1
    } else {
      unreachable!()
    };
    self.add_use(Some(current_function), value, (phi, idx));
  }

  /// Set a phi incoming slot to None while preserving arity.
  #[allow(unused)]
  pub fn remove_phi_incoming(
    &mut self,
    current_function: Option<Operand>,
    phi: Operand,
    idx: usize,
  ) {
    unimplemented!()
  }

  /// Eliminate the phi edge from the incomings.
  pub fn slay_phi_incoming(
    &mut self,
    current_function: Option<Operand>,
    phi: Operand,
    bb: Operand,
  ) {
    let phi_id = phi.get_op_id();

    let current_function = current_function.unwrap();

    if let OpData::Phi { incomings } = self.funcs[current_function].dfg[phi_id].data.clone() {
      if let Some(pos) = incomings.iter().position(|inc| {
        if let PhiIncoming::Data { bb: inc_bb, .. } = inc {
          inc_bb == &bb
        } else {
          false
        }
      }) {
        let PhiIncoming::Data { value, .. } = incomings[pos].clone() else {
          unreachable!("IR slay_phi_incoming: matched incoming must be data");
        };
        self.remove_use(Some(current_function), value, (phi, pos));

        for (idx, incoming) in incomings.iter().enumerate().skip(pos + 1) {
          let PhiIncoming::Data { value, .. } = incoming else {
            continue;
          };
          if !matches!(
            value,
            Operand::Value(_) | Operand::Param(_) | Operand::Global(_)
          ) {
            continue;
          }
          let users = self.users_mut(Some(current_function), *value);
          if let Some((_, user_idx)) = users
            .iter_mut()
            .find(|(user, user_idx)| *user == phi && *user_idx == idx)
          {
            *user_idx -= 1;
          } else {
            panic!(
              "IR slay_phi_incoming: user tuple ({:?}, {}) not found for {:?}",
              phi, idx, value
            );
          }
        }

        if let OpData::Phi { incomings } = &mut self.funcs[current_function].dfg[phi_id].data {
          // DO NOT use swap_remove here.
          incomings.remove(pos);
        } else {
          panic!("IR slay_phi_incoming: not a phi node");
        }
      } else {
        panic!(
          "IR slay_phi_incoming: no incoming edge from bb {:?} to phi {:?}",
          bb, phi
        );
      }
    } else {
      panic!("IR slay_phi_incoming: not a phi node");
    }
  }

  pub fn gc(&mut self) -> Vec<ArenaItem<Function>> {
    for func_id in self.funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      let func = &mut self.funcs[func_id];
      for item in func.dfg.storage.iter_mut() {
        if let ArenaItem::Data(op) = item {
          op.users.clear();
        }
      }
      func.params.clear_uses();
    }

    self.global_uses = FuncsGlobalUses::default();
    let old_arena = self.funcs.gc();

    for func_id in self.funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      let blocks = self.funcs[func_id]
        .cfg
        .collect()
        .into_iter()
        .map(|bb| self.funcs[func_id].cfg[Operand::BB(bb)].cur.clone())
        .collect::<Vec<_>>();

      for ops in blocks {
        for op_id in ops {
          self.add_uses(Some(func_id), op_id);
        }
      }
    }

    old_arena
  }
}

impl Default for IR {
  fn default() -> Self {
    Self::new()
  }
}
