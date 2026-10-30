<#
.SYNOPSIS
  Downloads the msedgedriver that matches the installed WebView2 runtime (needed by tauri-driver on Windows).
.DESCRIPTION
  tauri-driver drives the app through the Microsoft Edge WebDriver, whose version must match the WebView2
  runtime. This script reads the runtime version from the registry and downloads the matching driver from
  Microsoft's download server into .cache/edgedriver (git-ignored). Falls back to the latest driver of the
  same major version if the exact build is not published. Prints the path of msedgedriver.exe.
#>
[CmdletBinding()]
param(
  [string]$OutDir
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $OutDir) { $OutDir = Join-Path $here '../../../.cache/edgedriver' }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$exe = Join-Path $OutDir 'msedgedriver.exe'

$keys = @(
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
  'HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
)
$version = $null
foreach ($k in $keys) {
  $p = Get-ItemProperty $k -ErrorAction SilentlyContinue
  if ($p -and $p.pv) { $version = $p.pv; break }
}
if (-not $version) { throw 'WebView2 runtime not found; install it first.' }
Write-Host "WebView2 runtime: $version"

$base = 'https://msedgedriver.microsoft.com'
function Get-Zip([string]$ver, [string]$dest) {
  Invoke-WebRequest -UseBasicParsing -Uri "$base/$ver/edgedriver_win64.zip" -OutFile $dest
}

$zip = Join-Path $OutDir 'edgedriver.zip'
try {
  Get-Zip $version $zip
} catch {
  $major = $version.Split('.')[0]
  Write-Host "No exact driver for $version; looking up the latest release for major $major"
  $latest = (Invoke-WebRequest -UseBasicParsing -Uri "$base/LATEST_RELEASE_${major}_WINDOWS").Content
  $latest = ($latest -replace '[^\d\.]', '').Trim()
  Write-Host "Using msedgedriver $latest"
  Get-Zip $latest $zip
}
Expand-Archive -Path $zip -DestinationPath $OutDir -Force
Remove-Item $zip -Force
if (-not (Test-Path $exe)) { throw "msedgedriver.exe was not found after extraction in $OutDir" }
& $exe --version
Write-Host "EDGEDRIVER=$exe"
