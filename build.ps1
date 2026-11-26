param([switch]$Test, [switch]$NativeFlow)
# Optional convenience wrapper only; the application, resource builder and tests are Rust.
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
$oldPath = $env:Path; $oldHome = $env:CARGO_HOME
try {
    $t = Join-Path $PSScriptRoot '.toolchain\rustup\toolchains\stable-x86_64-pc-windows-gnu'
    if (Test-Path "$t\bin\cargo.exe") {
        $env:Path = "$t\bin;$t\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained;$env:Path"
        $env:CARGO_HOME = Join-Path $PSScriptRoot '.toolchain\cargo'
    } elseif (!(Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Install x64 Rust for Windows, or restore the project-local toolchain.' }
    if ($Test) { cargo test --locked; if ($LASTEXITCODE) { throw 'Rust tests failed' } }
    if ($NativeFlow) { cargo test --locked --release --test native_flow -- --ignored; if ($LASTEXITCODE) { throw 'Native GUI integration test failed' } }
    cargo build --release --locked; if ($LASTEXITCODE) { throw 'Rust release build failed' }
    Copy-Item 'target\release\Relay.exe' '.\Relay.exe' -Force
    Write-Host "Saved $PSScriptRoot\Relay.exe"
} finally { $env:Path=$oldPath; $env:CARGO_HOME=$oldHome; Pop-Location }
