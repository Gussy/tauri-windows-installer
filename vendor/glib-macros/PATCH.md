# GLib macros 0.18.5 dependency patch

Vendored from the MIT-licensed crates.io `glib-macros` 0.18.5 release, upstream
revision `42b9caf98e03ded086362d9653ca58fe94dc8658`, subdirectory `glib-macros`.
The original package version is retained.

The normalized Cargo manifest aliases `proc-macro-error` to the maintained
[`proc-macro-error3`](https://github.com/gamma0987/proc-macro-error3) 3.1.1 release,
with default features disabled and `syn2-error` enabled to match this crate's
existing Syn 2 API. This removes the unmaintained original dependency covered by
RUSTSEC-2024-0370. The intermediate `proc-macro-error2` fork is also unmaintained
(RUSTSEC-2026-0173), so it is not used. Original manifest provenance remains in
`Cargo.toml.orig`; Cargo resolves the edited `Cargo.toml`.

One `extern crate` alias in `src/lib.rs` also exposes the maintained package's
canonical name, because its attribute macro emits `::proc_macro_error3` paths.
The rest of the Rust source is unchanged.

Remove this patch when a compatible upstream GLib macros release migrates to a
maintained error reporting dependency.
