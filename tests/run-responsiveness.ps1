$ErrorActionPreference = 'Stop'
$projectPath = Join-Path $PSScriptRoot '..\src-tauri'
Push-Location $projectPath
try {
    $buildMessages = & cargo test --lib --no-run --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Native test build failed.' }
    $testArtifact = $buildMessages | ForEach-Object {
        $message = $_ | ConvertFrom-Json
        if ($message.reason -eq 'compiler-artifact' -and $message.profile.test -and $message.executable) {
            $message.executable
        }
    } | Select-Object -Last 1
    if (-not $testArtifact) { throw 'Native test executable was not produced.' }

    # Tauri's app manifest is not embedded in the Rust library test harness.
    # Common Controls v6 is required by its TaskDialogIndirect import on Windows.
    $sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $manifestTool = Get-ChildItem "$sdkRoot\*\x64\mt.exe" |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $manifestTool) { throw 'Windows SDK manifest tool (mt.exe) was not found.' }
    $manifestPath = Join-Path (Split-Path $testArtifact) 'responsiveness-test.manifest'
    @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency></assembly>
'@ | Set-Content -LiteralPath $manifestPath
    & $manifestTool.FullName -nologo -manifest $manifestPath "-outputresource:$testArtifact;#1"
    if ($LASTEXITCODE -ne 0) { throw 'Could not embed the test manifest.' }
    & $testArtifact responsiveness_tests --nocapture
    if ($LASTEXITCODE -ne 0) { throw 'Responsiveness regression test failed.' }
} finally {
    Pop-Location
}
