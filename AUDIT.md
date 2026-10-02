# Tauri Windows Installer Robustness Audit

**Remediation update:** The implementation fixes for findings 1–23 and the dependency findings are included in v0.2.0. See [Remediation and verification](#remediation-and-verification) below. The original evidence is retained here as the pre-fix baseline; its file line numbers refer to that baseline.

Audit date: October 1, 2026. Reviewed commit: `dd6d0fc`. Scope: installer, uninstaller, core bundle format, CLI packaging, vendored PE handling, CI and release workflows, VM test harness, and locked Rust dependencies.

The happy path is covered by passing Rust tests, but the installer has several concrete failure paths that can lose a working installation or leave cleanup impossible. The most urgent changes are to constrain destructive filesystem operations, serialize installation and uninstall, introduce a recoverable installation transaction, and repair the automated test runner. Testing success reported by the current shell harness is unreliable because failed assertions can still produce a passing suite result.

Three independent agents reviewed runtime behavior, packaging and core, and tests and release behavior. The primary review checked their findings, ran local Rust checks and dependency auditing, and consolidated the results below. This audit changed no application, installer, or test source files.

## Verification and limits

| Check | Result |
| --- | --- |
| Locked default Rust tests | 25 unit tests and 1 doctest passed |
| Core Clippy with bundler feature and warnings denied | Passed |
| Bundler Clippy with warnings denied | Passed |
| Rust formatting across workspace source files | Passed |
| Bash syntax for all test scripts | Passed |
| Packaging probes using exact compiled dependencies | Confirmed manifest loss, unsafe accepted inputs, partial configuration failures, signing argument corruption, and poisoned WebView2 cache |
| Isolated build script reproduction | Confirmed a rebuilt bundler can retain an older installer stub |
| Mocked shell test execution | Confirmed five failed assertions can produce a passing test status |
| RustSec scan of workspace Cargo.lock | 18 vulnerability advisory matches and 16 warnings |
| Native Windows execution and VM end to end suite | Not run |

Local compilation used the installed Rust 1.94.1 toolchain, rather than the pinned 1.93.1. No Windows target toolchain, UTM CLI, or QEMU image tool was available in this environment. Findings described as source traces follow deterministically from the code; Windows filesystem, process, and UI scenarios still require native regression tests. The audit did not establish exploitability of every advisory match.

P1 means address before relying on production durability. P2 means a concrete correctness issue or material compatibility gap. These priorities describe this project, not CVSS security ratings.

The [packaging probe source](/Users/gus/Development/tauri-windows-installer/audit-artifacts/packaging-probe.rs) and [recorded output](/Users/gus/Development/tauri-windows-installer/audit-artifacts/packaging-probe-output.txt) preserve the isolated validation evidence. These are audit artifacts, not regression tests integrated into the project.

## Findings requiring immediate attention

### 1 Invalid application identifiers can target unrelated directories

**P1.** [Installer path construction](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:65), [bundle manifest construction](/Users/gus/Development/tauri-windows-installer/core/src/bundle.rs:202).

The identifier is joined directly onto LocalAppData and Programs without validation or a containment check. An empty identifier targets the entire `%LOCALAPPDATA%\Programs` directory. The overwrite branch can then terminate its descendant application processes, rename that directory, and recursively delete the backup. A parent component or absolute path can target other user directories.

An isolated packaging probe confirmed that both the public bundle API and Tauri configuration deserialization accept empty, parent, and absolute identifiers. This is a configuration validation defect; it does not require a claim that an attacker can modify a signed installer.

Require a nonempty valid identifier at bundling and runtime. Before destructive operations, verify that the resolved installation is a dedicated immediate child of the expected Programs directory. Reject unsafe names, titles, executable paths, and WebView2 filenames too. Account for Windows path rules and reparse points.

### 2 Installation and uninstall have no shared lock

**P1.** [Installation directory preparation](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:129), [uninstall entry point](/Users/gus/Development/tauri-windows-installer/uninstaller/src/lib.rs:29).

Two fresh installers can both observe an empty directory. One can call `ensure_empty_dir` while the other is extracting, deleting the other process's files. Overlapping upgrades can rename a partial installation, overwrite metadata, or remove a backup another process needs. An uninstall can similarly race an installation.

Use a per-user, per-identifier mutex shared by both operations, and acquire it before changing directories, terminating application processes, or writing registration. Hold it through the transaction. Define how a second invocation exits or waits.

### 3 The working backup is deleted before installation is committed

**P1.** [Backup removal](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:209), [registration and metadata](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:215).

After extraction, the installer deletes the old installation before registry updates, shortcut creation, metadata writing, and application launch. Failure during any later required step leaves the previous working installation unavailable. For example, a redirected Desktop denying shortcut creation returns before uninstall metadata is written.

Keep the backup until executable validation, metadata, and required registration succeed. Roll back required failure paths consistently. Decide separately whether optional shortcut or launch failures should invalidate an otherwise completed installation.

### 4 Preparation errors and interruptions have no recovery path

**P1.** [Live directory rename](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:163), [preparation](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:185), [rollback](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:193).

The live directory is moved before decompression and extraction. Preparation failure returns through `?` without rollback. Process termination, panic, or memory exhaustion can leave an empty or partial live root and the old installation under a random backup name. A later run has no transaction record with which to recover it.

Even extraction rollback ignores failures when removing the partial root and restoring the backup. It logs that rollback was attempted, without recording whether it succeeded.

Extract and validate into a unique sibling staging directory before touching the live root. Persist sufficient transaction state to recover on the next invocation. Check restoration errors and keep the last known good directory until recovery or commit is confirmed.

### 5 Missing WebView2 causes an unbundled installer to panic

**P1.** [Prerequisite handling](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:54), [bootstrapper write](/Users/gus/Development/tauri-windows-installer/installer/src/bundle/webview2.rs:57).

The bundler supports packages without a WebView2 bootstrapper. When runtime detection fails, setup nevertheless calls `install()`, which unwraps absent bootstrapper data. A valid package therefore panics on a machine without WebView2.

Handle the missing runtime and missing bootstrapper combination explicitly. Choose a clear prerequisite error, an intentional download policy, or support for a packaged fixed runtime. Start logging before this step. Microsoft distinguishes online bootstrapper deployment from standalone offline deployment in its [WebView2 distribution guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution).

### 6 The shell test runner can report failures as passes

**P1.** [Test invocation](/Users/gus/Development/tauri-windows-installer/tests/run.sh:83), [assertion status](/Users/gus/Development/tauri-windows-installer/tests/helpers/assert.sh:8).

The complete test subshell runs on the left side of `||`. Bash disables `errexit` in that context, including within its functions. Failed installer commands or assertions do not stop the test. A successful final command makes the whole test pass. Assertion counters remain inside the subshell and are never used to decide the outcome.

A safe mocked reproduction using the actual fresh-install test printed five failed assertions yet returned `test_result=0`; the parent's failure counter remained zero.

Run each test as a separate Bash process that explicitly returns failure, or propagate every failure. Add a regression test where an early failed assertion followed by a successful command must fail the suite. This finding concerns the repository's automated evidence and does not invalidate manual Windows testing.

## Additional concrete defects

### 7 Packaging removes the embedded Windows manifest

**P2.** [PE initialization](/Users/gus/Development/tauri-windows-installer/vendor/libsui/lib.rs:115), [resource replacement](/Users/gus/Development/tauri-windows-installer/vendor/libsui/lib.rs:309).

The vendored library initializes a new empty resource directory instead of retaining the stub's resources. Building then replaces the original directory. A probe constructed a PE with an embedded manifest; after resource embedding its manifest was absent.

Preserve the existing resource directory and merge the new resources into it. Verify the final packaged executable retains its execution level and other compatibility settings. The underlying [editpe API](https://docs.rs/editpe/0.1.0/editpe/struct.Image.html) replaces the resource directory.

The [installer build script](/Users/gus/Development/tauri-windows-installer/installer/build.rs:4) also calls `new_manifest("app.manifest")`, which generates a manifest with that identity instead of reading the checked-in XML. Its host-only conditional omits that embedding during a macOS cross-build. Treat the intended manifest, build output, and final package as one verified pipeline.

### 8 Rebuilding setup can leave an older stub embedded

**P2.** [Stub lookup and change tracking](/Users/gus/Development/tauri-windows-installer/bundler/build.rs:38).

The build script reads `target/<profile>/setup.exe`, but its rerun directives watch the copied `bundler/setup.exe`. Changing the source artifact does not trigger the script.

An isolated reproduction embedded `stub_v1`, replaced the source artifact with `stub_v2`, then rebuilt successfully while the output still contained `stub_v1`. Installer fixes can consequently be absent from the bundler that distributes them.

Track the actual source artifact or provide an explicit stub input path. Verify its hash and format version in packaging and release checks.

### 9 Uninstall removes recovery information before cleanup is assured

**P2.** [Uninstall cleanup](/Users/gus/Development/tauri-windows-installer/uninstaller/src/lib.rs:57), [rename and self deletion](/Users/gus/Development/tauri-windows-installer/uninstaller/src/lib.rs:103), [process termination](/Users/gus/Development/tauri-windows-installer/uninstaller/src/lib.rs:148).

Termination results are ignored and the uninstaller does not wait for processes to exit. A content-removal failure merely sets a flag; metadata, shortcut, and registry cleanup can continue, followed by directory rename and self-deletion. Residual locked files can be left without a usable uninstall entry or metadata.

The uninstaller also does not move its working directory outside the installation before renaming it. Launching it with that working directory prevents the rename under Windows' documented [current-directory lock](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setcurrentdirectory). Earlier cleanup has already removed recovery information, and retrying fails to read metadata.

Wait for relevant processes, move cwd first, retain durable cleanup state, and preserve a retry mechanism until deletion succeeds. The delayed script should check that the directory was removed, not only that the executable disappeared.

### 10 Nested executable layouts cannot uninstall

**P2.** [Main executable validation](/Users/gus/Development/tauri-windows-installer/core/src/bundle.rs:161), [metadata write](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:240), [uninstaller root discovery](/Users/gus/Development/tauri-windows-installer/uninstaller/src/lib.rs:30).

The bundler accepts `bin/app.exe` and setup writes metadata at the installation root. The uninstaller derives its root from the executable's immediate parent, so it searches `<root>/bin/.twi-meta.json` and exits with failure.

Either explicitly restrict the main executable to the root or provide a validated installation-root discovery mechanism. The same bundler check only tests existence: probes also accepted `.`, an empty name, and `../outside.exe`. Require an actual bundled executable within the permitted root, not merely an existing path.

### 11 Disk capacity checks underestimate installation requirements

**P2.** [Required size](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:91), [Windows capacity query](/Users/gus/Development/tauri-windows-installer/installer/src/windows.rs:155), [decompression](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:263).

Required space equals compressed archive length. Highly compressible files can therefore pass preflight and fill the disk during extraction. The Windows query reads total volume free bytes rather than quota-aware bytes available to the caller.

Embed or calculate unpacked requirements, allow overhead, and use `lpFreeBytesAvailableToCaller`, as defined by [GetDiskFreeSpaceExW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw).

Decompression also reads the entire tar into memory after copying the compressed resource into a vector. A large bundle can exhaust memory after the working install has been moved. Stream with explicit size limits into staging and verify the full payload before commit.

### 12 WebView2 uses a shared predictable temporary executable

**P2.** [Temporary bootstrapper execution](/Users/gus/Development/tauri-windows-installer/installer/src/bundle/webview2.rs:55).

Every setup writes the same bootstrapper filename under TEMP. Installers for different applications can collide even after adding a per-app mutex. A running bootstrapper can cause the next write to fail and panic; overlapping writes can leave an invalid executable. The temporary executable is not cleaned up.

Create an exclusive temporary directory for each invocation, keep it until the child exits, propagate errors, and remove it afterward. Verify runtime availability after bootstrapper completion and define a timeout policy.

### 13 Failed WebView2 downloads poison the persistent build cache

**P2.** [Download and cache](/Users/gus/Development/tauri-windows-installer/bundler/src/webview2.rs:27).

The downloader does not reject HTTP error statuses or validate the executable. It writes directly to the final cache file and subsequently trusts existence. A probe returned HTTP 503 with an HTML body; the code returned success and cached the HTML as an executable. A later call with the server stopped reused the same HTML. An empty cache file was also accepted without redownload.

Reject unsuccessful status codes, validate expected content and provenance, use atomic cache publication, and replace invalid cached data. Add network timeouts and cache concurrency handling.

The two downloader tests modify the global `CACHE_DIR` environment variable while tests can run in parallel. Their pass in this audit does not remove that race. Pass cache paths as explicit inputs.

### 14 Initial application launch uses TEMP as its working directory

**P2.** [Safe installer cwd](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:87), [application spawn](/Users/gus/Development/tauri-windows-installer/installer/src/process.rs:101).

The installer changes cwd to TEMP and spawns the app without setting the child's working directory. The desktop shortcut instead sets cwd to the installation root. Relative asset and sidecar lookups can therefore work from the shortcut and fail during initial launch.

Set the child working directory explicitly to the intended application root.

### 15 Shortcut ownership is lost when titles or options change

**P2.** [Existing shortcut detection](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:134), [new metadata](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:232).

An upgrade checks only the new title's shortcut filename. A title change leaves the old link untouched, skips the new link, and records that no shortcut was created. Uninstall can no longer clean up the old link. Changing the executable filename can also make that remaining link broken. Disabling shortcut creation similarly loses the ownership state of an existing link.

Read prior metadata and track the shortcut actually owned by the installation. Update or remove that owned path deliberately during upgrade.

### 16 Setup can terminate itself when launched from the installation

**P2.** [Process filtering](/Users/gus/Development/tauri-windows-installer/installer/src/process.rs:16).

The installer kills every executable beneath the installation root, including its own PID. Launching a downloaded or copied setup from inside that directory can terminate setup before the upgrade proceeds.

Exclude the current PID in both kill and wait filters. Handle the setup file itself when deciding whether the directory can be replaced.

### 17 Release failures are invisible and logging is incomplete

**P2.** [Release entry point](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:2), [terminal error reporting](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:19), [logging start](/Users/gus/Development/tauri-windows-installer/installer/src/main.rs:75).

The release executable hides its console, but terminal failures are written only to stderr. Logs begin after manifest parsing and prerequisite handling. Registry, shortcut, metadata, and launch errors also have no central terminal log entry. A click can fail, possibly after stopping the running app, without a visible explanation or complete log.

Initialize logging early, log every terminal error and transaction state, and display a concise error with the log location. Replace panic-based resource and Windows API reads with errors.

### 18 Partial plugin configurations fail to deserialize

**P2.** [Plugin configuration](/Users/gus/Development/tauri-windows-installer/bundler/src/plugin_config.rs:5).

The plugin struct derives Default, but its `webview2` field has no Serde default. A configuration containing only `desktopShortcut`, only `icon`, or an empty plugin object fails with a missing-field error. The loader converts the error into a panic.

Probes confirmed all three cases. Add field defaults for optional configuration and return actionable configuration errors.

### 19 Signing configuration corrupts Windows paths and arguments

**P2.** [Tauri signing resolution](/Users/gus/Development/tauri-windows-installer/bundler/src/main.rs:145), [command parsing](/Users/gus/Development/tauri-windows-installer/core/src/bundle.rs:372).

Structured signing arguments are joined into a string, then parsed with Unix shell quoting. Windows backslashes, executable paths with spaces, and arguments containing spaces lose their original boundaries. The Tauri `%1` placeholder is removed and the output path is appended at the end, changing commands whose binary argument must appear earlier or within an option.

A probe using the actual resolution code transformed a structured executable path under `C:\Program Files\Signer` and a two-word password into separate, corrupted tokens. `--input="%1" --quiet` became an empty input argument followed by the binary at the end.

Retain a program and argument vector, substitute placeholders in place, and define a compatible parsing contract for legacy string commands. Validate signing output before declaring packaging successful.

### 20 The macOS CLI embeds an empty setup stub

**P2 compatibility gap.** [Non-Windows stub path](/Users/gus/Development/tauri-windows-installer/bundler/build.rs:19).

The non-Windows branch always writes an empty placeholder and returns, even if a real Windows stub was supplied in the bundler directory. Compilation and tests can succeed, but a normal packaging invocation later fails to parse the empty PE.

If packaging on macOS is intended, accept an explicit valid Windows stub. Otherwise make the development-only limitation clear at the CLI entry point. The library API can already receive actual stub bytes directly.

### 21 The manual demo build workflow fails on a clean checkout

**P2.** [Build order](/Users/gus/Development/tauri-windows-installer/.github/workflows/build.yaml:56), [workspace defaults](/Users/gus/Development/tauri-windows-installer/Cargo.toml:23).

The workflow runs the default release build and then copies setup.exe. Default members are core and bundler; the installer is not built. The bundler meanwhile requires setup.exe during its own compilation, before the later copy.

Build the installer explicitly, then build the bundler from that artifact. Exercise this order from a clean checkout.

### 22 Manual releases can validate a different revision

**P2.** [Reusable CI invocation](/Users/gus/Development/tauri-windows-installer/.github/workflows/release.yaml:70), [CI checkouts](/Users/gus/Development/tauri-windows-installer/.github/workflows/ci.yaml:19).

A manually dispatched release resolves the requested tag and builds that tag, but reusable CI receives no resolved ref. Its default checkout uses the triggering workflow ref. Dispatching on main for an older tag can consequently validate main and publish the tag's code.

Resolve the tag to a commit SHA and pass that same SHA through version validation, reusable CI, build, and release verification.

### 23 The VM harness targets incorrect paths and CLI forms

**P2.** [Install path](/Users/gus/Development/tauri-windows-installer/tests/lib.sh:23), [PowerShell assertions](/Users/gus/Development/tauri-windows-installer/tests/helpers/assert.sh:24), [VM commands](/Users/gus/Development/tauri-windows-installer/tests/helpers/vm.sh:83).

The harness expects LocalAppData/identifier, whereas setup now installs under LocalAppData/Programs/identifier. It then surrounds the PowerShell environment-variable expression with single quotes, preventing expansion. The uninstall test sends the same PowerShell expression to cmd.exe, which does not interpret it.

The current [official UTM CLI](https://github.com/utmapp/UTM/blob/main/utmctl/UTMCtl.swift) also expects guest commands under `--cmd` and file push/pull through stdin/stdout; the helpers use incompatible forms. These CLI discrepancies were checked against current upstream source, rather than an installed binary.

Resolve the guest installation directory into a concrete path, use it safely across all helpers, and validate commands against a supported UTM version. Fix this alongside the runner's failure propagation before trusting the suite.

## Dependency findings

The complete [machine-readable RustSec report](/Users/gus/Development/tauri-windows-installer/audit-artifacts/dependency-audit-2026-10-01.json) records the database revision, all package versions, advisory metadata, and warnings. Its database was updated on October 1, 2026.

The 18 vulnerability advisory matches span 12 package versions. They cover the full workspace lockfile, including build tools, demo dependencies, optional dependencies, and other platforms. Exposure must be evaluated per shipped artifact and affected API.

Prioritize the archive parser used by setup and the network stack used by the bundler:

| Package in lockfile | Advisory matches | Patched baseline reported by RustSec | Relevance |
| --- | --- | --- | --- |
| tar 0.4.44 | RUSTSEC-2026-0067 and 0068 | 0.4.45 | Setup directly parses and unpacks the bundled archive |
| rustls 0.23.37 | RUSTSEC-2026-0285 | 0.23.45 | Bundler download dependency |
| rustls-webpki 0.103.9 | RUSTSEC-2026-0049, 0098, 0099, 0104 | 0.103.13 in the same stable series | Bundler TLS dependency; assess actual verifier paths |
| aws-lc-sys 0.38.0 | RUSTSEC-2026-0044 and 0048 | 0.39.0 | Crypto backend in download graph; assess affected validation paths |
| h2 0.4.13 | RUSTSEC-2026-0258 | 0.4.16 | Bundler HTTP dependency |
| bytes 1.7.1 | RUSTSEC-2026-0007 | 1.11.1 | Shared network infrastructure |

The remaining matched packages are crossbeam-channel 0.5.13, idna 0.5.0, quick-xml 0.32.0, quinn-proto 0.11.14, remove_dir_all 0.5.3, and time 0.3.36. In particular, remove_dir_all 0.5.3 is distinct from the project's direct remove_dir_all_ext 0.8.4; do not conflate them.

RustSec documents the two tar issues in [RUSTSEC-2026-0067](https://rustsec.org/advisories/RUSTSEC-2026-0067.html) and [RUSTSEC-2026-0068](https://rustsec.org/advisories/RUSTSEC-2026-0068.html). Their prerequisites differ, and this audit did not demonstrate exploitation against a normal generated Windows package.

There are also 10 maintenance warnings and 6 soundness warnings. Direct dependencies include bincode, which is [unmaintained](https://rustsec.org/advisories/RUSTSEC-2025-0141.html), and deprecated tempdir, which introduces an older remove_dir_all. Remove unused dependencies, update compatible locked versions, and schedule migration of the binary format deliberately. Add dependency scanning to CI and require explicit, reasoned exceptions.

## Durability design recommendation

Implement the following sequence as a recoverable state machine:

1. Parse and validate the package and all filesystem inputs before side effects.
2. Acquire the shared application lock and recover any prior interrupted transaction.
3. Check prerequisites and available storage.
4. Extract into an exclusive sibling staging directory with bounded streaming.
5. Validate the archive, expected executable, required files, and metadata.
6. Stop the current app and wait for relevant processes and locks.
7. Record transaction state and perform the directory swap, retaining the old copy.
8. Write required registration and commit state, with recovery for partial writes.
9. Launch with an explicit working directory and report optional failures clearly.
10. Remove the backup only after commit; retain actionable cleanup state if removal fails.

Use the same lock and durable ownership record for uninstall. Preserve application data outside managed binaries. Record enough state to recover file, metadata, and registry consistency after termination or power loss.

Define and version the manifest schema before changing serialization. The installer stub and bundled manifest currently have no explicit compatibility handshake. Bind a known stub hash and schema version to release artifacts.

## Tests needed to establish durability

| Scenario | Required assertion |
| --- | --- |
| Early failed test assertion | Runner returns failure even when later commands succeed |
| Concurrent install and uninstall | One serialized operation owns the application; no mixed files or registration |
| Termination at every installation phase | Next invocation recovers either the old complete version or the committed new one |
| Denied preparation, metadata, registry, or shortcut writes | Previous version and uninstall capability remain recoverable |
| Corrupt or truncated payload and invalid executable | Rejected before destroying the working installation |
| Low disk capacity, quota, and highly compressed payload | Accurate failure with intact prior installation |
| Missing WebView2 with and without bootstrapper | Explicit expected outcome, no panic |
| Failed HTTP response and interrupted cache write | Invalid bootstrapper is rejected and cache can recover |
| Locked files and uninstall from install cwd | Cleanup completes or remains retryable with diagnostic state |
| Root and nested main executables | Supported layouts uninstall completely; unsupported layouts fail during bundling |
| Real version one to version two upgrade | New and removed sidecars, metadata, registry version, and preserved user data are verified |
| Title, executable, and shortcut option changes | Owned shortcuts are updated or removed correctly |
| Running-process replacement | Original PID exits; a separately identified new process launches |
| Self-deletion completion | Original and renamed uninstall directories, executable, and owned shortcuts disappear |
| Fresh build and stub rebuild | Final package embeds the intended setup hash and Windows manifest |
| Release by manually selected tag | CI and published artifact use the identical resolved commit |

Continuous CI currently runs core and bundler checks on Linux; the installer and uninstaller's Windows branches have no normal CI validation. Add Windows compilation and focused runtime checks.

The process-kill test checks files and registry instead of the original PID. The uninstall test checks only the original directory, which disappears immediately when renamed, and can miss abandoned cleanup directories. Repair these assertions.

The Packer fixture [disables Defender and UAC](/Users/gus/Development/tauri-windows-installer/packer/scripts/provision.ps1:47). Retain an isolated convenience fixture if useful, but add standard-user tests with default security settings, plus supported Windows 10 and 11 versions. Include Unicode and spaces in profile paths, redirected desktops, offline operation, and locked DLLs.

VM stop failures are suppressed before snapshot restore, and the standalone snapshot task does not initialize the defaults in lib.sh. Require verified stopped state before touching a disk image.

## Lower priority hardening

- Return errors instead of panicking when resources, known folders, directory reads, and folder size queries fail.
- Free SHGetKnownFolderPath allocations and use a COM initialization guard that releases interface objects before uninitialization.
- Compute EstimatedSize in KiB before narrowing to u32; the present bytes-first cast wraps above 4 GiB.
- Parse semantic version components deliberately; the current filter_map silently drops invalid or prerelease components in numeric PE versions.
- Build and sign into temporary output files, then publish atomically. A failure currently truncates or leaves a partial or unsigned final-named artifact.
- Check the final signature and retained resources, rather than relying only on a signing command's exit status.
- Reduce the CLI's configuration dependency to the necessary Tauri utilities where practical, avoiding unrelated GUI runtime dependencies.
- Update documentation to use the current Programs install directory, correct library options, supported build procedures, and realistic WebView2 availability. Clarify supported silent invocation and launch behavior.

## Remediation and verification

The primary agent and three implementation agents fixed and cross-reviewed runtime transactions, packaging/core, and tests/build/release behavior for v0.2.0. The table maps every numbered finding to its implemented fix; native Windows validation is required before release through the CI gate.

| Finding | Implemented change | Main source / regression |
| --- | --- | --- |
| 1 Unsafe identifiers | Validate Windows names and confined install roots; reject reparse roots and unmanaged overwrite | [Shared validation](core/src/validation.rs), [setup](installer/src/main.rs) |
| 2 Concurrent operations | Shared per-app OS file lock; standalone cleanup takes the same lock | [Lock](core/src/lock.rs), [cleanup worker](uninstaller/src/cleanup.ps1) |
| 3 Premature backup deletion | Keep backup through metadata, registry, shortcut and requested launch; persist commit before cleanup | [Transaction](installer/src/transaction.rs) |
| 4 Interrupted preparation / rollback | Stage before swapping; persist phases and external-state snapshots; retry interrupted rollback; recover invalid committed tree from a valid backup | [Transaction regressions](installer/src/transaction.rs) |
| 5 Missing WebView2 panic | Return a clear prerequisite error when both runtime and bootstrapper are absent | [Prerequisite handling](installer/src/bundle/webview2.rs) |
| 6 False test passes | Run each shell test in a separate process with enforced failure propagation | [Runner](tests/run.sh), [mock regressions](tests/harness-regression.sh) |
| 7 Lost PE manifest | Preserve native resources, including custom icons; sort tables and correct expanded PE image bounds; embed checked-in XML for Windows targets | [PE writer](vendor/libsui/lib.rs), [packaging tests](core/src/bundle.rs), [manifest build](installer/build.rs) |
| 8 Stale embedded stub | Track actual artifact and explicit input changes; enforce ABI and bind stub hash | [Build script](bundler/build.rs), [release checks](tests/verify-release.ps1) |
| 9 Non-retryable uninstall | Retain standalone worker, bounded recovery state and registration until cleanup succeeds; wait for processes; acknowledge worker startup before parent exits | [Uninstall preparation](uninstaller/src/windows_impl.rs), [worker](uninstaller/src/cleanup.ps1), [native retry test](tests/windows-e2e.ps1) |
| 10 Nested / unsafe executable | Validate relative executable and PE; discover a nested app through checked root ownership | [Shared validation](core/src/validation.rs), [executable validation](installer/src/executable.rs) |
| 11 Incorrect storage / memory estimates | Quota-aware free bytes; filesystem allocation and snapshot overhead; bounded streaming extraction and declared-size checks | [Archive](installer/src/archive.rs), [storage query](installer/src/windows.rs) |
| 12 Shared temporary bootstrapper | Exclusive temporary directory, timeout, Microsoft Authenticode verification and runtime postcheck | [WebView2 runtime](installer/src/bundle/webview2.rs) |
| 13 Poisoned download cache | Reject HTTP failures and invalid executables; use isolated cache lock, timeouts and atomic publication; tests use explicit paths | [Downloader and tests](bundler/src/webview2.rs) |
| 14 Wrong launch cwd | Set child cwd to the install root and assert it for an actual installer-launched process | [Process launch](installer/src/process.rs), [Windows suite](tests/windows-e2e.ps1) |
| 15 Lost shortcut ownership | Persist owned title; check target ownership; update/remove owned shortcuts across title, executable and option changes | [Metadata](core/src/manifest.rs), [shortcut handling](installer/src/windows.rs) |
| 16 Setup kills itself | Exclude self PID; use a trusted cwd outside the managed tree; retain cleanup journal when setup itself holds the backup | [Processes](installer/src/process.rs), [setup](installer/src/main.rs) |
| 17 Invisible failures | Start unique retained logging immediately; report failures with Windows dialogs unless silent; return nonzero status | [Setup entry](installer/src/main.rs), [uninstall entry](uninstaller/src/windows_impl.rs) |
| 18 Partial config panic | Default missing plugin fields and return configuration errors | [Configuration tests](bundler/src/plugin_config.rs) |
| 19 Signing argument corruption | Keep structured program/args; preserve legacy Windows backslashes; replace `%1` in place; clear stale stub signatures and verify final resources/certificate structure | [Signing and tests](core/src/bundle.rs), [release signature verification](tests/sign-release.ps1) |
| 20 Empty macOS stub | Accept explicit `TWI_SETUP_EXE` / `--setup-exe`; development builds without a stub report a clear packaging error | [Stub selection](bundler/src/main.rs), [build script](bundler/build.rs) |
| 21 Broken clean build order | Build setup first, then embed that exact stub into bundler | [Demo workflow](.github/workflows/build.yaml) |
| 22 Release revision mismatch | Resolve tag to SHA and pass it through validation, reusable CI, artifact creation and provenance | [Release workflow](.github/workflows/release.yaml) |
| 23 VM paths / CLI mismatch | Concrete Programs paths, current UTM command forms, checked exit codes and stopped-only snapshots | [VM helpers](tests/helpers/vm.sh), [harness regressions](tests/harness-regression.sh) |

The dependency lockfile was refreshed with Rust 1.93.1. Unused/deprecated direct dependencies were removed, the manifest moved to a bounded versioned JSON envelope with an explicit legacy decoder, and the bundler now depends on Tauri utilities rather than the GUI runtime. The remaining Linux demo GLib issue received its exact upstream mutable-out-pointer backport while retaining the actual 0.18 API version. Both macro crates now use maintained `proc-macro-error3`. Provenance and licenses accompany the vendored patches, and [the source gate](tests/audit-dependencies.py) verifies the reviewed GLib source before auditing. There are no advisory exclusions.

Additional cross-review fixes align builder and runtime archive/PE acceptance, bound path depth and raw GNU entries, preserve custom stub icons, and make cleanup iterative and safe for deep/long paths and live/broken junctions. Failed retries retain pre-existing recovery workers. Known-folder allocations and COM initialization are balanced. Numeric file versions and EstimatedSize use checked conversions. The VM fixture retains UAC and Defender and runs application commands under a standard user.

| Post-fix check | Result |
| --- | --- |
| Compiler | Pinned Rust 1.93.1 |
| Locked integrated host Rust suite | 69 unit tests + 1 doctest passed |
| Strict host Clippy for shipping crates, all targets | Passed |
| Windows x64 target Clippy for setup and uninstall, all targets | Passed; includes Windows-only test compilation |
| Rust formatting and diff whitespace | Passed |
| Bash syntax, ShellCheck and mock harness regressions | Passed |
| PowerShell syntax, PE reader and bounded-state/deep-path/link guard regressions | Passed on PowerShell 7.5.2 on macOS |
| Vendored macro compilation / optimized GLib backport regression | Passed |
| Reviewed GLib source gate | Passed |
| RustSec 0.22.2 scan, October 1 advisory database | 0 vulnerabilities, 0 warnings; [report](audit-artifacts/dependency-audit-after-fixes.json) |
| Native Windows / Windows PowerShell 5.1 execution / actual VM suite | Not run on this host; workflows and suites added |

The native suite exercises real upgrade files/versions/PIDs, installer launch cwd, shared locks, held-directory rollback, corrupt payloads, interrupted journal recovery, repair, shortcut changes, nested uninstall, long/deep assets, junction safety, and standalone retry after a locked sidecar. Hosted Windows CI is an administrator account; the VM / `-RequireStandardUser` suite remains necessary for the standard-user claim. Low-disk/quota denial, ACL-denied registration, every real power-loss boundary, older supported Windows versions, and missing-runtime/offline combinations remain useful additional native scenarios. A simulated journal boundary and portable unit tests do not establish real power-loss behavior.

Rebuild the setup stub before bundler, and rebuild applications using the updated uninstall library. Previously distributed binaries cannot acquire these fixes retroactively. See [README](README.md), [architecture](ARCHITECTURE.md), and [signing](SIGNING.md) for current build, operation, test and release procedures.
