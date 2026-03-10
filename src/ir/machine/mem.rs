pub type SlotId = usize;
pub struct FrameInfo {
	pub param_slots: Vec<Slot>
}

pub enum Slot {
	Param { offset: i32, size: u32 }, 
	Local { size: u32, align: u32, offset: i32 },
}

impl FrameInfo {
	/// Return the size of the entire stack frame.
	/// CAUTION: The size should be 16-bytes aligned.
	pub fn size(&mut self) -> u32 {
        todo!()
    }
	/// alloc local variable.
	pub fn alloc_local(&mut self, size: usize, align: usize) -> usize {
        todo!()
    }
	/// alloc params. 
	pub fn alloc_param(&mut self, size: usize, align: usize) -> usize {
        todo!()
    }
	pub fn alloc_callee_saved(&mut self, size: usize, align: usize) -> usize {
        todo!()
    }
	pub fn get_local(&self, slot_id: usize) -> Slot {
        todo!()
    }
}
