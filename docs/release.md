# Releasing SebWatch

Every release ships with patch notes.

## Steps

1. Bump the top-level CMake project version (`project(SebWatch VERSION X.Y.Z ...)`),
   by PR to `main`.
2. Write the patch notes in the format below.
3. Tag the merged commit on `main`: `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. Create the GitHub release with the patch notes as its body:
   `gh release create vX.Y.Z --title "SebWatch X.Y.Z" --notes-file <notes.md>`.

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
