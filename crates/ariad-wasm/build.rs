//! Gives the WebAssembly module a stack deep enough for documents at the local
//! nesting limit; the default 1 MiB overflows on deeply nested IR.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo::rustc-link-arg-cdylib=-zstack-size=8388608");
    }
}
