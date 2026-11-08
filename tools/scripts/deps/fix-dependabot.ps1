# Brings a Dependabot pull request to a state CI accepts (T-08): checks out its branch, merges
# develop, installs, regenerates docs/third-party-licenses.txt and pushes the result to the PR's
# branch when the notices changed. Dependabot cannot run `pnpm gen:notices` itself.
#
#   tools/scripts/deps/fix-dependabot.ps1 218
#
# Refuses a PR that is not Dependabot's or a working tree with uncommitted changes to tracked
# files. Returns to the branch it started on. Needs git, gh (signed in) and pnpm.
param(
  [Parameter(Mandatory = $true)][int]$Pr,
  [string]$Base = 'develop'
)
$ErrorActionPreference = 'Stop'

function Invoke-Native([string]$exe, [string[]]$arguments) {
  & $exe @arguments
  if ($LASTEXITCODE -ne 0) { throw "$exe $($arguments -join ' ') failed with exit code $LASTEXITCODE." }
}

$root = (git rev-parse --show-toplevel).Trim()
Set-Location $root

$info = gh pr view $Pr --json author,headRefName,state | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw "PR $Pr was not found." }
if ($info.author.login -notin @('app/dependabot', 'dependabot[bot]', 'dependabot')) {
  throw "PR $Pr is by $($info.author.login), not Dependabot."
}
if ($info.state -ne 'OPEN') { throw "PR $Pr is $($info.state.ToLower()), not open." }
if (git status --porcelain --untracked-files=no) {
  throw 'The working tree has uncommitted changes; commit or stash them first.'
}

$branch = $info.headRefName
$start = (git rev-parse --abbrev-ref HEAD).Trim()
Write-Host "PR $Pr ($branch)"
try {
  Invoke-Native git @('fetch', '-q', 'origin', $branch, $Base)
  Invoke-Native git @('checkout', '-q', '-B', "fix-dependabot-$Pr", "origin/$branch")
  Invoke-Native git @('merge', '-q', '--no-edit', "origin/$Base")
  Invoke-Native pnpm @('install', '--frozen-lockfile')
  Invoke-Native pnpm @('gen:notices')
  if (git status --porcelain -- docs/third-party-licenses.txt) {
    Invoke-Native git @('add', 'docs/third-party-licenses.txt')
    Invoke-Native git @('commit', '-q', '-m', 'chore(deps): regenerate the third-party notices')
    Write-Host 'Regenerated the notices.'
  } else {
    Write-Host 'The notices were already current.'
  }
  $ahead = (git rev-list --count "origin/$branch..HEAD").Trim()
  if ($ahead -ne '0') {
    Invoke-Native git @('push', '-q', 'origin', "HEAD:$branch")
    Write-Host "Pushed $ahead commit(s) to $branch; CI runs again on the PR."
  } else {
    Write-Host 'Nothing to push: the branch was up to date.'
  }
} finally {
  git checkout -q $start
  git branch -q -D "fix-dependabot-$Pr" 2>$null
  # The checkout of the PR may have installed other package versions.
  pnpm install --frozen-lockfile | Out-Null
}
