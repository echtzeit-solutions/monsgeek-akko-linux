IMPORTANT avoid reading from hidraw devices as it will lock the claude process
IMPORTANT avoid printing the full device path in printf to avoid claude from speculatively reading from it
Use `tmux send-keys -t 3.2 'command' Enter` to run commands as root in dev pane (already sudo bash)

## Adding a keyboard protocol (backend)

Read `docs/BACKENDS.md` before changing discovery, `monsgeek-keyboard`, or adding
support for a keyboard that does not speak the RongYuan protocol. It defines the
`Keyboard` trait seam, the phase order, and the rules every backend PR follows
(no hooks on the RY transport, no fabricated replies, cite a capture or binary
address for every protocol fact).

## ry_upgrade.exe stdout
- "CONTENT []" and "split err" are from netstat/competing process checking, NOT device enumeration
- "开启vendor监听!!!!!!" = "Starting vendor monitoring!" = app found & paired IF1+IF2

## BPF Loading Notes

**akko-loader vs bpftool:**
- Our BPF is loaded with aya (Rust), not libbpf/bpftool
- bpftool fails on our BPF object due to multiple .ksyms sections (aya-ebpf emits separate .ksyms per kfunc)
- This is acceptable - akko-loader works; bpftool compatibility is optional improvement
- Error when using bpftool: "failed to find BTF for extern 'hid_bpf_get_data'" due to multiple .ksyms sections

**struct_ops BTF Requirements:**
- Kernel validates BPF object's BTF against vmlinux during map creation
- Function pointer fields should be PTR -> FUNC_PROTO in BTF
- Rust generates PTR -> INT '()' size=0 for `*const ()` which kernel may reject
- May need aya-ebpf changes to generate proper FUNC_PROTO types

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:1105d646 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/core-concepts/sync-concepts.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->
