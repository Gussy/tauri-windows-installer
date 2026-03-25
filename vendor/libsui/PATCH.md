# libsui 0.13.0 — Local Patch

Vendored from [denoland/sui](https://github.com/denoland/sui) v0.13.0.

## What was changed

Two changes in `lib.rs`:

### 1. Added `resource_dir_mut()` accessor

```rust
pub fn resource_dir_mut(&mut self) -> &mut editpe::ResourceDirectory {
    &mut self.resource_dir
}
```

We need to set PE version info (product name, version, publisher) on the setup
executable so it appears in Windows Explorer's Properties dialog. The upstream
API only exposes `set_icon()` and `write_resource()` — there is no way to add
RT_VERSION resources. Adding version info after `build()` via editpe's
`set_resource_directory()` corrupts icon resources due to an editpe round-trip
serialization bug.

By exposing the internal resource directory, we can call editpe's
`set_version_info()` directly on it before `build()`. This way icons, custom
resources, and version info are all serialized in a single pass — avoiding the
round-trip corruption entirely.

### 2. Sort root resource table entries by type ID in `build()`

Windows `FindResource` uses binary search on the resource directory, so entries
must be sorted by type ID. The upstream code inserts RT_ICON, RT_GROUP_ICON,
and RT_RCDATA in an order that happens to work, but adding RT_VERSION via
`resource_dir_mut()` breaks the sort (RT_VERSION=16 gets inserted before
RT_GROUP_ICON=14). The fix sorts all root entries by type ID before calling
`set_resource_directory()`.

## Removal

This patch can be removed once upstream libsui exposes a way to set version
info or provides mutable access to the resource directory. Track
https://github.com/denoland/sui for updates, and remove the `[patch.crates-io]`
entry in the workspace `Cargo.toml` when upgrading.
