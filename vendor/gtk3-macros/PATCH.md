# GTK 3 macros 0.18.2 dependency patch

Vendored from the MIT-licensed crates.io `gtk3-macros` 0.18.2 release. Its packaged
`.cargo_vcs_info.json` records upstream revision
`00133512bfc8bbb8e9be59b20e93406c6a3eeb21`, subdirectory `gtk3-macros`, with the
upstream package's dirty flag retained. The original package version is retained.

The normalized Cargo manifest aliases `proc-macro-error` to the maintained
[`proc-macro-error3`](https://github.com/gamma0987/proc-macro-error3) 3.1.1 release,
with default features disabled and `syn2-error` enabled for the existing Syn 2 API.
This removes RUSTSEC-2024-0370's unmaintained dependency without introducing the
also-unmaintained `proc-macro-error2` fork. `Cargo.toml.orig` retains the original
manifest for provenance; Cargo resolves the edited `Cargo.toml`.

One `extern crate` alias in `src/lib.rs` exposes `proc_macro_error3` under its
canonical name for paths emitted by the maintained attribute macro. Other Rust
source is unchanged.

This patch does not make the upstream GTK 3 bindings maintained. Tauri's Linux
demo still depends on GTK 3. Remove the patch after upstream migrates to maintained
macros or Tauri migrates to GTK 4.
