# TEMPORARY (T-07 spike): finds out what a GitHub-hosted Windows runner lets the app do with a
# debugging port. Prints facts; never fails the job.
$ErrorActionPreference = 'Continue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$exe = Join-Path $root 'target/release/grain-to-pixel-desktop.exe'
$data = Join-Path $env:LOCALAPPDATA 'io.github.sir-oscar-isaac-newton.graintopixel'

function Section($t) { Write-Host "`n=== $t" }

function Webview-Processes {
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
    Select-Object -First 3 ProcessId, @{n = 'Cmd'; e = { $_.CommandLine.Substring(0, [Math]::Min(700, $_.CommandLine.Length)) } } |
    Format-List | Out-String | Write-Host
}

function Probe([int]$port, [int]$seconds = 30) {
  $deadline = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $deadline) {
    try {
      $r = Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$port/json/version" -TimeoutSec 2
      Write-Host "PORT $port ANSWERS: $($r.Content -replace '\s+', ' ')"
      return $true
    } catch { Start-Sleep -Milliseconds 700 }
  }
  Write-Host "PORT $port DID NOT ANSWER"
  Get-ChildItem $data -Recurse -Filter DevToolsActivePort -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "found $($_.FullName)" }
  return $false
}

function Stop-All {
  Get-Process grain-to-pixel-desktop, msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 2
}

Section 'identity of this session'
whoami
whoami /groups | Select-String 'Mandatory Label|BUILTIN\\Administrators'
Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "WebView2 runtime $($_.pv)" }

Section 'A1: flag baked into the app (additionalBrowserArgs)'
$p = Start-Process $exe -PassThru
$okA = Probe 9222
Webview-Processes
if ($okA -and (Test-Path (Join-Path $PSScriptRoot 'spike-attach.mjs'))) {
  Section 'A1: attach msedgedriver to the open port'
  & (Join-Path $PSScriptRoot 'install-edgedriver.ps1') | Out-Host
  node (Join-Path $PSScriptRoot 'spike-attach.mjs') $root 2>&1 | Out-Host
}
Stop-All

Section 'A2: the environment variable (the known failing way)'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9223'
$p = Start-Process $exe -PassThru
$null = Probe 9223 20
Webview-Processes
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
Stop-All

Section 'B1: a throwaway standard user (Start-Process -Credential)'
try {
  $secret = 'Aa1!' + ([guid]::NewGuid().ToString('N'))
  $pw = ConvertTo-SecureString $secret -AsPlainText -Force
  New-LocalUser -Name gtpe2e -Password $pw -ErrorAction Stop | Out-Null
  Add-LocalGroupMember -Group Users -Member gtpe2e -ErrorAction SilentlyContinue
  $cred = New-Object System.Management.Automation.PSCredential('.\gtpe2e', $pw)
  $p = Start-Process $exe -Credential $cred -LoadUserProfile -WorkingDirectory (Split-Path $exe) -PassThru -ErrorAction Stop
  $null = Probe 9222 30
  Webview-Processes
} catch { Write-Host "B1 failed: $($_.Exception.Message)" }
Stop-All

Section 'B2: de-elevated through a scheduled task (RunLevel Limited)'
try {
  $action = New-ScheduledTaskAction -Execute $exe
  $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
  Register-ScheduledTask -TaskName gtp-e2e -Action $action -Principal $principal -Force -ErrorAction Stop | Out-Null
  Start-ScheduledTask -TaskName gtp-e2e -ErrorAction Stop
  $null = Probe 9222 30
  Webview-Processes
} catch { Write-Host "B2 failed: $($_.Exception.Message)" }
Stop-All
Unregister-ScheduledTask -TaskName gtp-e2e -Confirm:$false -ErrorAction SilentlyContinue
exit 0
