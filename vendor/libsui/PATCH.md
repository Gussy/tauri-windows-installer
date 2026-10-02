# libsui 0.13.0 — Local Patch

Vendored from [denoland/sui](https://github.com/denoland/sui) v0.13.0.

## What was changed

Changes in `lib.rs`:

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

### 2. Preserve native resources while embedding the application

`PortableExecutable::from` retains all original resources, including RT_MANIFEST
and custom stub icons. The Windows loader therefore keeps the requested
`asInvoker` execution level after bundling. `set_icon` explicitly replaces both
icon resource types when an override is requested. Opaque icon and group payloads
are compared after a resource expansion and complete bundle in regression tests.
Malformed editpe parser panics are converted to errors.

### 3. Sort every resource directory before serialization

Windows `FindResource` uses binary search on the resource directory, so entries
must be sorted. The upstream code inserts RT_ICON, RT_GROUP_ICON,
and RT_RCDATA in an order that happens to work, but adding RT_VERSION via
`resource_dir_mut()` breaks the sort (RT_VERSION=16 gets inserted before
RT_GROUP_ICON=14). The fix recursively sorts named entries first by UTF-16 code
units, then numeric entries by ID, before calling `set_resource_directory()`.

### 4. Correct the final resource section's loader bounds

When enlarging an existing final resource section, editpe 0.1 can update its raw
size without expanding VirtualSize, and leave SizeOfImage unaligned. After
serialization, `normalize_pe_image_size` ensures the resource section covers its
data directory and rounds the image size to SectionAlignment. Checked arithmetic
rejects overflow. Regression fixtures in `core/src/bundle.rs` preserve the Windows
manifest and grow a resource by 64 KiB to verify these loader bounds.

## Removal

Remove this patch only after upstream preserves native resources, provides a way
to set version info, sorts all resource directories, and handles expanded PE
sections correctly. Keep the manifest and growth regression fixtures. Track
https://github.com/denoland/sui for updates, and remove the `[patch.crates-io]`
entry in the workspace `Cargo.toml` when upgrading.
