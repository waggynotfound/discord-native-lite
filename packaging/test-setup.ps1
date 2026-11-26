param([Parameter(Mandatory=$true)][string]$Setup)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Setup).Path
if (![Environment]::Is64BitProcess) { throw 'Run the installer smoke test from 64-bit PowerShell.' }
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{7128B448-EEE8-49F5-B7A1-336AC8D4FC8A}_is1'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Relay.lnk'
$desktopLink = Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'Relay.lnk'
if ((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath $shortcut) -or (Test-Path -LiteralPath $desktopLink)) {
    throw 'Existing Relay installation or shortcut detected. Run the smoke test on a clean Windows account.'
}
function Run-Setup([string]$Binary, [string]$Arguments) {
    $process = Start-Process -FilePath $Binary -ArgumentList $Arguments -PassThru -Wait
    try { if ($process.ExitCode -ne 0) { throw "Setup returned exit code $($process.ExitCode)." } }
    finally { $process.Dispose() }
}
$testDir = Join-Path ([IO.Path]::GetTempPath()) ('Relay setup check ' + [guid]::NewGuid().ToString('N'))
$uninstall = Join-Path $testDir 'unins000.exe'
$quiet = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-'
$app = $null
try {
    Run-Setup $exe ($quiet + ' /DIR="' + $testDir + '"')
    foreach ($file in 'Relay.exe','LICENSE','README.md','licenses\CARGO-NOTICES.txt','licenses\RUST-LIBRARY.html','unins000.exe') {
        if (!(Test-Path -LiteralPath (Join-Path $testDir $file) -PathType Leaf)) { throw "Missing installed file: $file" }
    }
    $registration = Get-ItemProperty -LiteralPath $key
    if ($registration.DisplayName -ne 'Relay' -or $registration.InstallLocation.TrimEnd('\') -ne $testDir) { throw 'Wrong uninstall registration.' }
    if (!(Test-Path -LiteralPath $shortcut)) { throw 'No Start menu shortcut.' }
    $shell = New-Object -ComObject WScript.Shell
    if ($shell.CreateShortcut($shortcut).TargetPath -ne (Join-Path $testDir 'Relay.exe')) { throw 'Start menu shortcut has wrong target.' }
    if (Test-Path -LiteralPath $desktopLink) { throw 'Setup created the optional desktop shortcut by default.' }
    $app = Start-Process -FilePath (Join-Path $testDir 'Relay.exe') -PassThru
    if (!$app.WaitForInputIdle(10000)) { throw 'Installed GUI did not initialize.' }
    $limit = [DateTime]::UtcNow.AddSeconds(6)
    do { $app.Refresh(); if ($app.MainWindowHandle -ne [IntPtr]::Zero -and $app.MainWindowTitle -eq 'Relay | Discord launcher') { break }; Start-Sleep -Milliseconds 100 } while ([DateTime]::UtcNow -lt $limit)
    if ($app.MainWindowTitle -ne 'Relay | Discord launcher') { throw 'Installed GUI did not open.' }
    Write-Host "Installed Relay $($registration.DisplayVersion); shortcut, notices and GUI verified."
} finally {
    if ($app) {
        if (!$app.HasExited) { [void]$app.CloseMainWindow(); if (!$app.WaitForExit(5000)) { $app.Kill(); $app.WaitForExit() } }
        $app.Dispose()
    }
    if (Test-Path -LiteralPath $uninstall) { Run-Setup $uninstall $quiet }
}
$deadline = [DateTime]::UtcNow.AddSeconds(15)
while ((Test-Path -LiteralPath $testDir) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 150 }
if ((Test-Path -LiteralPath $testDir) -or (Test-Path -LiteralPath $shortcut) -or (Test-Path -LiteralPath $key)) { throw 'Uninstall left setup files, a shortcut, or the registration behind.' }
Write-Host 'Uninstall verified. No real Discord client was launched or stopped.'
