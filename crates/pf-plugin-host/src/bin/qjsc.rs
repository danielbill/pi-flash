//! `qjsc` —— 把插件 JS 编译成 QuickJS 字节码（分发态）。
//!
//! 分发链：`packages/<slug>/main.js` → **本工具编译** → `plugin.qbc`（二进制，
//! 无明文源码）→ 打进包。宿主用 `JsPlugin::load_bytecode` 加载，引擎/ctx/渲染
//! 与源码版**完全相同**——同一份源码编出来的，显示不可能不一致。
//!
//! 用法：`qjsc <input.js> <output.qbc>`
//!
//! 版本约束：字节码与 quickjs-ng 版本绑定，换 rquickjs 版本必须重新编译分发包
//! （宿主读旧字节码会由 JS_ReadObject 的版本校验拒绝，不会静默跑错）。

use std::process::ExitCode;

use rquickjs::context::Ctx;
use rquickjs::module::{Module, WriteOptions, WriteOptionsEndianness};
use rquickjs::{Context, Runtime};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("用法: qjsc <input.js> <output.qbc>");
        return ExitCode::FAILURE;
    }
    let (src_path, out_path) = (&args[0], &args[1]);
    let src = match std::fs::read_to_string(src_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("✗ 源码不可读 {src_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("✗ QuickJS 运行时初始化失败: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ctx = match Context::full(&rt) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ 上下文创建失败: {e}");
            return ExitCode::FAILURE;
        }
    };
    let out = ctx.with(|ctx| -> Result<Vec<u8>, String> {
        compile(ctx, src).map_err(|e| format!("编译失败: {e}"))
    });
    match out {
        Ok(bytes) => match std::fs::write(out_path, &bytes) {
            Ok(()) => {
                println!("{} bytes → {out_path}", bytes.len());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("✗ 写字节码失败 {out_path}: {e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("✗ {e}");
            ExitCode::FAILURE
        }
    }
}

fn compile(ctx: Ctx<'_>, src: String) -> Result<Vec<u8>, rquickjs::Error> {
    // 模块名固定（宿主侧同一名字加载，稳定）
    let module = Module::declare(ctx, "pf-plugin-entry", src.into_bytes())?;
    // 大小端固定 = 产物可跨机器复用；宿主读侧由 QuickJS 按头部判定
    module.write(WriteOptions {
        endianness: WriteOptionsEndianness::Little,
        ..Default::default()
    })
}
