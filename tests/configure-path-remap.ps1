# Strip build-host paths (workspace checkout, CARGO_HOME, user profile) from
# release binaries. Rust panic locations are remapped with --remap-path-prefix;
# MSVC `__FILE__` strings from C dependencies (aws-lc-sys) are trimmed with
# /d1trimfile. cl.exe can see both the long and the 8.3 short form of a path,
# so both are mapped.
#
# Exports the flags to $GITHUB_ENV when running in GitHub Actions, otherwise to
# the current process. Flags are scoped to the MSVC target so they join the
# `target.'cfg(...)'.rustflags` in .cargo/config.toml instead of replacing them.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$roots = @(
    (Join-Path $cargoHome 'registry\src'),
    (Join-Path $cargoHome 'git\checkouts'),
    $workspace
)

$fso = New-Object -ComObject Scripting.FileSystemObject
$prefixes = foreach ($root in $roots) {
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    $long = (Resolve-Path $root).Path.TrimEnd('\')
    $long
    $fso.GetFolder($long).ShortPath.TrimEnd('\')
}
$prefixes = @($prefixes | Select-Object -Unique)
foreach ($prefix in $prefixes) {
    # Cargo and cc split these variables on whitespace.
    if ($prefix -match '\s') { throw "Cannot remap a build path containing whitespace: $prefix" }
}

$rustflags = ($prefixes | ForEach-Object { "--remap-path-prefix=$_=" }) -join ' '
$cflags = ($prefixes | ForEach-Object { "/d1trimfile:$_\" }) -join ' '
$vars = [ordered]@{
    CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $rustflags
    CFLAGS_x86_64_pc_windows_msvc                 = $cflags
    CXXFLAGS_x86_64_pc_windows_msvc               = $cflags
    # Read by verify-release.ps1 to prove the mapping took effect.
    TWI_REMAPPED_PATHS                            = ($prefixes -join ';')
}
foreach ($name in $vars.Keys) {
    if ($env:GITHUB_ENV) {
        "$name=$($vars[$name])" | Add-Content -LiteralPath $env:GITHUB_ENV -Encoding utf8
    }
    Set-Item -Path "env:$name" -Value $vars[$name]
    Write-Host "$name=$($vars[$name])"
}
