<#
.SYNOPSIS
  Reproduces what msedgedriver does to a WebView2 app, without the driver, and reports what happens.
.DESCRIPTION
  msedgedriver starts the app with WEBVIEW2_USER_DATA_FOLDER and WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
  (including --remote-debugging-port=0) and then waits for DevToolsActivePort in the user data folder.
  This script does the same for a few argument sets and prints, for each: whether the app stayed alive,
  whether DevToolsActivePort appeared, the files in the data folder and the tail of any browser log.
  Used to investigate "session not created: DevToolsActivePort file doesn't exist" on the CI runner.
#>
[CmdletBinding()]
param(
  [string]$App,
  [int]$WaitSeconds = 20
)
$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $App) { $App = Join-Path $here '../../../target/release/grain-to-pixel-desktop.exe' }
$App = (Resolve-Path $App).Path
$base = Join-Path ([System.IO.Path]::GetTempPath()) ('wv2-diag-' + [guid]::NewGuid().ToString('N').Substring(0, 8))

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$elevated = (New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
Write-Host "user: $($identity.Name); elevated (administrator): $elevated"

# Edge / WebView2 policies can disable remote debugging or ignore command-line flags.
foreach ($k in 'HKLM:\SOFTWARE\Policies\Microsoft\Edge', 'HKCU:\SOFTWARE\Policies\Microsoft\Edge', 'HKLM:\SOFTWARE\Policies\Microsoft\Edge\WebView2', 'HKLM:\SOFTWARE\Policies\Microsoft\EdgeWebView', 'HKLM:\SOFTWARE\Policies\Microsoft\Edge\Recommended') {
  if (Test-Path $k) {
    $props = (Get-ItemProperty $k).PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object { "$($_.Name)=$($_.Value)" }
    Write-Host "policy key ${k}: $($props -join '; ')"
  } else { Write-Host "policy key ${k}: (absent)" }
}

# Shows whether the flags from WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS reached the browser process.
function Show-BrowserProcess {
  $procs = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction SilentlyContinue)
  $browser = $procs | Where-Object { $_.CommandLine -and $_.CommandLine -notmatch '--type=' } | Select-Object -First 1
  if ($browser) {
    $cl = $browser.CommandLine
    Write-Host ('browser process: ' + $browser.ExecutablePath)
    Write-Host ('  has --remote-debugging-port: ' + ($cl -match 'remote-debugging-port') + '; --enable-logging: ' + ($cl -match 'enable-logging') + '; --user-data-dir: ' + ($cl -match 'user-data-dir'))
    Write-Host ('  command line (first 600 chars): ' + $cl.Substring(0, [Math]::Min(600, $cl.Length)))
  } else {
    Write-Host ("no WebView2 browser process found (msedgewebview2 processes: $($procs.Count))")
  }
}

$driverArgs = '--allow-pre-commit-input --disable-background-networking --disable-client-side-phishing-detection --disable-default-apps --disable-hang-monitor --disable-popup-blocking --disable-sync --enable-automation --enable-logging --log-level=0 --no-first-run --password-store=basic --remote-debugging-port=0 --test-type=webdriver --use-mock-keychain'

function Invoke-Pass([string]$label, [string]$browserArgs) {
  Write-Host "`n===== PASS: $label"
  $udf = Join-Path $base ($label -replace '[^\w]', '_')
  New-Item -ItemType Directory -Force $udf | Out-Null
  $env:WEBVIEW2_USER_DATA_FOLDER = $udf
  if ($browserArgs) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $browserArgs } else { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
  $p = Start-Process -FilePath $App -PassThru
  $found = $false
  for ($i = 0; $i -lt $WaitSeconds; $i++) {
    Start-Sleep -Seconds 1
    if ((Test-Path (Join-Path $udf 'EBWebView/DevToolsActivePort')) -or (Test-Path (Join-Path $udf 'DevToolsActivePort'))) { $found = $true; break }
    if ($p.HasExited) { break }
  }
  $p.Refresh()
  Write-Host "app alive: $(-not $p.HasExited); DevToolsActivePort present: $found"
  Show-BrowserProcess
  $portFile = @('EBWebView/DevToolsActivePort', 'DevToolsActivePort') | ForEach-Object { Join-Path $udf $_ } | Where-Object { Test-Path $_ } | Select-Object -First 1
  if ($portFile) { Write-Host ('port file ' + $portFile.Substring($udf.Length) + ': ' + ((Get-Content $portFile) -join ' / ')) }
  Get-ChildItem $udf -Filter 'chrome_debug.log' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1 | ForEach-Object {
    Write-Host '--- chrome_debug.log (tail)'
    Get-Content $_.FullName -Tail 30 -ErrorAction SilentlyContinue
  }
  Write-Host ("webview2 processes: " + ((Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Measure-Object).Count))
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
  Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 2
}

# Same launch, but from a non-elevated (Basic User) token via runas /trustlevel.
function Invoke-LimitedPass([string]$browserArgs) {
  $label = 'limited-token-driver-flags'
  Write-Host "`n===== PASS: $label (runas /trustlevel:0x20000)"
  $udf = Join-Path $base $label
  New-Item -ItemType Directory -Force $udf | Out-Null
  $cmd = Join-Path $base 'limited.cmd'
  $integrity = Join-Path $base 'integrity.txt'
  @(
    '@echo off',
    "whoami /groups | findstr /c:`"Mandatory Label`" > `"$integrity`"",
    "set `"WEBVIEW2_USER_DATA_FOLDER=$udf`"",
    "set `"WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=$browserArgs`"",
    "start `"`" `"$App`""
  ) | Set-Content -Path $cmd -Encoding ascii
  & runas /trustlevel:0x20000 "cmd /c `"$cmd`"" 2>&1 | ForEach-Object { Write-Host "runas: $_" }
  $found = $false
  for ($i = 0; $i -lt $WaitSeconds; $i++) {
    Start-Sleep -Seconds 1
    if (Test-Path (Join-Path $udf 'EBWebView/DevToolsActivePort')) { $found = $true; break }
  }
  $alive = [bool](Get-Process grain-to-pixel-desktop -ErrorAction SilentlyContinue)
  Write-Host "app alive: $alive; DevToolsActivePort present: $found"
  Show-BrowserProcess
  if (Test-Path $integrity) { Write-Host ('integrity level inside the launched process: ' + ((Get-Content $integrity) -join ' | ').Trim()) } else { Write-Host 'integrity file not written (the launch script did not run)' }
  if (-not $found) { Write-Host ('data folder entries: ' + ((Get-ChildItem $udf -Recurse -Depth 2 -ErrorAction SilentlyContinue | Select-Object -First 15 | ForEach-Object { $_.FullName.Substring($udf.Length) }) -join ', ')) }
  Get-Process grain-to-pixel-desktop -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 2
}

Invoke-Pass 'plain-no-flags' ''
Invoke-Pass 'driver-flags' $driverArgs
Invoke-Pass 'driver-flags-plus-no-sandbox' ($driverArgs + ' --no-sandbox')
Invoke-Pass 'driver-flags-plus-disable-gpu' ($driverArgs + ' --disable-gpu')
Invoke-LimitedPass $driverArgs

Remove-Item -Recurse -Force $base -ErrorAction SilentlyContinue
Remove-Item Env:WEBVIEW2_USER_DATA_FOLDER, Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
