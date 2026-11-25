# Releasing SebWatch

Every release ships with patch notes.

## Steps

1. Bump `version` under `[workspace.package]` in the root `Cargo.toml` and commit the
   updated `Cargo.lock`, by PR to `main`.
2. Write the patch notes in the format below.
3. Create the GitHub release as a draft, with the patch notes as its body:
   `gh release create vX.Y.Z --draft --title "SebWatch X.Y.Z" --notes-file <notes.md>`.
4. Tag the merged commit on `main`: `git tag vX.Y.Z && git push origin vX.Y.Z`. The tag starts
   the `Release packages` workflow (`.github/workflows/release.yml`): it builds
   `sebwatch-X.Y.Z-1.x86_64.rpm` on Fedora and `SebWatch-X.Y.Z-x86_64.AppImage` on the oldest
   supported Ubuntu, tests both in clean Fedora and Ubuntu containers (`scripts/test-packages`)
   and attaches them to the draft. Without a release for the tag it makes a draft with empty
   notes; it never publishes.
5. Once both packages are attached, publish it: `gh release edit vX.Y.Z --draft=false`.

## Packages

- `scripts/package rpm` (on Fedora, with `rpm-build`) or `scripts/package appimage` builds a
  package into `target/packages/`; `scripts/test-packages` tests both there. The rpm follows
  `packaging/sebwatch.spec`, the AppImage is made by a pinned linuxdeploy.
- The rpm installs with `sudo dnf install ./sebwatch-X.Y.Z-1.x86_64.rpm` or from the release
  URL, and removes with `sudo dnf remove sebwatch`. The AppImage runs where it is, after
  `chmod +x`.
- Win+Esc on KDE Plasma: the rpm's scriptlets run `sebwatch --bind-win-esc` on install and
  `--unbind-win-esc` on removal in the Plasma session of the user who ran `sudo dnf`;
  `scripts/install` and `scripts/uninstall` do the same into `~/.local`. The AppImage binds only
  when run with `--bind-win-esc`. Binding takes effect at once, without logging out.
  `scripts/test-win-esc` checks it (the shortcut active, not only recorded) on a headless KWin
  with its own D-Bus session and HOME, leaving the desktop and its shortcuts alone.
- To build and test both without a release, run the workflow by hand (Actions, `Release
  packages`, Run workflow): the packages stay as that run's artifacts. A PR that changes the
  packaging runs it too.

## Patch-note format

- Three sections: **New**, **Improved**, **Fixes**.
- For users only: leave out system changes (build, CI, refactors, internal docs).
- Most important items first. Each of those starts with a short **bold** sentence saying what
  the feature is generally about, followed by a few concise explanatory sentences in regular
  text.
- Other items: just concise explanatory sentences.

Example:

```markdown
## New

- **See where your RAM went.** The main window splits used memory by app, and the parts add
  up to the total. Background programs such as Android emulators get their own rows.
- The window remembers its size and position.

## Improved

- Memory numbers refresh without the list jumping around.

## Fixes

- Apps with several windows are no longer counted twice.
```
