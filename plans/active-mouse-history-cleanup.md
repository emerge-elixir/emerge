# Mouse input history cleanup

Preserve the reviewed tree, including the existing changelog/migration edits, on
a local backup branch before reconstructing history from origin/main.

Commit sequence:
1. Element-local logical pointer positions, transport and behavioral coverage.
2. Buffered pointer movement delivery without synthetic duplicates.
3. Captured drag lifecycle, including blur cleanup and invalid-geometry teardown.
4. Scrollbar safety for clipped and collapsed geometry.
5. Authoritative hover ownership across stale registry rebuilds.
6. Public reference/tutorial/migration docs, changelog and implementation notes.

Fold test pruning and review fixes into their owning commits, not cleanup commits.
Validate each code snapshot with Rust event/codec tests and relevant ExUnit tests.
Verify the assembled tree against the backup before moving the feature branch
to the reconstructed history. Do not push or alter other worktrees.

Validation: all five intermediate code snapshots passed Rust event tests and
relevant ExUnit tests. The assembled tree initially matched the backup exactly.
Final diff checking also exposed three leftover rebase-conflict marker lines in
the input guide; these were removed while preserving the DRM wake table row.
Apart from that documentation cleanup and this note, the final tree is unchanged.
