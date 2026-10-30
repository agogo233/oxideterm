# Release Process

This page is for maintainers preparing native releases. Contributors should not create release tags or dispatch publishing workflows unless explicitly asked.

## Prepare

1. Obtain explicit approval that the GUI build has been run successfully and is ready to publish. That approval is the release gate.
2. Start from an up-to-date `main` with a clean working tree. Fetch tags before deciding that a version is available.
3. Select one supported channel and its exact version/tag form:

   | Channel | Version | Tag | Changelog |
   | --- | --- | --- | --- |
   | Stable | `X.Y.Z` | `vX.Y.Z` | `stable-changelog.md` |
   | Beta | `X.Y.Z-beta.N` | `vX.Y.Z-beta.N` | `beta-changelog.md` |
   | GPUI preview | `X.Y.Z-gpui-preview.N` | `gpui-vX.Y.Z-gpui-preview.N` | `gpui-preview-changelog.md` |

4. Establish the changelog range with the repository helper rather than commit subjects alone:

   ```sh
   python3 .agents/skills/oxideterm-release/scripts/release_context.py \
     --repo . --channel <channel> --version <version>
   ```

5. Review the meaningful changes since that range and update the selected channel changelog under [`.github/release-notes/`](../../.github/release-notes/). Each entry begins with `## <version>` and contains matching `### 中文` and `### English` blocks, in the order expected by the composer.
6. Validate the intended version without writing files:

   ```sh
   python3 scripts/release/bump_version.py <version> --dry-run
   ```

7. Apply the version update with `scripts/release/bump_version.py <version>`. It updates the workspace version and lockfile; do not hand-edit those outputs. README badges display the latest GitHub release automatically and need no version edits.
8. Compose the release body into a temporary file with `.github/scripts/compose_release_notes.py`, then verify channel, language-block order, version section, and stable download links.
9. Review the complete release diff and stage only the intended release files. Commit and push the release preparation before creating an annotated tag on that verified commit.

## Publish

The Windows x64 portable package is built by `.github/workflows/windows-x64-portable.yml`. It packages the selected commit, verifies the package contents, and uploads the portable directory as a workflow artifact. It does not create a GitHub Release.

The tag does not publish by itself. After pushing the annotated tag, dispatch the packaging workflow explicitly:

```sh
gh workflow run windows-x64-portable.yml \
  --repo <owner/repo> \
  --ref main
```

After the run succeeds, download the artifact, generate the checksum list, and publish the release explicitly:

```sh
gh run download <run-id> --repo <owner/repo> --name OxideTerm-windows-x64-portable --dir dist/portable
gh release create <tag> dist/portable/* sha256sums.txt \
  --repo <owner/repo> \
  --title "OxideTerm <version>" \
  --notes-file <composed-release-notes>
```

## Failure Boundaries

- Distinguish compilation, packaging, binary verification, and upload failures before changing code. macOS packages use SDK 26.2 from Xcode 26.3 with a 13.0 deployment target for both architectures. Keep the Mach-O deployment target and `LSMinimumSystemVersion` aligned through `scripts/release/package_native.py`; changing only the plist does not change the executable.
- A packaging fix must be present in the commit used by a new dispatch. Re-running a job against an old commit does not pick up a later fix.
- If a tag exists unexpectedly, compare its annotated object and peeled commit with the intended release commit, then stop.
- If a packaging dispatch was canceled before a GitHub Release exists and the tag still points at the intended commit, leave the tag unchanged and dispatch a new packaging run.
- Moving an existing tag requires explicit authorization and a force-with-lease update against the recorded annotated tag object. Never delete the remote tag as a shortcut.
- If a GitHub Release already exists or release assets may have reached users, keep the tag immutable and publish the next patch version.
- If a completed packaging run produced artifacts but the release upload was incomplete, rebuild the package and replace the assets with `gh release upload --clobber`. A failed build produces no artifact, so fix the build and dispatch the workflow again.

## Verify And Recover

- Confirm the published tag, release notes, checksums, and all expected assets.
- Keep stable, beta, and GPUI preview channel notes separate. Do not publish a preview as a stable release.
- If a release must be rebuilt, inspect the existing tag and release assets before making any destructive GitHub change.

For a release that has already reached users, prefer a new patch release to retagging. The recovery path exists only for explicitly authorized, unpublished same-version repairs.

The detailed packaging implementation lives in `scripts/release/` and [`.github/workflows/windows-x64-portable.yml`](../../.github/workflows/windows-x64-portable.yml). Treat those files as the executable source of truth when this guide and workflow behavior differ.
