//! Bindings generator entry point.
//!
//! Build the bridge library first, then generate Kotlin or Swift sources from
//! its embedded UniFFI metadata:
//!
//! ```text
//! cargo build -p plume-ffi
//! cargo run -p plume-ffi --bin uniffi-bindgen -- generate \
//!     --library target/debug/plume_ffi.dll --language kotlin \
//!     --out-dir android/app/src/main/java
//! ```
//!
//! Point `--library` at `libplume_ffi.so` for Android and `libplume_ffi.dylib`
//! for iOS.

fn main() {
    uniffi::uniffi_bindgen_main();
}
