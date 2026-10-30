<#
.SYNOPSIS
  Runs the WebDriver e2e test with a non-elevated (Basic User) token.
.DESCRIPTION
  WebView2 ignores the environment overrides that msedgedriver relies on when its host process is
  elevated, which is how GitHub-hosted Windows runners run jobs (runneradmin, High integrity).
  `runas /trustlevel:0x20000` starts a process with the same user but a limited token. The child
  starts from the user's default environment, so PATH and the test variables are passed explicitly.
  Exit code: the exit code of the test run.
#>
[CmdletBinding()]
param(
  [int]$TimeoutSeconds = 300
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = (Resolve-Path (Join-Path $here '../../..')).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) ('gtp-e2e-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force $work | Out-Null
$log = Join-Path $work 'e2e.log'
$exitFile = Join-Path $work 'e2e.exit'
$cmd = Join-Path $work 'run.cmd'

$edgeDriver = if ($env:GTP_E2E_EDGEDRIVER) { $env:GTP_E2E_EDGEDRIVER } else { Join-Path $root '.cache/edgedriver/msedgedriver.exe' }
$node = (Get-Command node).Source
@(
  '@echo off',
  "set `"PATH=$env:Path`"",
  "set `"GTP_E2E_EDGEDRIVER=$edgeDriver`"",
  "cd /d `"$root`"",
  "`"$node`" --test apps/desktop/tests/e2e/window.e2e.mjs > `"$log`" 2>&1",
  "echo %ERRORLEVEL% > `"$exitFile`""
) | Set-Content -Path $cmd -Encoding ascii

Write-Host 'Starting the e2e run with a non-elevated token...'
& runas /trustlevel:0x20000 "cmd /c `"$cmd`""
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
while (-not (Test-Path $exitFile) -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 2 }

if (Test-Path $log) { Get-Content $log }
if (-not (Test-Path $exitFile)) {
  Write-Host "e2e run did not finish within $TimeoutSeconds s"
  exit 1
}
$code = [int]((Get-Content $exitFile) -join '').Trim()
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
exit $code
