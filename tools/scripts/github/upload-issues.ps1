<#
.SYNOPSIS
  Bulk-creates the GitHub issues defined in docs/issues.md (JSON block) with the GitHub CLI.
.DESCRIPTION
  Creates labels and milestones if missing, then creates the issues in dependency order and replaces
  {{dep:ID}} placeholders with real issue numbers. Safe to re-run: issues whose "ID -" title prefix already
  exists are skipped. Writes tools/scripts/github/issue-map.json (ID -> number, url).
.EXAMPLE
  .\upload-issues.ps1 -DryRun
  .\upload-issues.ps1 -Repo owner/grain-to-pixel
#>
[CmdletBinding()]
param(
  [string]$Repo,
  [string]$IssuesFile,
  [string]$SetupFile,
  [string]$MapFile,
  [string[]]$Only,
  [int]$DelayMs = 1200,
  [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $IssuesFile) { $IssuesFile = Join-Path $here '../../../docs/issues.md' }
if (-not $SetupFile) { $SetupFile = Join-Path $here 'repo-setup.json' }
if (-not $MapFile) { $MapFile = Join-Path $here 'issue-map.json' }

function Read-Issues([string]$path) {
  $raw = [System.IO.File]::ReadAllText((Resolve-Path $path), [System.Text.Encoding]::UTF8)
  $m = [regex]::Match($raw, '(?s)<!-- issues-json:start -->\s*```json\s*(.*?)\s*```\s*<!-- issues-json:end -->')
  if (-not $m.Success) { throw "No JSON block found in $path" }
  $items = $m.Groups[1].Value | ConvertFrom-Json
  return @($items)
}
function Write-Utf8NoBom([string]$path, [string]$text) {
  [System.IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding($false)))
}

$issues = Read-Issues $IssuesFile
$setup = Get-Content -Raw -Encoding UTF8 $SetupFile | ConvertFrom-Json
if ($Only) { $issues = @($issues | Where-Object { $Only -contains $_.id }) }
Write-Host "Loaded $($issues.Count) issues, $($setup.labels.Count) labels, $($setup.milestones.Count) milestones."

# dependencies must come first
$seen = @{}
foreach ($i in $issues) {
  foreach ($d in $i.dependencies) {
    if (-not $Only -and -not $seen.ContainsKey($d)) { throw "Order error: $($i.id) depends on $d which is not earlier in the array." }
  }
  $seen[$i.id] = $true
}

if ($DryRun) {
  Write-Host '--- DRY RUN: no GitHub calls are made ---'
  foreach ($l in $setup.labels) { Write-Host "gh label create '$($l.name)' --color $($l.color) --force" }
  foreach ($m in $setup.milestones) { Write-Host "gh api repos/<repo>/milestones -f title='$($m.title)'" }
  $n = 0
  foreach ($i in $issues) {
    $n++
    $lab = ($i.labels | ForEach-Object { "--label '$_'" }) -join ' '
    Write-Host ("[{0}/{1}] gh issue create --title '{2}' --milestone '{3}' {4} --body-file <body {5} chars>" -f $n, $issues.Count, $i.title, $i.milestone, $lab, $i.body.Length)
  }
  return
}

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'GitHub CLI (gh) is not installed. Install it first (see docs/issues_upload_instructions.md).' }
gh auth status | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'gh is not authenticated. Run: gh auth login' }
if (-not $Repo) { $Repo = (gh repo view --json nameWithOwner -q .nameWithOwner) }
if (-not $Repo) { throw 'Pass -Repo owner/name (or run inside a clone with a GitHub remote).' }
gh repo view $Repo --json nameWithOwner | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Repository $Repo not found or not accessible." }
Write-Host "Target repository: $Repo"

# labels
foreach ($l in $setup.labels) {
  gh label create $l.name --color $l.color --description $l.description --repo $Repo --force | Out-Null
}
Write-Host 'Labels ready.'

# milestones
$existingMs = @(gh api "repos/$Repo/milestones?state=all&per_page=100" --jq '.[].title')
foreach ($m in $setup.milestones) {
  if ($existingMs -notcontains $m.title) {
    gh api "repos/$Repo/milestones" -f title="$($m.title)" -f description="$($m.description)" | Out-Null
    Write-Host "Created milestone $($m.title)"
  }
}

# existing issues (idempotency)
$map = @{}
$existing = gh issue list --repo $Repo --state all --limit 1000 --json number,title,url | ConvertFrom-Json
foreach ($e in $existing) {
  if ($e.title -match '^([A-Z]-\d{2}) ') { $map[$Matches[1]] = @{ number = $e.number; url = $e.url } }
}
Write-Host "$($map.Count) matching issues already exist and will be skipped."

$created = 0
foreach ($i in $issues) {
  if ($map.ContainsKey($i.id)) { Write-Host "skip   $($i.id) (#$($map[$i.id].number))"; continue }
  $body = $i.body
  foreach ($d in $i.dependencies) {
    $ref = if ($map.ContainsKey($d)) { "#$($map[$d].number)" } else { $d }
    $body = $body.Replace("{{dep:$d}}", $ref)
  }
  $fence = '```'
  $body = $body + "`n`n## Copy-paste prompt for the AI assistant`n<details><summary>Show prompt</summary>`n`n${fence}text`n$($i.prompt)`n${fence}`n`n</details>`n"
  $tmp = [System.IO.Path]::GetTempFileName()
  try {
    Write-Utf8NoBom $tmp $body
    $args = @('issue', 'create', '--repo', $Repo, '--title', $i.title, '--body-file', $tmp, '--milestone', $i.milestone)
    foreach ($l in $i.labels) { $args += @('--label', $l) }
    $url = (& gh @args | Select-Object -Last 1).Trim()
    if ($LASTEXITCODE -ne 0 -or $url -notmatch '/issues/(\d+)$') { throw "Failed to create $($i.id): $url" }
    $map[$i.id] = @{ number = [int]$Matches[1]; url = $url }
    $created++
    Write-Host "create $($i.id) -> $url"
  }
  finally { Remove-Item -Force $tmp -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds $DelayMs
}

Write-Utf8NoBom $MapFile ($map | ConvertTo-Json -Depth 4)
Write-Host "Done. Created $created issue(s); mapping written to $MapFile"
