use alloc::collections::btree_map::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
//use core::borrow::BorrowMut;
use core::cell::RefCell;
//use core::convert::TryInto;
use core::ops::FnMut;
//use core::pin::Pin;
use core::slice::{from_raw_parts, from_raw_parts_mut};
use spin::Mutex;
use lazy_static::*;
use core::arch::asm;
use crate::{get_new_page, os_copy_from_user, os_copy_to_user};
use crate::set_writeable;
use crate::get_exec_path;
extern crate trap_context_riscv;
use trap_context_riscv::TrapContext;
// extern "C" {
//     fn get_new_page(addr: usize, len: usize) -> usize;
//     fn set_writeable(addr: usize);
//     fn get_exec_path() -> String;
// }

#[cfg(feature = "rCore-Plus")]
use {
    rcore_memory::memory_set::MemoryAttr,
    rcore_memory::memory_set::handler::{Delay, ByFrame},
    rcore_memory::paging::PageTable,
    crate::memory::{AccessType, handle_page_fault_ext, GlobalFrameAlloc},
    crate::process::current_thread,
};


use crate::riscv_insn_decode::{insn_decode, InsnStatus, get_insn_length};
use super::probes::{get_sp, ProbeType};

use trapframe::{UserContext};
pub struct Uprobes {
    pub inner: RefCell<BTreeMap<usize, UprobesInner>>,
}

struct CurrentUprobes{
    inner: RefCell<BTreeMap<usize, UprobesInner>> ,
}

struct CurrentProcessUprobesInner{
    uprobes: Uprobes,
    current_uprobes: CurrentUprobes,
}

struct CurrentProcessUprobes{
    inner: RefCell<BTreeMap<String, CurrentProcessUprobesInner>>,
}

#[derive(Clone)]
pub struct UprobesInner {
    pub addr: usize,
    pub length: usize,
    pub slot_addr: usize,
    pub addisp: usize,
    pub func_ra: Vec<usize>,
    pub func_ebreak_addr: usize,
    pub insn_ebreak_addr: usize,
    pub handler: Arc<Mutex<for<'r> fn(&'r mut TrapContext,usize) >>, //tag: uprobe_handler
    pub post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapContext) + Send>>>,
    pub probe_type: ProbeType,
}


unsafe impl Sync for Uprobes {}
unsafe impl Sync for UprobesInner {}
unsafe impl Sync for CurrentUprobes {}
unsafe impl Sync for CurrentProcessUprobes {}
unsafe impl Sync for CurrentProcessUprobesInner {}

lazy_static! {
    pub static ref UPROBES: Uprobes = Uprobes::new();
}

lazy_static! {
    static ref CURRENT_PROCESS_UPROBES: CurrentProcessUprobes = CurrentProcessUprobes::new();
}

// error[E0787]: asm in naked functions must use `noreturn` option
//   --> /home/oslab/ruprobes/src/uprobes.rs:86:9
//    |
// 86 |         asm!("c.ebreak", "c.ebreak");
//    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
//    |
// help: consider specifying that the asm block is responsible for returning from the function
//    |
// 86 |         asm!("c.ebreak", "c.ebreak", options(noreturn));
//    |                                    +++++++++++++++++++

#[naked]
extern "C" fn __ebreak() {
    unsafe {
        asm!("c.ebreak", "c.ebreak", options(noreturn));
    }
}

impl CurrentProcessUprobes{
    fn new() -> Self{
        Self{
            inner: RefCell::new(BTreeMap::new()),
        }
    }

    fn uprobes_init(&self){
        info!("uprobes_init");
        unsafe{
            let my_path = get_exec_path();
            if let Some(inner) = self.inner.borrow().get(&my_path){
                inner.uprobes.add_uprobepoint();
            }
        }
    }

    pub fn register_uprobes(
        &self,
        path: String,
        addr: usize,
        handler: Arc<Mutex<for<'r> fn(&'r mut TrapContext,usize) >>, //tag: uprobe_handler
        post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapContext) + Send>>>,
        probe_type: ProbeType
    ) -> isize {
        let mut uprobes_inner: core::cell::RefMut<'_, BTreeMap<String, CurrentProcessUprobesInner>> = self.inner.borrow_mut();
        if let Some(inner) = uprobes_inner.get_mut(&path.clone()){
            inner.uprobes.register_uprobe(addr, handler, post_handler, probe_type);
        }
        else{
            let uprobes = Uprobes::new();
            info!("uprobes: add new path");
            uprobes.register_uprobe(addr, handler, post_handler, probe_type);
            let current_uprobes = CurrentUprobes::new();
            uprobes_inner.insert(path.clone(), CurrentProcessUprobesInner{
                uprobes,
                current_uprobes,
            });
            info!("uprobes: insert success");
        }
        info!("uprobes: path={}", unsafe{get_exec_path()});
        unsafe{
            if path == get_exec_path(){
                info!("uprobes: path=execpath");
                uprobes_inner.get_mut(&path.clone()).unwrap().uprobes.inner.borrow_mut().get_mut(&addr).unwrap().add_uprobepoint();
                info!("uprobes: path=execpath, add sucess");
            }}
        0
    }

    unsafe fn uprobes_trap_handler(&self, trap_context: &mut TrapContext){
        let path = get_exec_path();
        let mut uprobes_inner = self.inner.borrow_mut();
        let mut uprobes = uprobes_inner.get(&path.clone()).unwrap().uprobes.inner.borrow_mut();
        let mut current_uprobes = uprobes_inner.get(&path.clone()).unwrap().current_uprobes.inner.borrow_mut();
        match uprobes.get_mut(&trap_context.sepc) {
            Some(probe) => {
                // run user defined handler
                (probe.handler.lock())(trap_context,trap_context.sepc); //tag: uprobe_handler
                // single step the probed instruction
                match probe.probe_type{
                    ProbeType::SyncFunc =>{
                        trap_context.x[2] = trap_context.x[2].wrapping_add(probe.addisp);
                        //cx.general.sp = cx.general.sp.wrapping_add(probe.addisp);
                        trap_context.sepc = trap_context.sepc.wrapping_add(probe.length);
                        //cx.sepc = cx.sepc.wrapping_add(probe.length);
                        if let Some(_) = probe.post_handler{
                            if !current_uprobes.contains_key(&probe.func_ebreak_addr){
                                current_uprobes.insert(probe.func_ebreak_addr, probe.clone());
                            }
                            let current_uprobe: &mut UprobesInner = current_uprobes.get_mut(&probe.func_ebreak_addr).unwrap();
                            current_uprobe.func_ra.push(trap_context.x[1]);
                            //current_uprobe.func_ra.push(cx.general.ra);
                            trap_context.x[1] = probe.func_ebreak_addr as usize;
                            //cx.general.ra = probe.func_ebreak_addr as usize;
                        }
                    },
                    ProbeType::Insn =>{
                        trap_context.sepc = probe.slot_addr as usize;
                        //cx.sepc = probe.slot_addr as usize;
                        probe.insn_ebreak_addr = trap_context.sepc + probe.length;
                        //probe.insn_ebreak_addr = cx.sepc + probe.length;
                        if !current_uprobes.contains_key(&probe.insn_ebreak_addr){
                            current_uprobes.insert(probe.insn_ebreak_addr, probe.clone());
                        }
                    }
                    ProbeType::AsyncFunc => {
                        unimplemented!("probing async function is not implemented yet")
                    }
                }
            }
            None => {
                match current_uprobes.get_mut(&trap_context.sepc){
                    Some(probe) =>{
                        if probe.insn_ebreak_addr == trap_context.sepc{
                            if let Some(post_handler) = &probe.post_handler{
                                (post_handler.lock())(trap_context);
                            }
                            let sepc = probe.addr + probe.length;
                            current_uprobes.remove(&trap_context.sepc);
                            trap_context.sepc = sepc;
                        }
                        else{
                            (probe.post_handler.as_ref().unwrap().lock())(trap_context);
                            trap_context.sepc = probe.func_ra.pop().unwrap();
                            if probe.func_ra.len() == 0{
                                current_uprobes.remove(&trap_context.sepc);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

impl CurrentUprobes{
    fn new() -> Self{
        Self{
            inner: RefCell::new(BTreeMap::new()),
        }
    }
}

impl UprobesInner {
    pub fn new(
        addr: usize,
        handler: Arc<Mutex<for<'r> fn(&'r mut TrapContext,usize) >>,//tag: uprobe_handler
        post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapContext) + Send>>>,
        probe_type: ProbeType
    ) -> Option<Self> {
        Some(Self {
            addr,
            length: 0,
            slot_addr: 0,
            addisp: 0,
            func_ra: Vec::new(),
            func_ebreak_addr: 0,
            insn_ebreak_addr: 0,
            handler,
            post_handler,
            probe_type,
        })
    }

    unsafe fn add_uprobepoint(&mut self){//这个函数的注释中所说的“改动”是指将对虚拟内存的直接读写用osutil.rs里的os_copy_from_user和os_copy_to_user替换，不是说将这个模块适配到其他os时就一定要替换。
        // get free point in user stack
        let addr = self.addr;
        unsafe {
            self.func_ebreak_addr = 
            get_new_page(addr, 2); //get_new_page是通过页表来查找空闲内存的，返回的是空闲的地址，但是对这个地址没有做读或写操作，故不需要改动
        }
        unsafe {
            self.slot_addr = 
            get_new_page(addr, 6);//不需要改动。理由同上。
        }//但是，涉及func_ebreak_addr，slot_addr两个指针的读写的部分要改动.
        //let mut slot: &mut [u8] = unsafe { from_raw_parts_mut(self.slot_addr as *mut u8, 6)};//涉及slot的显然要改动。
        unsafe{set_writeable(addr);}//不涉及用户内存空间的内存读写，故无需改动。
        let mut inst_copy:[u8;2]=[0,0];
        unsafe {
            os_copy_from_user(addr, &mut (inst_copy[0]), 2);
        }
        // let inst = unsafe { from_raw_parts(addr as *const u8, 2) };//涉及insn的要改动。
        // read the lowest byte of the probed instruction to determine whether it is compressed
        let length = get_insn_length(addr);//此处已经修复。
        self.length = length;//无需改动。
        // save the probed instruction to a buffer
        unsafe{
            os_copy_to_user(self.slot_addr, &inst_copy[0], length);
        }
        //slot[..length].copy_from_slice(&inst[..length]);

        // decode the probed instruction to retrive imm
        let ebreak = unsafe { from_raw_parts(__ebreak as *const u8, 2) };

        match self.probe_type{
            ProbeType::Insn =>{
                match insn_decode(addr){
                    InsnStatus::Legal =>{
                        unsafe{
                            os_copy_to_user(self.slot_addr+length, &ebreak[0], 2);
                        }
                        //slot[length..length+2].copy_from_slice(ebreak);
                        self.insn_ebreak_addr = self.slot_addr + length;
                    },
                    _ => {warn!("uprobes: instruction is not legal");},
                }
            }
            ProbeType::SyncFunc =>{
                unsafe{
                    os_copy_to_user(self.func_ebreak_addr, &ebreak[0], 2);
                }
                // let mut ebreak_ptr = unsafe { from_raw_parts_mut(self.func_ebreak_addr as *mut u8, 2)};
                // ebreak_ptr.copy_from_slice(ebreak);

                match get_sp(addr){
                    Some(sp) => self.addisp = sp,
                    None => {error!("sp not found!");}
                }
            }
            ProbeType::AsyncFunc =>{
                error!("not implemented yet!");
            }
        }
        self.arm()
    }

    pub fn arm(&self) {//要改动
        let ebreak = unsafe { from_raw_parts(__ebreak as *const u8, self.length) };
        unsafe{
            os_copy_to_user(self.addr, &(ebreak[0]), self.length);
        }
        // let mut inst = unsafe { from_raw_parts_mut(self.addr as *mut u8, self.length) };
        // inst.copy_from_slice(ebreak);
        unsafe { asm!("fence.i") };
    }

    pub fn disarm(&self) {//要改动
        unsafe{
            os_copy_to_user(self.addr, self.slot_addr as *const u8, self.length);
        }
        // let mut inst = unsafe { from_raw_parts_mut(self.addr as *mut u8, self.length) };
        // let slot = unsafe { from_raw_parts(self.slot_addr as *const u8, self.length)};
        // inst.copy_from_slice(slot);
        unsafe { asm!("fence.i") };
    }
}

impl Uprobes {
    pub fn register_uprobe(
        &self,
        addr: usize,
        handler: Arc<Mutex<for<'r> fn(&'r mut TrapContext,usize) >>, //tag: uprobe_handler
        post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapContext) + Send>>>,
        probe_type: ProbeType,
    ) -> isize{
        let probe = UprobesInner::new(addr, handler, post_handler, probe_type);
        if let Some(probe) = probe {
            self.inner.borrow_mut().insert(addr, probe);
            info!("uprobes: register success");
            1
        } else {
            error!("uprobes: probe initialization failed");
            -1
        }
    }

    fn new() -> Self {
        Self {
            inner: RefCell::new(BTreeMap::new()),
        }
    }

    fn uprobes_trap_handler(&self, cx: &mut UserContext) {

    }

    fn add_uprobepoint(&self){
        let mut uproebs = self.inner.borrow_mut();
        for inner in uproebs.values_mut(){
            unsafe { inner.add_uprobepoint() };
        }
    }
}

#[cfg(feature = "rCore-Plus")]
fn get_new_page(addr: usize, len: usize) -> usize{
    let thread = current_thread().unwrap();
    let mut vm = thread.vm.lock();
    let ebreak_addr = vm.find_free_area(addr, len);
    vm.push(
        ebreak_addr,
        ebreak_addr + len,
        MemoryAttr::default().user().execute().writable(),
        ByFrame::new(GlobalFrameAlloc),
        "point",
    );
    unsafe {asm!("fence.i");}
    ebreak_addr
}

#[cfg(feature = "rCore-Plus")]
fn set_writeable(addr: usize){
        let thread = current_thread().unwrap();
        let mut vm = thread.vm.lock();
        let mut page_table_entry = vm.get_page_table_mut().get_entry(addr).unwrap();
        page_table_entry.set_writable(true);
        unsafe {asm!("fence.i");}
}


#[cfg(feature = "rCore-Plus")]
fn get_exec_path() -> String{
    info!("uprobes: get path");
    // get path of current thread
    let ret = current_thread().unwrap().proc.try_lock().expect("locked!").exec_path.clone();
    info!("uprobes get path success path = {}", ret);
    ret
}



pub fn uprobe_register(
    path: String,
    addr: usize,
    handler: Arc<Mutex<for<'r> fn(&'r mut TrapContext,usize) >>,//tag: uprobe_handler
    post_handler: Option<Arc<Mutex<dyn FnMut(&mut TrapContext) + Send>>>,
    probe_type: ProbeType
) -> isize {
    CURRENT_PROCESS_UPROBES.register_uprobes(path ,addr, handler, post_handler, probe_type)
}

pub fn uprobes_trap_handler(cx: &mut TrapContext) {
    info!("uprobes: into uprobes trap handler");
    unsafe{
        CURRENT_PROCESS_UPROBES.uprobes_trap_handler(cx);
    }
}

pub fn uprobes_init(){
    CURRENT_PROCESS_UPROBES.uprobes_init();
    info!("uprobes: init sucess");
}