// cargo build 会在编译整个程序前自动执行 build.rs，以编译一些非
// Rust 库，比如 C 依赖库等。
// Ref:
// https://course.rs/cargo/reference/build-script/intro.html
fn main() {
    println!("cargo:rerun-if-changed=../user/src");
    println!("cargo:rerun-if-changed={}", TARGET_PATH);
}

static TARGET_PATH: &str = "../user/target/riscv64gc-unknown-none-elf/release/";
