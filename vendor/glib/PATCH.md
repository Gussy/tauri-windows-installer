# GLib 0.18.5 local security backport

This directory contains the crates.io `glib` 0.18.5 release, under its original
MIT license. Upstream source revision: `42b9caf98e03ded086362d9653ca58fe94dc8658`,
subdirectory `glib`, recorded in `.cargo_vcs_info.json`. The package version is
retained as 0.18.5 to describe the API actually in use.

Only two source lines differ from that release: `VariantStrIter::impl_get`
declares its output pointer mutable and passes `&mut p` to
`g_variant_get_child`. This backports the upstream fix from
[gtk-rs-core PR 1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343) for
[RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html).
The immutable out-pointer in the released crate violates Rust's aliasing rules
and can cause a null dereference in optimized builds.

Tauri's Linux GTK 3 dependencies require GLib 0.18; replacing this crate with
the incompatible GLib 0.20 API would require an upstream GTK migration. Windows
installer, bundler and uninstaller binaries do not use this Linux demo dependency.

The reviewed `src/variant_iter.rs` SHA-256 is
`a0f5ee8acb8faa089bcdfbc9a57372609fce7654026ccef7d9a224d05a654ccc`.
CI verifies this exact source, the local Cargo patch and resolved package source
before running the dependency audit. Cargo audit does not apply registry-version
advisories to this local path dependency, so the source gate verifies the backport
without a blanket advisory ignore. Other unsoundness advisories remain denied.
The added `tests/variant_str_iter_backport.rs` fixture exercises all
five affected iterator entry points. Run it in an optimized build to cover the
original failure:

```sh
cargo test --locked --release -p twi-glib-backport-test
```

Remove the local patch and its source gate when Tauri's Linux dependencies
support a compatible released crate containing the fix.
