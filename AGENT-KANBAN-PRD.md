# Agent Kanban PRD

## Product Summary

Agent Kanban is a local-first kanban application for orchestrating AI coding agents against git repositories. The product lets a developer create task cards, start isolated coding attempts in git worktrees, stream agent execution, review code changes, provide follow-up feedback, and integrate completed work.

This is not a generic kanban board. The kanban board is the planning interface for a local coding-agent harness.

## Goals

- Turn a task card into an isolated git worktree and branch.
- Run coding agents safely inside that isolated workspace.
- Stream logs, status, and execution output in real time.
- Let users review diffs before integrating work through local merge, editor handoff, push, or PR.
- Support multiple attempts per task without contaminating the main checkout.
- Recover cleanly after app restarts, crashed commands, and abandoned worktrees.
- Make unsafe operations explicit: branch deletion, force push, merge, and command execution.
- Keep the product local-first, single-user, and understandable.

## Non-Goals

- Multi-user cloud collaboration.
- Organizations, billing, roles, or permissions.
- Remote-hosted workspaces.
- Custom cloud issue tracker.
- Full IDE replacement: editing, language intelligence, debugging, and project-wide navigation remain external/editor-owned.
- Complex automation marketplace.
- Multi-agent swarm orchestration.

## Primary User

A developer using AI coding CLIs who wants to parallelize implementation work while keeping each agent run isolated, inspectable, and recoverable.

## Recommended Stack

- Backend/UI: Phoenix + LiveView
- Persistence: Ecto + SQLite for local-first storage
- Realtime: Phoenix PubSub
- Runtime orchestration: DynamicSupervisor, Task.Supervisor, GenServer
- Styling: Tailwind
- Rich browser widgets: LiveView JS hooks
- Git/process execution: native Elixir wrappers around `git` and shell commands

Use LiveView for core stateful UI. Use JS hooks only where browser-native interactivity is clearly better: kanban drag/drop, terminal/log rendering, diff viewer, and preview iframe controls.

Stack decision:
- Choose Elixir/Phoenix for supervised process orchestration, explicit runtime state, PubSub, and a clean local web UI.
- Accept that `mix release` packages the BEAM runtime and is less minimal than a single Rust/Go binary.
- Keep JS-heavy widgets isolated behind LiveView hooks so LiveView owns workflow state while browser code owns rendering-heavy interactions.
- If terminal/diff/preview surfaces grow into a full IDE-like UI, revisit whether those surfaces should become a SPA island rather than forcing them through LiveView.

## Core Entities

### Project

A named container for one or more local repositories.

Fields:
- `id`
- `name`
- `created_at`
- `updated_at`

Relationships:
- has many repos
- has many tasks

### Repo

A registered local git repository.

Fields:
- `id`
- `project_id`
- `path`
- `name`
- `display_name`
- `default_target_branch`
- `default_working_dir`
- `setup_script`
- `cleanup_script`
- `dev_server_script`
- `created_at`
- `updated_at`

### AgentProfile

A named configuration for one executable coding agent. Model this explicitly so command execution is auditable.

Fields:
- `id`
- `name`
- `executor`
- `command_template`
- `default_args`
- `default_env`
- `dangerous_mode`
- `created_at`
- `updated_at`

Rules:
- The exact rendered command must be visible before first run.
- `dangerous_mode` must be explicit and user-controlled.
- Environment values must be redacted in UI when they look secret-like.
- Profiles should not store API keys directly.
- Install/auth detection can start as a runtime check against the configured command rather than persisted profile fields.

### Command Execution Contract

All external commands are rendered from an agent profile, repo script, or explicit user action into a durable execution process.

Command template variables:
- `{prompt_file}`: path to a temporary prompt file owned by the app
- `{worktree_path}`: absolute path to the attempt worktree
- `{repo_path}`: absolute path to the primary repo inside the worktree
- `{task_title}`: task title
- `{attempt_branch}`: generated attempt branch
- `{target_branch}`: target branch

Rules:
- Prefer passing large prompts by file instead of shell-escaping prompt text.
- Template rendering must fail closed if required variables are missing.
- Unknown template variables should produce a validation error.
- Commands must be rendered into argv where possible; shell strings are allowed only when explicitly configured.
- Rendered command, cwd, environment snapshot, and prompt reference must be persisted with the execution process.

### Task

A kanban card representing desired work.

Fields:
- `id`
- `project_id`
- `title`
- `description`
- `status`
- `created_at`
- `updated_at`

Statuses:
- `todo`
- `in_progress`
- `in_review`
- `done`
- `cancelled`

### Attempt

One isolated run against a task. Each attempt owns a git worktree, branch, runtime state, and execution history.

Fields:
- `id`
- `task_id`
- `repo_id`
- `branch`
- `target_branch`
- `base_commit`
- `head_commit`
- `worktree_path`
- `agent_working_dir`
- `status`
- `setup_completed_at`
- `archived`
- `pinned`
- `name`
- `created_at`
- `updated_at`

Relationships:
- belongs to task
- belongs to repo
- has many execution processes

Statuses:
- `created`
- `preparing`
- `running`
- `needs_review`
- `failed`
- `cancelled`
- `merged`
- `archived`

### ExecutionProcess

A durable record of a command or agent run.

Fields:
- `id`
- `attempt_id`
- `agent_profile_id`
- `run_reason`
- `status`
- `command`
- `cwd`
- `env_snapshot`
- `prompt_path`
- `prompt_snapshot`
- `agent_session_ref`
- `agent_message_ref`
- `summary`
- `exit_code`
- `pid`
- `started_at`
- `completed_at`
- `created_at`
- `updated_at`

Run reasons:
- `setup_script`
- `coding_agent`
- `follow_up`
- `cleanup_script`
- `dev_server`

Statuses:
- `queued`
- `running`
- `completed`
- `failed`
- `killed`

### ExecutionLog

Append-only output from an execution process.

Fields:
- `id`
- `execution_process_id`
- `stream`
- `sequence`
- `content`
- `inserted_at`

Streams:
- `stdout`
- `stderr`
- `system`

Logs must be durable enough to inspect after reloads and ordered enough to replay faithfully in the UI.

Storage strategy:
- store execution metadata in SQLite
- store large log bodies as append-only files under the app data directory
- store DB offsets/checkpoints so logs can be replayed incrementally
- enforce a configurable maximum log size per execution process
- show truncation clearly if logs exceed the configured limit
- apply backpressure or batching so high-volume output does not freeze LiveView
- keep raw log storage separate from display normalization

### Merge

Records successful integration of an attempt into a target branch or PR.

Fields:
- `id`
- `attempt_id`
- `repo_id`
- `type`
- `target_branch`
- `merge_commit`
- `pr_url`
- `pr_status`
- `created_at`
- `updated_at`

### AppConfig

Local user and runtime configuration.

Fields:
- `workspace_root`
- `default_executor`
- `editor_command`
- `shell`
- `max_concurrent_attempts`
- `log_retention_days`
- `allow_dangerous_agent_flags`

Secrets and auth tokens must not be stored in plaintext app config. The app should rely on agent CLI authentication; any future app-managed credentials must use OS keychain integration.

### Expanded Model

The core model intentionally keeps attempts single-repo and records agent context directly on execution processes. If the product needs richer workflows, add these as extensions rather than starting with them:

- `AttemptRepo`: junction table for multi-repo attempts with per-repo target branch and base/head commits.
- `AgentSession`: durable conversation thread when multiple agent conversations per attempt become necessary.
- `AgentTurn`: extracted prompt/response record if execution processes become too broad for conversation history.

Do not introduce these tables until the product has concrete behavior that needs them.

## Product Constraints

The product is local-first and single-user. It may support multiple projects, repos, attempts, and agent profiles, but the default experience should keep the core loop obvious: create task, start attempt, review diff, integrate work.

Constraints:
- one local user
- app-owned worktrees live under `workspace_root`
- only one mutating process per attempt at a time
- every external command has a durable execution record

Local trust boundary:
- the app binds to localhost by default
- requests from browsers must pass origin/host validation
- destructive POST actions should use LiveView events or CSRF-protected routes
- no unauthenticated LAN exposure by default
- if user enables non-localhost binding, show a warning that local repos and command execution become network-reachable

Data invariants:
- repo paths are unique after canonicalization
- project names are unique enough for navigation but do not need to be globally unique
- task belongs to exactly one project
- attempt belongs to exactly one task
- attempt belongs to exactly one repo in the core model
- execution log sequence is unique by `(execution_process_id, sequence)`
- execution processes may not transition from terminal states back to running
- merge records are append-only integration history

Privacy and telemetry:
- no source code, prompts, logs, diffs, repo paths, branch names, or task descriptions should leave the machine by default
- telemetry is off by default
- if telemetry is added, it must be opt-in, clearly described, and limited to coarse product/runtime events
- crash/error reports are opt-in and must exclude source code, prompts, logs, diffs, repo paths, branch names, task descriptions, and environment values
- diagnostics exports must redact secrets and allow the user to inspect before sharing
- agent CLIs may contact external services; the app must make that trust boundary clear during agent profile setup

Secrets:
- the app should not store agent API keys directly
- prefer the agent CLI's own authentication mechanism
- if the app later stores credentials, use OS keychain integration rather than SQLite or plaintext config
- environment variables shown in the UI must be redacted when names or values look secret-like

Cross-platform constraints:
- path handling must work on macOS and Linux
- Windows support runs through WSL and should be treated as Linux inside the app
- do not assume POSIX shell semantics unless the configured shell is POSIX-like
- command templates must distinguish argv execution from shell execution
- process termination must account for process groups/child processes
- path comparisons for workspace containment must use canonical paths
- native Windows paths, drive-letter casing, and Windows process management are out of scope unless native Windows support is explicitly added later

## Core User Flows

### Create Project

1. User creates a project.
2. User registers one or more local git repositories.
3. System validates paths and detects current branches.
4. User optionally configures setup, cleanup, and dev server scripts.

### First-Run Setup

1. User chooses or confirms the workspace root.
2. System verifies git availability.
3. User creates or selects an agent profile.
4. System runs install/auth checks for the selected profile.
5. App clearly explains that agent CLIs may contact external services and run commands in the selected worktree.
6. User creates a project and registers a repo.
7. User sees a ready state with the next action: create a task.

First-run setup must be skippable only when defaults are valid. If setup is incomplete, the app should show a precise blocker instead of failing later during attempt creation.

### Create Task

1. User clicks create task.
2. User enters title and optional description.
3. Task appears in `todo`.
4. User may create only the task or create and start immediately.

### Start Attempt

1. User selects a task.
2. User creates an attempt.
3. User chooses agent profile and target branch.
4. System creates a branch name from task title and attempt id.
5. System runs preflight checks.
6. System creates git worktree(s).
7. System creates an attempt record.
8. System starts the coding agent.
9. Task moves to `in_progress`.

Preflight checks:
- repo path exists and is a git repository
- target branch exists locally or remotely
- current git executable is available and supported
- generated branch name is not already in use
- worktree destination does not already exist
- git worktree metadata is not corrupt
- required agent executable is available
- workspace root is writable
- optional setup script is present and executable by the configured shell

Remote/fetch policy:
- do not fetch automatically unless the user explicitly requests it or enables auto-fetch
- if target branch only exists on remote, show that a local tracking branch or worktree can be created from it
- show stale remote information as a warning, not a blocker
- network-dependent git operations should be clearly labeled

Dirty source check:
- The original checkout may be dirty.
- Creating a worktree is still allowed if git can safely create it from the target branch.
- The UI must clearly show that the source checkout has unrelated uncommitted changes if that matters for the selected branch.

Out-of-band changes:
- users may edit files directly inside attempt worktrees
- the app treats worktree contents as source of truth for diff and merge
- if files change while a process is running, the app does not try to attribute every change to the agent
- before follow-up or merge, show current dirty/untracked state
- if the app detects the worktree branch changed outside the app, reconcile metadata and show a warning

Branch naming:
- default format: `ak/{short_attempt_id}-{slugified_task_title}`
- title slug should be lowercase, ASCII, dash-separated, and length-limited
- branch names must be validated with `git check-ref-format`
- on collision, append a short numeric or random suffix
- branch name is generated once and stored on the attempt

Worktree placement:
- all app-created worktrees live under `workspace_root`
- default path format: `{workspace_root}/{project_slug}/{attempt_id}`
- deleting an attempt may delete this path only if it is under `workspace_root`
- paths outside `workspace_root` are never deleted automatically

### Monitor Execution

1. User opens task attempt panel.
2. UI streams logs and process status.
3. Runtime updates execution process state.
4. On completion, task moves to `in_review`.
5. On failure or cancellation, task remains inspectable with logs.

Execution controls:
- user can stop a running process
- stop first sends graceful termination
- after timeout, stop escalates to force kill
- process status must be reconciled after app restart
- running processes from a previous app instance must be detected as gone or reattached if possible

### Review Changes

1. User opens diff panel.
2. System computes diff between worktree branch and target branch.
3. User reviews changed files and stats.
4. User may inspect the diff and either stop, delete, open in editor, merge, or send follow-up feedback.
5. Follow-up creates a new execution process for the same attempt.

Diff semantics:
- compare the attempt worktree against the target branch recorded on the attempt
- include committed and uncommitted worktree changes
- show file status from `git status --porcelain`
- show textual diff from target branch to current worktree
- show binary files as changed but not render binary content
- cap rendered diff size per file and for the whole attempt
- show explicit "diff truncated" indicators when limits are hit
- ignore files according to git status; do not invent separate ignore semantics
- include untracked files when feasible, but cap size and never inline very large untracked files
- store `base_commit` at attempt creation so stale target branches can be detected
- if target branch moves after attempt creation, show a stale-base warning instead of silently changing the diff baseline

### Integrate Work

1. User chooses local merge or open in editor.
2. System verifies branch state.
3. System merges locally when the user confirms.
4. On successful merge, task moves to `done`.
5. Attempt may be archived automatically unless pinned.

Integration checks:
- refuse direct merge if target branch is remote-only
- refuse merge if worktree has unresolved conflicts
- show ahead/behind status before merge
- show changed files and commit range before merge
- refuse merge if target branch moved and user has not acknowledged stale-base warning
- preserve user's uncommitted work in the attempt unless merge flow explicitly commits or refuses it

Integration behavior:
- direct local merge is allowed only into a local target branch
- open-in-editor is acceptable as the fallback integration path
- push and PR creation are optional integration paths

## Prompt Contract

Prompt construction must be deterministic and inspectable. The app should show the final prompt or a preview before sending it to an agent.

Initial agent prompt includes:
- task title
- task description
- project name
- repo name
- worktree path
- target branch
- generated attempt branch
- explicit instruction to work only inside the worktree
- optional setup notes from the repo/profile

Follow-up prompt includes:
- user follow-up text
- current task title
- current attempt branch
- relevant review comments, if any
- optional summary of previous execution when available

Prompt construction must not include:
- unrelated task data
- secrets from config or environment
- full logs unless the user explicitly includes them
- arbitrary files unless selected by the user or referenced by the task context

The product does not need complex prompt templating initially. It needs a stable default template and an inspectable final payload.

Prompt audit:
- store the final prompt payload or prompt-file path with the execution process
- show the prompt used for each agent run in the UI
- if the prompt references generated files, keep those files until the execution record is pruned

## State Machines

### Task Status Rules

- `todo`: task exists but has no active attempt.
- `in_progress`: at least one attempt has a running setup, agent, follow-up, or cleanup process.
- `in_review`: latest meaningful attempt process completed, failed, or was killed and awaits user review.
- `done`: user merged locally or confirmed external PR merge.
- `cancelled`: user explicitly marked the task as not worth doing.

Manual dragging changes task status only. It must not start, stop, merge, push, or delete anything.

### Attempt Status Rules

- `created`: DB record exists, worktree may not exist yet.
- `preparing`: worktree/setup is in progress.
- `running`: setup, agent, follow-up, cleanup, or dev server process is running.
- `needs_review`: no active process and changes/logs are ready for inspection.
- `failed`: setup or agent failed before producing reviewable output.
- `cancelled`: user stopped execution.
- `merged`: work was integrated.
- `archived`: hidden from active attempt lists but retained for history.

Attempt status is a cached, denormalized value derived from durable execution records and integration state. Execution records remain the source of truth. Startup recovery must reconcile `Attempt.status` if it drifts.

## Failure Taxonomy

Failures should map to actionable UI messages and durable error codes.

Required failure codes:
- `git_executable_not_found`
- `repo_not_found`
- `not_git_repository`
- `target_branch_not_found`
- `remote_unavailable`
- `branch_already_exists`
- `invalid_branch_name`
- `worktree_create_failed`
- `worktree_missing`
- `agent_executable_not_found`
- `agent_auth_required`
- `setup_failed`
- `process_nonzero_exit`
- `process_killed`
- `process_timeout`
- `git_conflict`
- `stale_target_branch`
- `diff_too_large`
- `merge_refused`
- `log_limit_exceeded`
- `unknown`

Each failure should store:
- user-facing message
- technical detail
- suggested next action
- related entity ids

## Kanban UI Requirements

- Board has fixed columns: Todo, In Progress, In Review, Done, Cancelled.
- Cards show title, attempt state, latest executor, and failure/running indicators.
- Cards show whether the task has zero, one, or multiple attempts.
- Cards show stale review state when new commits appear after the last agent run.
- Clicking a card opens the task detail panel.
- Task detail shows task fields and attempts table.
- Attempt detail shows conversation/logs, diffs, and actions.
- Preview is useful but should not block the core task/attempt/diff loop.
- Dragging cards updates task status only; it does not trigger automation.
- Board updates live when executions change task status.
- Empty states must explain the next action: create task, start attempt, review diff, or merge.
- Running states should be visible without opening the task.
- Terminal/error states should have one clear primary recovery action.
- The selected task/attempt should be URL-addressable so reloads preserve context.

## Safety Requirements

- Agents must run only inside the attempt worktree by default.
- The app must display the exact command before first execution of an agent profile.
- Dangerous agent flags must be visible in profile configuration.
- The app must never silently delete user-created files outside its workspace root.
- Environment variables passed to agents must be inspectable, with secrets redacted.
- Logs must redact known secret patterns where practical, but users should be warned that command output may contain secrets.
- The app should treat local scripts as arbitrary code and make that clear in setup.
- The product does not promise OS-level sandboxing.
- Any command run by the app should have an explicit cwd under the attempt worktree unless it is a read-only repo discovery command.
- The app should warn before using agent flags that bypass approval prompts.
- Opening files in an editor must use an explicit user-configured editor command.
- The editor command must be previewed before first use and stored in app config.
- Repo discovery commands must be read-only.
- Browser-accessible routes must not expose arbitrary filesystem reads. File browsing should be constrained to user-selected repo roots and app-owned directories.

## Destructive Action Rules

Destructive and irreversible operations require explicit confirmation that names the affected resource.

Operations requiring confirmation:
- merge into target branch
- push branch
- force push branch
- delete branch
- delete worktree
- delete attempt
- delete task with attempts
- reset app metadata
- reset app metadata and app-owned worktrees

Confirmation copy must include:
- affected repo
- affected branch
- affected filesystem path when applicable
- whether the action can be undone
- whether the operation touches files outside the app data directory

## Recovery And Cleanup

On app startup:
- scan DB for `running` execution processes
- mark missing OS processes as `failed` or `killed`
- verify attempt worktree paths still exist
- surface missing/corrupt worktrees in the UI
- reconcile branch/head commit metadata
- clean abandoned temp files owned by the app
- verify app data and workspace root permissions

Cleanup actions:
- archive attempt: hide from active UI, keep DB records and worktree unless configured otherwise
- delete attempt: delete DB records and optionally delete worktree/branch
- delete task: require confirmation and explain impact on attempts/worktrees
- prune orphan worktrees: only for paths inside configured workspace root

Data locations:
- SQLite database lives under the app data directory
- log files live under the app data directory
- worktrees live under `workspace_root`
- generated temp files live under an app-owned temp directory

Reset/export:
- user can reveal app data directory
- user can export SQLite database and app config
- user can reset app metadata without deleting worktrees
- user can reset app metadata and app-owned worktrees with separate confirmation

Migrations:
- schema migrations must be automatic on startup
- failed migrations must stop startup with a clear diagnostic message
- backups before destructive migrations are strongly preferred
- app should never silently delete old metadata during migration

## Concurrency Model

- Multiple attempts may run concurrently.
- A configurable global concurrency limit prevents accidental resource exhaustion.
- Only one mutating execution process should run per attempt at a time.
- Dev server may run alongside an idle attempt, but not block follow-up unless the configured command requires exclusive access.
- Follow-up requests against a running agent are queued or rejected explicitly.
- AttemptServer should serialize state transitions for a single attempt.

Queued/rejected states must be visible instead of silently ignored.

## Runtime Architecture

### Supervision Tree

```text
App.Supervisor
  App.Repo
  Phoenix.PubSub
  App.Runtime.AttemptSupervisor
  App.Runtime.CommandSupervisor
```

### AttemptServer

One GenServer per active attempt.

Responsibilities:
- owns current runtime state for the attempt
- starts setup, agent, follow-up, cleanup, and dev server commands
- receives process output
- writes log chunks and status changes to DB
- broadcasts PubSub events
- handles cancellation
- enforces per-attempt execution serialization
- reconciles durable state on start
- exits when attempt is idle

### CommandRunner

Runs one external process.

Responsibilities:
- spawn command with cwd/env
- stream stdout/stderr
- capture exit status
- support cancellation through OS process termination
- emit normalized events to AttemptServer
- persist enough metadata to debug what command actually ran

Execution modes:
- non-interactive command mode is required
- PTY mode is optional and may be needed for some agents
- commands must receive explicit cwd, env, argv, and timeout settings
- shell execution should be opt-in; prefer argv execution where possible
- if shell execution is used, render the exact shell command in the UI

Timeouts:
- default timeout may be disabled for coding agents
- setup/cleanup scripts should have configurable timeouts
- stop action must work even when timeout is disabled

### Event Model

Runtime events must be append-only and idempotent enough for LiveView reconnects.

Event types:
- `execution_started`
- `execution_log_appended`
- `execution_completed`
- `execution_failed`
- `execution_killed`
- `attempt_status_changed`
- `task_status_changed`
- `diff_changed`

Rules:
- every log event has a monotonically increasing sequence per execution process
- every status transition is persisted before broadcasting
- LiveViews must reload canonical state from the database on mount
- PubSub is for freshness, not the source of truth
- PubSub messages may be dropped; UI must tolerate missed events
- repeated stop/merge/delete actions must be idempotent or return a clear already-completed response

### PubSub Topics

Suggested topics:

```text
project:{project_id}:tasks
task:{task_id}
attempt:{attempt_id}:status
attempt:{attempt_id}:logs
attempt:{attempt_id}:diff
execution:{execution_process_id}
```

## Diagnostics

The app needs a local diagnostics page early, because most failures will be environment-specific.

Diagnostics should show:
- app version
- app data directory
- workspace root
- SQLite database path
- git version
- detected shell
- detected agent profiles and availability
- configured editor command
- configured max concurrency and log limits
- active execution processes
- recent failed execution processes
- orphan worktrees owned by the app
- last startup recovery summary

Diagnostics actions:
- copy diagnostics summary
- open app data directory
- open workspace root
- rerun agent availability checks
- run git health check for a repo
- validate workspace root permissions

## UX Requirements

- The app should always show what is running and why.
- Primary actions should be state-aware: create task, start attempt, stop process, review diff, merge.
- Dangerous actions should state the exact affected branch/path.
- Long-running actions should have progress or live output.
- Errors should include a next step, not just a stack trace.
- Empty states should be operational, not decorative.
- Keyboard navigation should cover board navigation, create task, open task, close panel, and stop process.
- The UI should remain usable when logs are large, diffs are large, or an agent is noisy.
- Reloading the page should preserve selected project/task/attempt through the URL.

## API / LiveView Boundaries

Prefer LiveView events for first-party UI actions:
- create task
- update task
- create attempt
- archive attempt
- merge attempt
- start follow-up

Use JSON endpoints only where LiveView hooks or browser-native capabilities need them:
- log stream fallback
- terminal websocket
- diff payloads
- filesystem/repo picker
- agent callbacks if required

External automation APIs are out of scope for this spec. The local UI and runtime contracts should not depend on MCP or any external automation client.

## JS Hooks

Use hooks for:
- kanban drag/drop
- terminal/log virtualized rendering
- diff viewer
- editor-like text input if needed
- preview iframe/device controls

Do not build a full SPA unless the product becomes IDE-like enough to justify client-side state ownership.

## Build Slice

The PRD describes the final product shape. The first implementation should still build the thinnest coherent slice that proves the core loop:

1. Create project.
2. Register one local repo.
3. Configure one agent profile.
4. Create task.
5. Create attempt.
6. Create git worktree and branch.
7. Run one configured agent command.
8. Stream logs into LiveView.
9. Persist execution status.
10. Show diff.
11. Stop a running command.
12. Recover status correctly after app restart.
13. Allow manual merge or open in editor.

Do not let optional surfaces obscure the core loop. PR creation, subtasks, multi-repo attempts, dev server preview, and review comments should come after the basic attempt lifecycle is trustworthy.

Keep out of the first build slice:
- follow-up prompts
- setup/cleanup scripts
- multiple attempts per task
- concurrent running attempts
- PTY terminal
- custom prompt templates

## Acceptance Criteria

- A user can create a task and start an attempt against a local repo.
- A user can inspect the rendered agent command and prompt before first execution.
- The app creates a worktree under the configured workspace root.
- The original checkout remains unchanged by agent execution.
- Logs stream live and remain visible after page reload.
- Killing the browser tab does not kill the running process.
- Restarting the app does not lose completed/failed execution state.
- User can inspect the diff between attempt branch and target branch.
- User can stop a running command.
- User can delete an attempt without deleting unrelated files.
- Missing agent executable, branch conflicts, worktree failures, and command failures produce actionable errors.
- No merge, push, force push, or deletion happens without explicit user action.
- A diagnostics screen can explain the current environment and recent failures.

## Testing Strategy

Test coverage should focus on the parts that can destroy user trust.

Required automated tests:
- branch name generation and validation
- worktree path containment under `workspace_root`
- command template validation and rendering
- repo preflight checks
- task and attempt state transitions
- execution process lifecycle
- log ordering and replay
- log truncation behavior
- cancellation state handling
- startup recovery for stale `running` processes
- diff baseline behavior when target branch moves
- large/binary diff truncation
- delete attempt does not remove paths outside `workspace_root`
- local origin/host validation for HTTP requests
- canonical path containment across symlinks
- out-of-band worktree changes before merge
- first-run setup blockers

Required manual test scenarios:
- missing git executable
- missing agent executable
- agent exits nonzero
- agent outputs very large logs
- binary and large generated files in diff
- user closes browser during execution
- app restarts after execution completes
- app restarts while execution is marked running
- worktree folder is manually deleted
- target branch is deleted or renamed
- target branch advances after attempt starts
- merge conflict during integration
- symlink inside workspace root points outside workspace root
- user edits attempt files manually while app is open
- non-localhost binding warning
- WSL path with spaces if Windows-through-WSL is supported

## Packaging And Local Runtime

The product should be shippable as a local developer tool.

Requirements:
- app binds to localhost by default
- app selects a free port or allows a configured port
- app prints the URL on startup
- app can open the browser automatically when requested
- static assets are bundled into the release
- app data directory is deterministic per OS
- workspace root is configurable
- logs explain where data and worktrees live
- app should handle port conflicts with a clear message or automatic fallback
- app should not require admin/root privileges

Phoenix release considerations:
- use `mix release` for packaged builds
- bundle compiled assets before release
- document required system dependencies: git, chosen agent CLI, shell
- avoid requiring Postgres for local use

## Optional Surfaces

These capabilities belong in the product, but they should not obscure the core loop:
- multiple attempts per task
- follow-up prompts that resume or continue prior agent context
- setup and cleanup scripts
- branch push
- PR creation through GitHub CLI
- attempt archive/pin
- tags/snippets
- multi-repo attempts
- subtasks linked to parent attempts
- preview/dev server panel
- inline diff comments
- rebase/conflict workflows
- agent profile variants
- persistent terminal
- import tasks from markdown/specs

## Key Product Principles

- Isolation first: never let an agent mutate the user's main checkout.
- Durable state first: every process and attempt should survive page reloads.
- Explicit integration: agents do not push, merge, or create PRs without user action.
- Logs are product surface, not debug output.
- Failed attempts are useful artifacts, not garbage.
- Keep the domain small until the core loop feels excellent.
