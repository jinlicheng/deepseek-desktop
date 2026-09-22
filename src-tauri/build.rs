fn main() {
    // 图标是编译期嵌入的（运行时窗口图标、托盘图标）：让 cargo 在 icons 变化时重新构建，
    // 否则改了图标可能因为看不到依赖变化而跳过编译，跑出来的还是旧图标
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
