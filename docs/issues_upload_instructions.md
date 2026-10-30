# Uploading the issues to GitHub with the GitHub CLI

Creates the 68 issues defined in [issues.md](issues.md) as real GitHub issues, with labels, milestones, dependency links and the copy-paste AI prompt inside each issue. Everything is done by one script: [`tools/scripts/github/upload-issues.ps1`](../tools/scripts/github/upload-issues.ps1) (PowerShell 5.1 on Windows, or `pwsh` 7 on macOS/Linux).

> **Status (2026-10-29):** uploaded to the private repository `sir-oscar-isaac-newton/Grain-to-Pixel` (68 issues, 24 labels, 8 milestones). The ID-to-number mapping is in `tools/scripts/github/issue-map.json`. A first run was cut short after 18 issues by an output pipe; re-running the script created the remaining 50 without duplicates.

## What gets created

| Item | Count | Source |
|---|---|---|
| Labels | 24 (`area:*`, `type:*`, `size:*`, `needs:*`, `adr`, `status:*`) | `tools/scripts/github/repo-setup.json` |
| Milestones | 8 (M0 to M7) | `tools/scripts/github/repo-setup.json` |
| Issues | 68, titled `T-01 – ...`, `P-03 – ...`, and so on | JSON block at the end of `docs/issues.md` |

Issues are created in dependency order, so each issue body lists its dependencies as real links (`#12`). The script is idempotent: an issue whose `ID –` title prefix already exists is skipped, so you can re-run it after a failure.

## 1. Prerequisites

1. **Install the GitHub CLI**
   - Windows: `winget install --id GitHub.cli -e`
   - macOS: `brew install gh` (and `brew install --cask powershell` if you want to run the script there)
2. **Log in** (choose GitHub.com, HTTPS, and a browser login; the `repo` scope is required):
   ```bash
   gh auth login
   gh auth status
   ```
3. **Have the target repository.** If it does not exist yet (private is recommended, because the prompts describe the project in detail):
   ```bash
   gh repo create <owner>/grain-to-pixel --private
   ```
   To connect your local clone later (after `git init`, task T-01): `git remote add origin https://github.com/<owner>/grain-to-pixel.git`.
4. **Node 20** is only needed if you regenerate `issues.md` (step 2).

## 2. (Optional) Regenerate the JSON from the source

The source of truth is `tools/scripts/issues/issues.src.txt`. After editing it:

```bash
node tools/scripts/issues/build-issues.mjs --check   # validates ids, dependencies, milestones, order
node tools/scripts/issues/build-issues.mjs           # rewrites docs/issues.md and repo-setup.json
```

## 3. Dry run (no GitHub calls)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/scripts/github/upload-issues.ps1 -DryRun
```

It prints the planned `gh label create`, milestone and `gh issue create` commands, in order, and fails if any dependency is not created earlier.

## 4. Upload

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/scripts/github/upload-issues.ps1 -Repo <owner>/grain-to-pixel
```

On macOS/Linux: `pwsh tools/scripts/github/upload-issues.ps1 -Repo <owner>/grain-to-pixel`.

What the script does: checks `gh` and authentication, verifies the repository, creates or updates labels (`gh label create --force`), creates missing milestones (`gh api repos/<repo>/milestones`), reads existing issues to skip duplicates, then for each issue writes the body to a temporary UTF-8 file and runs:

```text
gh issue create --repo <repo> --title "<ID – title>" --body-file <tmp> --milestone "<milestone>" --label <label> [--label ...]
```

It waits 1.2 seconds between issues (override with `-DelayMs`) to stay clear of GitHub's secondary rate limits; 68 issues take about two minutes. The mapping of issue ID to number and URL is written to `tools/scripts/github/issue-map.json` (commit it).

Options: `-Only T-01,T-02` creates a subset; `-IssuesFile`, `-SetupFile`, `-MapFile` override paths.

## 5. Verify

```bash
gh issue list --repo <owner>/grain-to-pixel --state open --limit 100 --json number,title | jq length   # expect 68
gh issue list --repo <owner>/grain-to-pixel --milestone "M0 Foundations"
gh issue view 1 --repo <owner>/grain-to-pixel
```

Check that an issue shows its dependency links, the checklist of acceptance criteria and the collapsible prompt.

## 6. Manual equivalents (for one issue)

```bash
gh label create "area:ui" --color 1d76db --description "User interface" --repo <owner>/grain-to-pixel --force
gh api repos/<owner>/grain-to-pixel/milestones -f title="M0 Foundations" -f description="..."
gh issue create --repo <owner>/grain-to-pixel --title "T-01 – Initialize Git repository and LFS" \
  --body-file body.md --milestone "M0 Foundations" --label "area:tooling" --label "type:chore" --label "size:S" --label "needs:human"
```

## 7. Working with the issues afterwards

- GitHub is the live tracker: use the `status:proposed`, `status:in-progress`, `status:in-review` and `status:blocked` labels and issue comments; close the issue when it is Done (see [workflow.md](workflow.md)).
- Each AI session starts by opening the issue, expanding "Show prompt" and pasting it into the assistant.
- Scope changes: edit `issues.src.txt`, regenerate, then edit the affected GitHub issue (`gh issue edit <n> --body-file ...`). Do not re-run the full upload to change existing issues; it skips them.

## 8. Undo (destructive; ask first)

Closing is safer than deleting:

```powershell
(Get-Content tools/scripts/github/issue-map.json -Raw | ConvertFrom-Json).PSObject.Properties | ForEach-Object { gh issue close $_.Value.number --repo <owner>/grain-to-pixel --comment "Closed: bulk import reverted" }
```

Deleting issues (`gh issue delete <n> --yes`) needs admin rights and cannot be undone.

## 9. Troubleshooting

| Symptom | Fix |
|---|---|
| `gh is not authenticated` | `gh auth login`; the token needs the `repo` scope |
| `Repository ... not found` | check `owner/name`, or create it (step 1.3) |
| `milestone ... not found` | rerun; milestones are created before issues. Titles must match exactly |
| `was submitted too quickly` / rate limit | rerun with `-DelayMs 3000`; already created issues are skipped |
| Garbled characters in titles | use PowerShell 7 (`pwsh`) or Windows Terminal; the body file is always UTF-8 without BOM |
| Script stops half-way | fix the cause and rerun; it resumes by skipping existing `ID –` titles |
| `Order error: X depends on Y` | the JSON array is not in dependency order; regenerate with the builder |
