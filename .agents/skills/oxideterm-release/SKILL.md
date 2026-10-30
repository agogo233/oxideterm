---
name: oxideterm-release
description: Prepare, publish, or recover canceled OxideTerm stable, beta, or GPUI preview releases by deriving changelog content from the previous tag, selecting the requested release-note detail level, running the repository version-bump script, validating channel-specific notes and fork ownership, committing, pushing, creating the correct annotated tag, building the Windows portable package, and publishing the release assets. Use when the user asks to upgrade the OxideTerm version, prepare a release, write a release changelog, commit and push a release, create a release tag, republish the same version after its release workflow was canceled, or repair release assets.
---

# OxideTerm Release

Publish an OxideTerm release through the repository-owned automation. Treat the user's completed GUI run as the release gate and do not run Cargo tests by default.

## Required release input

Resolve these values before making release changes:

- Target version.
- Target channel: `stable`, `beta`, or `gpui-preview`.
- Explicit confirmation that the user ran the GUI application and approved publishing.
- Release-note detail level when the user explicitly requests `minimal`, `medium`, or `detailed`; otherwise use `detailed`.

Infer the channel only when the version is unambiguous. Ask before publishing if GUI approval is absent. Preparing a changelog or dry run does not require approval.

## Channel contract

| Channel | Version | Tag | Changelog | Base notes |
|---|---|---|---|---|
| Stable | `X.Y.Z` | `vX.Y.Z` | `.github/release-notes/stable-changelog.md` | `.github/release-notes/stable.md` |
| Beta | `X.Y.Z-beta.N` | `vX.Y.Z-beta.N` | `.github/release-notes/beta-changelog.md` | `.github/release-notes/beta.md` |
| GPUI preview | `X.Y.Z-gpui-preview.N` | `gpui-vX.Y.Z-gpui-preview.N` | `.github/release-notes/gpui-preview-changelog.md` | `.github/release-notes/gpui-preview.md` |

Do not invent another tag prefix. Release notes are selected from the existing tag, and the packaging workflow builds the commit that the tag points to.

## Workflow

### 1. Inspect repository state

Work from the OxideTerm repository root. Read `AGENTS.md`, then inspect:

```bash
git status --short --branch
git remote -v
git log -10 --oneline --decorate
```

Fetch the publishing remote and tags before deciding that a tag is available:

```bash
git fetch origin --tags
```

Confirm the branch is not behind or diverged from its upstream, and inspect the target tag locally and remotely:

```bash
git rev-list --left-right --count HEAD...@{upstream}
git tag --list <tag>
git ls-remote --tags origin refs/tags/<tag> refs/tags/<tag>^{}
```

Preserve unrelated user changes. Stop if the intended release scope cannot be separated safely or the branch has diverged. For an ordinary release, stop when the target tag exists. Only move an existing tag through the explicit canceled-release recovery procedure below.

### 2. Establish the changelog range

Run the bundled helper before writing the changelog:

```bash
python3 .agents/skills/oxideterm-release/scripts/release_context.py \
  --repo . --channel <channel> --version <version>
```

Prefer the previous reachable tag from the same channel as the baseline:

- Stable compares with the previous stable tag.
- Beta compares with the previous beta tag.
- GPUI preview compares with the previous GPUI preview tag.

For the first release in a channel, use the newest reachable release tag from any of the three channels as the bootstrap baseline. Use the root commit only when the repository has no earlier reachable release tag. This keeps a first beta or preview changelog scoped to work since the actual preceding release instead of summarizing the entire repository.

The helper prints the exact range, commits, and diff summary. Also inspect actual changes, including intended uncommitted changes:

```bash
git diff <previous-tag>..HEAD
git diff
git diff --cached
```

Do not derive release notes from commit subjects alone. Read the meaningful implementation and user-facing differences. Exclude mechanical version bumps, changelog edits, formatting-only churn, and internal details that have no release impact.

#### Resolve pull requests and contributors

For every release channel, map the changes in the exact release range to their originating pull requests before drafting the notes. The helper's commit list excludes merge commits; inspect merge history and GitHub's commit-to-PR associations as well so merge, squash, and rebase merges receive attribution:

```bash
git log --format='%H %s' <previous-tag>..<target-ref>
gh api --paginate repos/<owner>/<repo>/commits/<commit-sha>/pulls \
  --jq '.[] | {number, title, url: .html_url, author: .user.login, merged_at, merge_commit_sha}'
gh pr view <number> --repo <owner>/<repo> \
  --json number,title,url,author,mergedAt,mergeCommit,commits
```

Keep a working mapping of each included PR's number, URL, author login, and actual contribution. Deduplicate PRs returned for multiple commits. Confirm inclusion against the target history and diff; merge dates alone do not establish release scope. Verify original PR provenance for cherry-picked changes. Fully reverted changes must not be presented as shipped features.

Use the PR author, not the person or bot that merged it. For contributor thanks, exclude confirmed project maintainers and bot accounts; do not assume a human contributor is a maintainer merely because GitHub labels them `COLLABORATOR` or `MEMBER`. Keep credit for external PR work when the maintainer made follow-up fixes. A referenced issue number is not evidence of a PR or its author. If GitHub lookup fails, keep the unresolved attribution in working notes and resolve it before finalizing release notes; do not interpret lookup failure as no contributions.

### 3. Write the channel changelog

Insert `## <version>` as the newest entry in the selected changelog. The heading must exactly match the version because `.github/scripts/compose_release_notes.py` uses it to locate the entry. The heading is an extraction boundary, not necessarily part of the published release body.

Write every version entry as two complete language blocks in this order (Chinese first, English second). The composer adds language jump links at the very top of bilingual release notes for every channel:

```markdown
## <version>

### 中文

<Chinese summary and sections>

### English

<English summary and sections>
```

Keep both blocks structurally aligned: they must describe the same outcomes, limitations, validation status, and upgrade requirements in the same order. Translate for natural release-note language rather than word for word, and keep product names, commands, file names, and protocol names unchanged when translation would reduce precision. Do not merge English and Chinese into the same bullet. Use concise user-facing past tense in English and concise completed-action wording in Chinese. Keep each language's opening summary paragraph on one physical line so the GitHub Release editor does not show an artificial break. Combine related commits into one outcome and avoid raw commit-title dumps, implementation trivia, unsupported performance claims, and claims that were not verified. Use one restrained, semantically relevant emoji on each main stable-release section heading in both blocks; do not decorate every bullet or mix multiple emoji styles within one section.

For a fork release, each language block must clearly separate changes inherited from official OxideTerm upstream from changes implemented by the fork. Use explicit localized headings equivalent to `Upstream changes` / `上游更新` and `Fork-specific changes` / `Fork 自有更新`; subsystem headings may be nested below them when the selected detail level warrants it. Classify merged or cherry-picked official work as upstream regardless of commit author, and classify behavior unique to the publishing fork as fork-specific. When a fork modifies an upstream feature, describe the inherited capability under upstream and the fork's material delta under fork-specific changes. If either category has no changes in the release range, state that explicitly instead of omitting the category. Never let a combined summary or bullet imply that the fork authored upstream work.

#### PR references and contributor thanks

Follow the attribution pattern in [NyaTerm v1.2.12](https://github.com/nyakang/nyaterm/releases/tag/v1.2.12): PR references beside the changes they introduced, plus a dedicated contributor section. Derive the actual entries from OxideTerm's release range.

- Append linked PR numbers to every release-note item introduced through a PR, including maintainer-authored PRs. Use `（PR [#<number>](<pr-url>)）` in Chinese and `(PR [#<number>](<pr-url>))` in English. Put the reference at the end of the relevant bullet or feature paragraph, rather than only in the contributor list or full-changelog link. Keep issue links distinct from PR links. Direct commits have no invented PR number.
- When several PRs contributed to a grouped item, include all relevant PR numbers once in that item. Attribute only the outcome supported by each PR; separate a maintainer's independent follow-up change when grouping would credit the wrong author. For PRs from another repository, qualify the link label with `owner/repo#number`.
- Whenever the release includes external PR contributions, add `#### 👥 贡献者` inside the Chinese block and `#### 👥 Contributors` inside the English block, after the change sections and before upgrade notes when present. Thank every external PR author whose contribution is included in this release, including returning contributors and documentation, translation, build, or packaging contributors whose work may not warrant a standalone feature bullet.
- List each author once per language block using their verified GitHub `@login`, a brief description of their contribution, and linked PR numbers. Match authors and PR references across both languages. A generic thank-you or GitHub's automatic avatar strip does not replace these entries. If identifying first-time contributors, verify prior repository contributions; a `New Contributors` list supplements the full thanks section.
- Apply attribution at every detail level and in every channel. If the verified range contains no external PR contribution, omit the contributor section rather than adding an empty heading. A PR outside the release range is not credited as a new contribution to this version.

Example contributor entries (replace the placeholders with verified release data):

```markdown
#### 👥 贡献者

感谢以下外部贡献者参与本次更新：

- @<login>：<具体贡献>（PR [#<number>](<pr-url>)）。

#### 👥 Contributors

Thank you to the external contributors to this release:

- @<login>: <specific contribution> (PR [#<number>](<pr-url>)).
```

Place the two example sections in their respective language blocks, not together as a third bilingual section.

#### Release-note detail level

Use the same level for the English and Chinese blocks. The levels control coverage and grouping, not a mandatory word count. Never omit a breaking change, security boundary, upgrade requirement, known limitation, or material compatibility issue just to satisfy a shorter level.

| Level | Required density |
|---|---|
| `minimal` | Use only when the user explicitly requests it. Write one short summary followed by roughly 2–5 highly consolidated outcome bullets, with at most one or two headings when they materially improve scanning. Cover only the release's defining user-visible changes and required warnings. |
| `medium` | Use only when the user explicitly requests it. Write one summary, a small number of useful sections, and grouped bullets that cover the major features, fixes, and security or compatibility outcomes. Consolidate related supporting workflows instead of enumerating each one separately. |
| `detailed` | Default unless the user explicitly chooses another level. Organize the notes by meaningful subsystem or workflow and give each distinct user-visible capability, behavior change, compatibility fix, supported limitation, and verified performance or validation result enough space to be understood without reading the diff. Include supporting workflows when they materially affect how users understand or use the change, but do not add filler when a release has fewer real changes. |

Treat the levels as coverage and grouping guidance, not fixed templates. Determine the complete factual delta first, then consolidate it to the selected level. If the user says only “write the release notes” or “prepare the release,” use `detailed`.

Apply channel-specific emphasis:

- **Stable:** Summarize the complete delta since the previous stable tag. In each language block, start with one short release summary, then use only useful sections such as `#### ✨ Highlights` / `#### ✨ 重点更新`, `#### 🛠️ Fixes` / `#### 🛠️ 修复`, `#### 🔒 Security` / `#### 🔒 安全`, or `#### 🧰 Release Maintenance` / `#### 🧰 发布维护`. Emphasize user-visible behavior and compatibility. The composed GitHub Release body must begin with `[中文](#中文) | [English](#english)` navigation, followed by the complete `### 中文` block and then the matching `### English` block, omit both a product-major heading such as `# OxideTerm 2.0` and a repeated version heading such as `## 2.0.7`, then place `## 📥 Download for your system`, installation tips, and links after both language blocks.
- **Beta:** Summarize the delta since the previous beta tag in complete English and Chinese blocks. State what is approaching stable, what changed, and which workflows need validation. Mention known limitations only when supported by the diff or issue context, and include them in both languages.
- **GPUI preview:** Summarize the delta since the previous GPUI preview tag in complete English and Chinese blocks. Focus on newly testable native UI/runtime work, parity, rough edges, and concrete testing targets. Apply the selected detail level while keeping the writing test-oriented rather than turning preview notes into stable-release marketing.

If there is no earlier tag for that channel, state which preceding release tag was used as the bootstrap baseline.

### 4. Run the repository version script

Always use the repository script; never hand-edit the workspace version or lockfile:

```bash
python3 scripts/release/bump_version.py <version>
```

This validates SemVer, updates `[workspace.package]`, and refreshes `Cargo.lock` offline. README badges display the latest GitHub release automatically and need no version edits.

### 5. Perform lightweight release validation

Do not run `cargo test`, `cargo check`, or launch the GUI by default. The user owns GUI validation before publishing. Run broader checks only when explicitly requested or when a concrete release blocker requires them.

Validate only the release mechanics:

```bash
python3 scripts/release/bump_version.py <version> --dry-run
git diff --check
```

Compose the exact release notes into a temporary file outside the repository:

```bash
python3 .github/scripts/compose_release_notes.py \
  --version <version> \
  --tag <tag> \
  --base <base-notes> \
  --changelog <channel-changelog> \
  --output /tmp/oxideterm-release-notes-<version>.md
```

Read the generated file and verify that the intended section appears once, the channel is correct, both `### 中文` and `### English` appear once in that order, their claims and bullet coverage match, and stable download URLs use the target tag. For stable notes, also verify that `[中文](#中文) | [English](#english)` is the first visible content, both navigation links target the language headings, and both language blocks appear before downloads, the GitHub Release title is not repeated in the body, and the order is bilingual changelog, downloads, installation tips, then links.

Compare the composed notes with the PR mapping: each PR-derived change must retain its linked PR number beside the item in both languages, every included external PR author must appear in both contributor sections with the correct contribution and PR links, and the references must resolve to pull requests in the correct repository. Perform this check on the composed output, not just the changelog source.

### Fork release attribution check

This release skill ships inside the repository, so forks that reuse it inherit this rule. Determine the publishing repository from the remote that will receive the branch and tag. Treat it as a fork whenever that repository is not `AnalyseDeCircuit/oxideterm`, even when a separate `upstream` remote points to the official repository.

```bash
git remote -v
git remote get-url origin
```

For an official `AnalyseDeCircuit/oxideterm` release, keep the official links and About attribution. For a fork release, all three checks below are blocking:

1. **Release-note ownership and provenance.** The notes must clearly state that the build is a community fork. In both language blocks, they must separately identify changes inherited from official upstream and changes implemented by the fork, following the provenance rules in the changelog-writing section. Compare the release range with official upstream history rather than inferring ownership from author names or commit-message wording. Support, documentation, issue, download, and changelog links must resolve to resources owned by the fork, not `AnalyseDeCircuit/oxideterm/issues`, the upstream changelog, upstream release downloads, or `oxideterm.app` documentation. The stable download composer currently derives upstream asset URLs from `.github/scripts/compose_release_notes.py`; a fork must redirect that source as well as the visible base-note links.
2. **No in-app updater.** The application does not ship an in-app updater; releases are distributed only as manual downloads from the release page. A fork must not reintroduce a self-update path that resolves to official endpoints: `rg -n 'releases/(latest|download)' crates/` must not show application code fetching updates, and no update-channel crate may return to the workspace.
3. **Help & About attribution.** Keep the existing `Copyright © <year> AnalyseDeCircuit` attribution; a fork must not replace or remove it. Add an adjacent small, localized line that clearly identifies the application as a fork and names the fork maintainer or project. Render it in the Help & About legal footer and add the corresponding key to all 11 locale catalogs.

Why this rule exists: users cannot tell an official release from a fork build when the fork reuses the same name, version numbers, and release notes. Unmarked fork releases route bug reports and support traffic into the upstream repository, confuse downloads, and misattribute defects to the upstream maintainers. A short fork-attribution line and fork-owned support links prevent that confusion at no cost to the fork, and they are the standard expectation for any GPL redistribution.

```bash
gh release list --repo <fork-owner>/oxideterm --limit 3
rg -n 'AnalyseDeCircuit/oxideterm|oxideterm\.app' \
  <composed-release-notes> <base-notes> .github/scripts/compose_release_notes.py
rg -n 'releases/(latest|download)' crates/ --glob '!vendor/**'
rg -n 'settings_view\.help\.copyright|AnalyseDeCircuit|fork' \
  crates/oxideterm-gpui-app/src/workspace/settings/pages/help.rs \
  crates/oxideterm-i18n/locales/*/settings_view.json
cargo test -p oxideterm-i18n locale_catalogs_have_the_same_complete_key_set
```

The `rg` commands are discovery checks: review each match rather than blindly requiring zero results, because the preserved copyright attribution must still name `AnalyseDeCircuit`. If a fork release fails any of the three ownership checks, treat publishing as blocked. Do not commit, push, or tag until the fork owns its release links and the About footer both preserves the upstream copyright and identifies the fork in all supported locales.

### 6. Review, commit, push, tag, and dispatch

Review the complete release diff and status before staging. Confirm that no secret, build artifact, unrelated file, or temporary release-notes file is included.

Stage only the reviewed release files and intended product changes, then inspect the exact commit payload:

```bash
git add -- <reviewed-files>
git diff --cached --stat
git diff --cached
```

Use the established release commit style:

```bash
git commit -m "Release OxideTerm <version>"
```

Push the branch before creating the tag. Then create an annotated tag on the verified release commit and push only that tag:

```bash
git push origin <branch>
git tag -a <tag> -m "OxideTerm <version>"
git push origin <tag>
```

Afterward, verify both refs, then build the release artifacts. This fork publishes the Windows x64 portable package through `.github/workflows/windows-x64-portable.yml`, which packages and verifies the selected commit and uploads the portable directory as a workflow artifact:

```bash
git rev-parse HEAD
git rev-list -n 1 <tag>
git ls-remote --heads origin <branch>
git ls-remote --tags origin refs/tags/<tag> refs/tags/<tag>^{}
gh workflow run windows-x64-portable.yml \
  --repo <owner/repo> \
  --ref main
git status --short --branch
```

Find the new `workflow_dispatch` run and verify that its `headSha` matches the release commit:

```bash
gh run list \
  --repo <owner/repo> \
  --workflow windows-x64-portable.yml \
  --branch main \
  --event workflow_dispatch \
  --limit 10 \
  --json databaseId,status,conclusion,headSha,displayTitle,url
```

The tag does not trigger packaging by itself, and the workflow does not create a GitHub Release. After the run succeeds, download the artifact, generate the checksum list with the platform checksum tool (`sha256sum` on Linux, `Get-FileHash` on Windows), and publish the release explicitly:

```bash
gh run download <run-id> \
  --repo <owner/repo> \
  --name OxideTerm-windows-x64-portable \
  --dir dist/portable
gh release create <tag> dist/portable/* sha256sums.txt \
  --repo <owner/repo> \
  --title "OxideTerm <version>" \
  --notes-file <composed-release-notes>
```

Confirm the published assets with `gh release view <tag> --repo <owner/repo>`.

## Recover an interrupted publish

Use this procedure only when the maintainer explicitly requests republishing the same version. The earlier packaging run must have been canceled or failed before the GitHub Release was created. If `gh release view <tag>` finds a published release or release assets may already have reached users, keep the tag immutable and publish the next patch version instead.

1. Fetch and record both the remote annotated tag object and its peeled commit before changing anything:

```bash
git fetch origin --tags
git ls-remote --tags origin refs/tags/<tag> refs/tags/<tag>^{}
gh run list --repo <owner/repo> --workflow windows-x64-portable.yml --limit 10
gh release view <tag> --repo <owner/repo>
```

Treat a missing GitHub Release as expected only when the packaging run was canceled or failed. When the existing tag already peels to the intended release commit, leave it unchanged and dispatch `windows-x64-portable.yml` again from `main`.

2. If the tag must move because the release commit itself changed, obtain explicit authorization to move it. Prepare and validate the corrected release normally, commit and push the branch, then re-fetch and verify that the remote tag object still equals the value recorded in step 1.

3. Recreate the annotated local tag on the verified release commit, then update the remote tag with a lease against the old annotated tag object, not the peeled commit:

```bash
git tag -fa <tag> -m "OxideTerm <version>" <release-commit>
git push --force-with-lease=refs/tags/<tag>:<old-tag-object> origin refs/tags/<tag>
```

Never delete the remote tag before recreating it; deletion creates an unprotected interval and loses the comparison guard. If the lease fails, fetch and stop to inspect who changed the tag.

4. Verify that the peeled tag resolves to the intended release commit, then dispatch a new packaging run and confirm its `headSha` matches the selected publishing branch revision:

```bash
git rev-parse <release-commit>
git rev-list -n 1 <tag>
git ls-remote --heads origin <branch>
git ls-remote --tags origin refs/tags/<tag> refs/tags/<tag>^{}
gh workflow run windows-x64-portable.yml \
  --repo <owner/repo> \
  --ref main
gh run list --repo <owner/repo> --workflow windows-x64-portable.yml --branch main --event workflow_dispatch --limit 10
```

Report the new run URL and status, then publish the release from the fresh artifact as described above. Do not keep monitoring it unless the user explicitly asks.

## Rebuild release assets

Use this procedure when the release exists but its assets are incomplete or wrong. This fork has no repair workflow, so the package is rebuilt and the assets are replaced explicitly.

1. Confirm the published release and its current assets:

```bash
gh release view <tag> --repo <owner/repo>
```

2. Dispatch `windows-x64-portable.yml` from `main`, wait for the successful run, and download its artifact:

```bash
gh workflow run windows-x64-portable.yml --repo <owner/repo> --ref main
gh run download <run-id> \
  --repo <owner/repo> \
  --name OxideTerm-windows-x64-portable \
  --dir dist/portable
```

3. Replace the assets and regenerate the checksum list:

```bash
gh release upload <tag> dist/portable/* sha256sums.txt --repo <owner/repo> --clobber
```

4. Verify the repaired assets on the release page or with:

```bash
gh release view <tag> --repo <owner/repo>
```

If the packaging run failed before producing an artifact, no asset exists for that build: fix the build and dispatch the workflow again.

## Failure handling

- If the branch push fails, do not create or push the tag.
- If the branch push succeeds but tag creation or push fails, do not dispatch packaging and report that partial state precisely.
- If the tag push succeeds but the packaging dispatch fails, leave the immutable tag in place, report the partial state, and retry the same explicit dispatch after resolving the workflow problem.
- If the tag already exists unexpectedly, compare its target with the intended commit and stop. Never retag without the maintainer's explicit same-version recovery authorization.
- If new commits or worktree changes appear during preparation, re-read status and regenerate the release range before publishing.
- If channel detection, previous-tag selection, or release scope is ambiguous, ask rather than guessing.
