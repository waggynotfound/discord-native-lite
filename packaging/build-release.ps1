param([string]$Iscc, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$oldPath = $env:Path; $oldCargo = $env:CARGO_HOME
$utf8 = [Text.UTF8Encoding]::new($false)
Push-Location $root
try {
    # Prefer an existing project-local compiler without changing user-level configuration.
    $local = Join-Path $root '.toolchain\rustup\toolchains\stable-x86_64-pc-windows-gnu'
    if (Test-Path "$local\bin\cargo.exe") {
        $env:Path = "$local\bin;$local\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained;$env:Path"
        $env:CARGO_HOME = Join-Path $root '.toolchain\cargo'
    }
    if (!$Iscc) {
        $cmd = Get-Command ISCC.exe -ErrorAction SilentlyContinue
        $candidates = @($cmd.Source, "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe")
        $Iscc = $candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
    }
    if (!$Iscc -or !(Test-Path -LiteralPath $Iscc -PathType Leaf)) { throw 'Install Inno Setup 6.3+ or supply -Iscc <path to ISCC.exe>.' }
    $hostLine = rustc -vV | Where-Object { $_ -like 'host: *' }
    $target = "$hostLine" -replace '^host: ', ''
    if ($target -notmatch '^x86_64-pc-windows-(gnu|msvc)$') { throw 'Build using a native x64 Windows GNU or MSVC Rust toolchain.' }
    $sysroot = (rustc --print sysroot).Trim()
    $json = cargo metadata --locked --format-version 1 --filter-platform $target
    if ($LASTEXITCODE) { throw 'Failed to resolve locked Cargo metadata.' }
    $meta = ($json -join "`n") | ConvertFrom-Json
    $project = $meta.packages | Where-Object { $_.id -eq $meta.resolve.root }
    $version = $project.version
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Release version must be numeric X.Y.Z.' }
    if (!$OutputDirectory) { $OutputDirectory = Join-Path $root "dist\release\$version" }
    $out = [IO.Directory]::CreateDirectory($OutputDirectory).FullName
    $stage = [IO.Directory]::CreateDirectory((Join-Path $meta.target_directory ("installer-staging\" + [guid]::NewGuid().ToString('N')))).FullName
    $payload = [IO.Directory]::CreateDirectory((Join-Path $stage 'payload')).FullName
    $licenses = [IO.Directory]::CreateDirectory((Join-Path $payload 'licenses')).FullName

    cargo build --release --locked --target $target
    if ($LASTEXITCODE) { throw 'Release compilation failed.' }
    Copy-Item -LiteralPath (Join-Path $meta.target_directory "$target\release\Relay.exe") -Destination $payload
    foreach ($file in 'README.md','LICENSE','CONTRIBUTING.md') { Copy-Item -LiteralPath $file -Destination $payload }
    $images = [IO.Directory]::CreateDirectory((Join-Path $payload 'docs\images')).FullName
    foreach ($file in 'discord-six-processes.png','discord-three-processes.png') { Copy-Item -LiteralPath "docs\images\$file" -Destination $images }
    $notes = [Text.StringBuilder]::new()
    [void]$notes.AppendLine("Relay $version | Third-party notices").AppendLine()
    $ids = @($meta.resolve.nodes.id)
    foreach ($p in @($meta.packages | Where-Object { $_.source -and $_.id -in $ids } | Sort-Object name,version)) {
        $dir = Split-Path $p.manifest_path -Parent
        $files = @(Get-ChildItem -LiteralPath $dir -File | Where-Object { $_.Name -match '^(LICEN[SC]E|COPYING|NOTICE|UNLICENSE|COPYRIGHT)' } | Sort-Object Name)
        if (!$files.Count) { throw "No license text for $($p.name) $($p.version); review before shipping." }
        [void]$notes.AppendLine("=== $($p.name) $($p.version) | $($p.license) ===")
        foreach ($f in $files) { [void]$notes.AppendLine($f.Name).AppendLine([IO.File]::ReadAllText($f.FullName)).AppendLine() }
    }
    [IO.File]::WriteAllText((Join-Path $licenses 'CARGO-NOTICES.txt'), $notes.ToString(), $utf8)
    # License the linked standard library as well as crates; this file ships with rustc.
    Copy-Item -LiteralPath (Join-Path $sysroot 'share\doc\rust\COPYRIGHT-library.html') -Destination (Join-Path $licenses 'RUST-LIBRARY.html')
    if ($target.EndsWith('-gnu')) {
        $mingw = @(Get-ChildItem -LiteralPath (Join-Path $sysroot 'share\doc') -Recurse -File -Filter 'COPYING.RUNTIME' | Select-Object -First 1)
        if (!$mingw.Count) { throw 'GNU runtime license texts not available in this toolchain.' }
        foreach ($n in 'COPYING.RUNTIME','COPYING3') { Copy-Item -LiteralPath (Join-Path $mingw[0].DirectoryName $n) -Destination (Join-Path $licenses "GCC-$n.txt") }
    }
    Copy-Item -LiteralPath (Join-Path (Split-Path $Iscc -Parent) 'license.txt') -Destination (Join-Path $licenses 'INNO-SETUP.txt')

    $base = "Relay-$version-windows-x64"
    & $Iscc /Qp "/DAppVersion=$version" "/DPayloadDir=$payload" "/O$stage" (Join-Path $PSScriptRoot 'Relay.iss')
    if ($LASTEXITCODE -or !(Test-Path "$stage\$base-setup.exe")) { throw 'Setup compiler did not produce an installer.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($payload, "$stage\$base-portable.zip")
    $names = @("$base-setup.exe", "$base-portable.zip")
    $lines = foreach ($n in $names) { (Get-FileHash -LiteralPath "$stage\$n" -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + $n }
    [IO.File]::WriteAllText("$stage\SHA256SUMS.txt", (($lines -join "`n") + "`n"), $utf8)
    foreach ($n in ($names + 'SHA256SUMS.txt')) { Copy-Item -LiteralPath "$stage\$n" -Destination (Join-Path $out $n) -Force }
    Write-Host "Release assets saved to $out"
} finally { $env:Path = $oldPath; $env:CARGO_HOME = $oldCargo; Pop-Location }
