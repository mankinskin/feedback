tools/install/sync-copilot-surfaces.sh currently always deep-copies .agents/{agents,prompts,instructions} into .github/{agents,prompts,instructions}. This risks silent data loss: someone edits the generated .github/ copy, forgets the canonical .agents/ source, and the next sync run silently overwrites their edit.

Requested behavior (per user decision in session b9020ba2-df5d-426a-b1b9-228ef159cad1):
- For each synced directory, attempt to create a real symlink from .github/<dir> to .agents/<dir> first.
- Symlinks must never be committed to git — .gitignore already ignores /.github/agents/, /.github/prompts/, /.github/instructions/ regardless of entry type, so this is already safe, but must be re-verified after the change.
- If symlink creation fails (e.g. missing Windows Developer Mode/admin rights, or git core.symlinks issues), fall back to the existing behavior: copy the files, but mark them read-only after copying so accidental edits fail loudly instead of being silently lost.
- Print which mode (symlink vs read-only copy) was used for each directory so users understand why edits may or may not stick.