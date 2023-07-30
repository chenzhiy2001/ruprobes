#![no_std]
#![feature(naked_functions)]
#![feature(asm)]
#![feature(unsafe_block_in_unsafe_fn)]
#[macro_export]

#[macro_use]
extern crate log;
extern crate alloc;

use alloc::string::String;
extern "C" {
    fn get_new_page(addr: usize, len: usize) -> usize;
    fn set_writeable(addr: usize);
    fn get_exec_path() -> String;
    fn os_copy_from_user(usr_addr: usize, kern_buf: *mut u8, len: usize) -> i32;
    fn os_copy_to_user(usr_addr: usize, kern_buf: *const u8, len: usize) -> i32;
}

// mod kprobes;
mod riscv_insn_decode;
mod uprobes;
mod probes;

//use alloc::sync::Arc;
// pub use kprobes::kprobes_trap_handler;
pub use uprobes::uprobes_trap_handler;
//use spin::Mutex;
//use trapframe::TrapFrame;
pub use probes::ProbeType;
pub use probes::ProbePlace;
pub use uprobes::{uprobes_init,uprobe_register};
// pub use kprobes::ProbeType;

// pub fn kprobe_register(addr: usize, handler: Arc<Mutex<dyn FnMut(&mut TrapFrame) + Send>>, post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapFrame) + Send>>>, probe_type: ProbeType) -> isize {
//     kprobes::KPROBES.register_kprobe(addr, handler, post_handler, probe_type)
// }

// pub fn kprobe_unregister(addr: usize) -> isize {
//     kprobes::KPROBES.unregister_kprobe(addr)
// }


// #[cfg(rCore-Tutorial)]
// pub fn uprobe_register(path: String, addr: usize, handler: Arc<Mutex<dyn FnMut(&mut TrapFrame) + Send>>, post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapFrame) + Send>>>, probe_type: ProbeType) -> isize {
//     uprobes::UPROBES.register_uprobe()
// }

// #[cfg(rCore-Tutorial)]
// pub fn uprobe_unregister(path: String, addr: usize) -> isize {
//     todo!()
// }