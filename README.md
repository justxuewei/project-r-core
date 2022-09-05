# Project rCore

Yet another rCore implementation by justxuewei, the original version you may refer to [rcore-os/rCore-Tutorial-v3](https://github.com/rcore-os/rCore-Tutorial-v3).

## Contents

In this repo:

- [os](./src/os): rCore operatering system running on riscv64.
- [user](./src/user): user applications.
- ch1-exercises
  - [ls](./src/ls): ls program, [reference](https://rcore-os.github.io/rCore-Tutorial-Book-v3/chapter1/7exercise.html).
  - [backtrace](./src/backtrace): (WIP: encountered segmentation fault) backtrace using base pointer and frame pointer running on Linux x64, [reference](https://rcore-os.github.io/rCore-Tutorial-Book-v3/chapter1/7exercise.html).
- ch2-exercises
  - backtrace: implemented at [stack_trace.rs](./src/os/src/stack_trace.rs).
  - get-task-info: implemented at [os: syscall/task_info.rs](./src/os/src/syscall/task_info.rs) and [user: 05get_task_info.rs](./src/user/src/bin/05get_task_info.rs).
  - app-elapsed-time: implemented at [batch::app_running_time](./src/os/src/batch.rs). 
  - syscall-times: implemented at [batch::stat_syscall](./src/os/src/batch.rs). 

Reviews (in chinese):

- [Project rCore: 地址空间](https://nxw.name/2022/address-space-introduction)
- [Project rCore: 地址空间](https://nxw.name/2022/address-space-introduction)
- [Project rCore: 进程](https://nxw.name/2022/project-rcore-process)
- [Project rCore: 文件系统](https://nxw.name/2022/project-rcore-file-system)

## Tips

VSCode configs for riscv64 target to avoid `#![no_std]` warning.

```
// ref: https://github.com/rust-lang/vscode-rust/issues/729
// for rust extenstion:
{
    "rust.target": "riscv64gc-unknown-none-elf",
    "rust.all_targets": false
}

// for rust-analyzer extenstion:
{
    "rust-analyzer.cargo.target": "riscv64gc-unknown-none-elf",
    "rust-analyzer.checkOnSave.allTargets": false
}
```

Recommended extensions for VSCode

- rust-analyzer
- GitLens
- RISC-V Support
- Markdown All in One
- Makefile Tools
