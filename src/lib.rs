#![no_std]
#![feature(naked_functions)]
#![feature(asm)]
#![feature(unsafe_block_in_unsafe_fn)]
#[macro_export]

#[macro_use]
extern crate log;
extern crate alloc;

// mod kprobes;
mod riscv_insn_decode;
mod uprobes;
mod probes;

use alloc::sync::Arc;
// pub use kprobes::kprobes_trap_handler;
pub use uprobes::uprobes_trap_handler;
use spin::Mutex;
use trapframe::TrapFrame;
pub use uprobes::ProbeType;
pub use probes::ProbePlace;
pub use uprobes::uprobes_init;

// pub use kprobes::ProbeType;

// pub fn kprobe_register(addr: usize, handler: Arc<Mutex<dyn FnMut(&mut TrapFrame) + Send>>, post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapFrame) + Send>>>, probe_type: ProbeType) -> isize {
//     kprobes::KPROBES.register_kprobe(addr, handler, post_handler, probe_type)
// }

// pub fn kprobe_unregister(addr: usize) -> isize {
//     kprobes::KPROBES.unregister_kprobe(addr)
// }

pub fn uprobe_register(path: String, addr: usize, handler: Arc<Mutex<dyn FnMut(&mut TrapFrame) + Send>>, post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapFrame) + Send>>>, probe_type: ProbeType) -> isize {
    #[cfg(rCore-Tutorial)]
    uprobes::UPROBES.register_uprobe()
}

pub fn uprobe_unregister(path: String, addr: usize) -> isize {
    #[cfg(rCore-Tutorial)]
    uprobes::UPROBES.register_uprobe()
}