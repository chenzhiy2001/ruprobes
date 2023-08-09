# ruprobes
`ruprobes` is a modularized version of `uprobe` originated from [rCore-ebpf](https://github.com/hm1229/rCore-ebpf) which currently runs on [rCore-Tutorialv3](https://github.com/rcore-os/rCore-Tutorial-v3). It helps you dynamically probe one or more functions and instructions in user space.

The crate aims to be modular as possible. If your OS has sufficient eBPF codes, you only need to write a handler and 2 function calls in your OS. See [registering-uprobes](#registering-uprobes) and [uprobes-init-and-handling](#uprobes-init-and-handling).

## Usage
The following describes how you can use this crate in your OS for uprobe functionalities. All example code comes from [this commit](https://github.com/chenzhiy2001/rcore-ebpf/commit/108e81bab6d83c445ca9d70fdc5be55f588b5f21).

### Prerequisites
Before porting this crate to your OS, it should already have eBPF support. It is even better if the OS has kprobes support.

### Adding Dependencies
Firstly, you should append ruprobes and its dependencies in `Cargo.toml`. For example, in rCore-Tutorial-v3:
```
ruprobes = { git = "https://github.com/chenzhiy2001/ruprobes", features = ["rCore-Tutorial"] }
trap_context_riscv = { git = "https://github.com/chenzhiy2001/trap_context_riscv"}
trapframe = { git = "https://github.com/rcore-os/trapframe-rs"}
spin = "0.5"
```
If you're porting to other OSes, you might need some minor changes in ruproes code which is being discussed [here](#Supporting More OSes).

### Reading Data from User Space Addresses
Your need to implement 2 functions in kernel in order to copy/write data from/to user space address because `ruprobes` would modifiy the page where a breakpoint is setted. The functions look like this:
```rust
#[no_mangle]
pub extern "C" fn os_copy_from_user(usr_addr: usize, kern_buf: *mut u8, len: usize) -> i32;
#[no_mangle]
pub extern "C" fn os_copy_to_user(usr_addr: usize, kern_buf: *const u8, len: usize) -> i32;
```
Their implementations differ in different OSes due to different page table design. For example, in [rCore-ebpf](https://github.com/hm1229/rCore-ebpf), the kernel can read/write user virtual address *directly* because it costs only one pagetable for a process and its kernel space, while in [rCore-Tutorialv3](https://github.com/rcore-os/rCore-Tutorial-v3), a so-called dual-pagetable design (which means that the processes and kernel use different pagetables) is being used, which makes reading and writing user addresses complicated because you'll have to do more page pable manipulations.

The use of `#[no_mangle]` and `extern "C"` syntaxes makes sure that ruprobes can use those functions you have provided.

Please check the documents of your kernel's eBPF and kprobe implementations because they might already have similar code doing this. If so, you can just write a wrapper around them(e.g., the one by livingshade: <https://livingshade.github.io/ebpf-doc/rcore/>).

### Compatibility with existing eBPF implementation
Your OS's eBPF implementation usually has a struct of tracepoint types such as kprobe, kretprobe, etc. You need to add uprobe tracepoint types in it. 

An example:
```diff
 pub enum TracepointType {
     KProbe,
     KRetProbeEntry,
     KRetProbeExit,
+    UProbe_Insn,
+    URetProbeEntry_Insn, //javascript-level long names :(
+    URetProbeExit_Insn,
+    UProbe_SyncFunc,
+    URetProbeEntry_SyncFunc,
+    URetProbeExit_SyncFunc,
  }
```

Also make sure that your eBPF implementation can specify user's uprobe requests. For example:

```rust
else if type_str.eq_ignore_ascii_case("uretprobe_insn@entry") {
        tp_type = URetProbeEntry_Insn;
}

```

Then define BPFContext by following its kprobe counterparts. For example, in `rcore-ebpf`:
```rust
#[repr(C)]
/// uProbe context are just registers, or Trapframe
struct UProbeBPFContext {
    ptype: usize,//0 is syncfunc
    paddr: usize,
    tf: TrapFrame,
}

impl UProbeBPFContext {
    pub fn new(tf: &TrapFrame, probed_addr: usize, t: usize) -> Self {
        UProbeBPFContext {
            ptype: t,
            paddr: probed_addr,
            tf: tf.clone(),
        }
    }
```


You may find that your eBPF implementation uses a different kind of TrapFrame struct. In this case, consider using [trap_context_riscv](https://github.com/chenzhiy2001/trap_context_riscv) in your eBPF implementation or write a transformation function.


### Registering Uprobes
You'll need a handler to run eBPF programs when a tracepoint is triggered. For example:

```rust
fn uprobe_syncfunc_handler(tf: &mut trap_context_riscv::TrapContext, probed_addr: usize) {//tag: uprobe_handler
    let tracepoint:Tracepoint=Tracepoint::new(UProbe_SyncFunc, probed_addr);
    let ctx: UProbeBPFContext = UProbeBPFContext::new(&tf,probed_addr,0);
    info!("run attached progs in uprobe_syncfunc_handler!");
    run_attached_programs(&tracepoint, ctx.as_ptr());
    info!("run attached progs in uprobe_syncfunc_handler exit!");
}

```

Your eBPF implementation usually provides a handler which is called in OS's trap handling code. In this "master handler" you need to check the probe type then register the responding uprobe by calling `uprobe_register`.

```rust
/// ...
UProbe_SyncFunc => { //tag: uprobe_handler
                uprobe_register(user_program_path.unwrap().to_string(), addr,  Arc::new(spin_Mutex::new(uprobe_syncfunc_handler)),None, ruprobes::ProbeType::SyncFunc);  
                map.insert(tracepoint, vec![program]);
            }
```
### Uprobes Init and Handling

In `sys_exec`, you need to call `uprobes_init()` 

In your OS's trap handler, you need to check trap scause. If it's a breakpoint(`ebreak`), then call `uprobes_trap_handler`. For example:

```rust
    match scause.cause() {
        Trap::Exception(Exception::Breakpoint) => { // uprobe
            let mut cx = current_trap_cx();
            println!("[user] breakpoint at {:#x}", cx.sepc);
            unsafe {
                // This works but looks messy. We should use a clearer syntax
                // TrapContext(from rCore-Tutorial) => UserContext (from rCore-Plus, supported by ruprobes)
                uprobes_trap_handler(cx);
            }
        }

```

### Some Headers
You may need a `uprobes.h` based on your existing `kprobes.h`.

## Examples
- [rcore-ebpf](https://github.com/chenzhiy2001/rcore-ebpf)  (All letters are lowercase) is an Operating System with eBPF, kprobes and uprobes support which is suitable for OS debugging with [code-debug](https://github.com/chenzhiy2001/code-debug) tool. See its uprobes implementation details [here](https://github.com/chenzhiy2001/rcore-ebpf/commit/108e81bab6d83c445ca9d70fdc5be55f588b5f21). The work is based on:
  - [ruprobes by chenzhiy2001](https://github.com/chenzhiy2001/ruprobes)
  - [kprobes by cubele](https://cubele.github.io/probe-docs/ebpf%E7%A7%BB%E6%A4%8D/kprobes/%E5%AE%9E%E7%8E%B0/#_2)
  - [eBPF by livingshade](https://livingshade.github.io/ebpf-doc/)
  - [rCore-Tutorial-v3 by rCore Community](https://github.com/rcore-os/rCore-Tutorial-v3)
## Hacking
### Supporting More OSes
1. Clone thie repository
2. in `Cargo.toml`, change the ruprobes import to:
```rust
ruprobes = { path = "/path/to/ruprobes", features = ["YOUR_OS_NAME"] }
```
3. in the code of `ruprobes`, use `#[cfg(YOUR_OS_NAME)]` before your OS specific code.

