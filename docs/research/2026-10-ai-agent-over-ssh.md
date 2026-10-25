# AI Agent Operation on Remote Servers over SSH — Architecture Research

**Date:** 2026-10-25
**Project:** LiteTerm (Tauri 2 + russh + Vue 3, Termius clone)
**Research question:** What is the best architecture for letting an AI agent operate on REMOTE servers over SSH from a local desktop app — instead of installing an agent CLI on every server?

**Context:** LiteTerm's Rust backend already has an SSH session pool with PTY, writer injection, stdout events, and session logging.

---

## TL;DR Recommendation

**Recommended: Option B — a local headless agent sidecar (opencode by default, `claude -p` / `codex exec` as alternates) driving a LiteTerm-provided MCP SSH-exec tool that routes into LiteTerm's existing russh session pool.**

Why this wins:

1. **Zero server-side install** — the exact pain point. The agent brain runs on the user's desktop; every command goes through LiteTerm's existing SSH connections. This is the same architecture class Termius uses for its AI Agent (local app proposes/executes over its own sessions with human confirmation — [Termius AI Agent blog](https://termius.com/blog/ai-agent)).
2. **You don't rebuild the agent brain.** opencode is MIT-licensed, supports 75+ providers, has a first-class headless server (`opencode serve`, OpenAPI + SSE), JSON event output (`opencode run --format json`), and rich MCP client config — see findings below. Claude Code (`claude -p --output-format stream-json`) and Codex (`codex exec --json`) have equivalent headless modes.
3. **Approval gates are a solved problem**: opencode exposes permissions over its server API; Claude Code can delegate permission prompts to an MCP tool (`--permission-prompt-tool`) — meaning LiteTerm's UI can be the human-in-the-loop approver.
4. **Every command is visible in the user's terminal** if you route tool calls through the session pool (Termius-style transparency), while using SSH `exec` channels for clean stdout/stderr/exit-status semantics rather than PTY scraping.

Fallbacks: a minimal built-in Rust agent loop (Option A) only as a "no-sidecar quick assist" mode; the tmux/agent-on-server pattern (Option C) only as an optional add-on for users who specifically want persistent agents that survive disconnects. Warp's remote-companion model (a small server installed on each host) is the strong counter-argument if you later want file-tree/editing features — but it reintroduces per-server install, which is what we're eliminating.

---

## 1. opencode (sst/opencode, now anomalyco/opencode)

**Verdict: the best sidecar candidate. MIT, headless-first, MCP-native, provider-agnostic.**

- **Repo & license:** [anomalyco/opencode](https://github.com/anomalyco/opencode) — "The open source coding agent", **MIT license**, ~221k stars, actively pushed (verified via GitHub API, 2026-10-25).
- **Headless server:** `opencode serve` runs a headless HTTP server exposing an **OpenAPI 3.1 spec** at `http://<hostname>:<port>/doc` (default port 4096) and streams events via **SSE** (`GET /event`). The API covers sessions (create/fork/abort/revert/**permissions**), messages (sync `POST /session/:id/message` and async), shell commands, files, config, MCP/LSP status. Auth: HTTP basic auth via `OPENCODE_SERVER_PASSWORD` ([docs/server](https://opencode.ai/docs/server)).
- **Non-interactive runs:** `opencode run [message..]` with `--format json` (raw JSON events), `--model`, `--agent`, `--session`/`--continue`/`--fork`, `--file`, `--title`, `--auto` (auto-approve), and crucially **`--attach http://localhost:4096`** to connect to an already-running server (avoids cold-boot delays) plus `--username/--password` for auth ([docs/cli](https://opencode.ai/docs/cli)).
- **MCP client:** full support in `opencode.json` under the `mcp` key — `"type": "local"` (stdio `command` array + `environment` + `cwd` + `timeout`) or `"type": "remote"` (URL + headers, automatic OAuth via RFC 7591 DCR), per-server `enabled` toggles, and **per-agent tool permissions with glob patterns** (`"tools": { "my-mcp*": false }`) ([docs/mcp-servers](https://opencode.ai/docs/mcp-servers/), [docs/config](https://opencode.ai/docs/config), [github/github-mcp-server opencode install guide](https://github.com/github/github-mcp-server/blob/main/docs/installation-guides/install-opencode.md)).
- **Remote execution story:** opencode does **not** execute on a remote machine by itself. Community patterns are (a) install opencode on the remote box and `opencode serve` there, then client-attach over SSH tunnel ([r/opencodeCLI thread](https://www.reddit.com/r/opencodeCLI/comments/1qdc89a/opencode_to_run_commands_on_remote_server)), or (b) give a local opencode an SSH MCP tool — exactly Option B. There is an open issue about SSH-based remote server connections to the desktop app ([issue #7790](https://github.com/anomalyco/opencode/issues/7790)).
- **Security caution:** exposing `opencode serve` on a network has had RCE-class findings — keep it on localhost inside LiteTerm ([HN discussion](https://news.ycombinator.com/item?id=46581095)).
- **Embeddability:** the OpenAPI 3.1 spec "powers SDK generation" ([docs/server](https://opencode.ai/docs/server)) — LiteTerm's Rust backend can talk to the sidecar over plain HTTP/SSE with a generated client, no Node SDK needed.

## 2. Claude Code (and Codex CLI / Gemini CLI equivalents)

**Claude Code — official headless patterns:**

- `claude -p` (print mode) is non-interactive; `--output-format text|json|stream-json` and `--input-format text|stream-json` (streaming requires `--verbose`). Piping supported (`cat file | claude -p "query"`) ([CLI reference](https://code.claude.com/docs/en/cli-reference)).
- Safety/automation flags: `--allowedTools "Bash(git log *)" "Read"`, `--disallowedTools`, `--tools` (restrict which tools exist), `--permission-mode default|acceptEdits|plan|auto|dontAsk|bypassPermissions`, **`--permission-prompt-tool mcp__<server>__<tool>`** — delegate permission prompts to an MCP tool (this is how a GUI becomes the approver), `--max-turns`, `--max-budget-usd`, `--json-schema`, session resume via `--continue`/`--resume`/`--session-id`, `--mcp-config` for MCP servers ([CLI reference](https://code.claude.com/docs/en/cli-reference)).
- **Agent SDK:** the "Claude Code SDK" was renamed to **Claude Agent SDK** (early 2026), TypeScript + Python; it **spawns the Claude Code CLI as a subprocess** and exposes the same agent loop, tools, permissions, sessions, hooks ([TS reference](https://code.claude.com/docs/en/agent-sdk/typescript), [claude-agent-sdk-python](https://github.com/anthropics/claude-agent-sdk-python), [ksred.com analysis](https://www.ksred.com/the-claude-agent-sdk-what-it-is-and-why-its-worth-understanding)).
- Billing note: since June 15, 2026, Agent SDK / `claude -p` headless usage is metered in a **separate credit pool** at API rates, not the subscription ([totalum.app summary](https://www.totalum.app/blog/claude-agent-sdk-credits-2026)).
- **Official SSH story:** there is no first-class "SSH remote runtime"; the documented pattern is installing Claude Code on the remote host and running it there, often inside tmux, with `claude -p` for scheduled/headless jobs ([lumadock VPS guide](https://lumadock.com/tutorials/run-claude-code-on-vps)). Community tools like [mjmirza/headless-claude](https://github.com/mjmirza/headless-claude) do `ssh user@server "claude -p '...'"` — i.e., agent-on-server, which is what we're avoiding.

**Codex CLI:** `codex exec` is the official non-interactive mode; `--json` switches stdout to JSON Lines for scripting ([learn.chatgpt.com non-interactive docs](https://learn.chatgpt.com)); OpenAI describes Codex Exec as "a lightweight, scriptable CLI mode for one-off tasks, CI runs, and automation" ([openai.com App Server post](https://openai.com)); `codex exec fork` added headless session branching in 0.148.0 ([release notes summary](https://www.getclaudeskills.com)). MCP is supported via config (`mcp_servers` in config.toml).

**Gemini CLI:** `gemini -p "..."` non-interactive with `--output-format json` / `stream-json` (schema includes token usage/latency); `--non-interactive` for CI; MCP configured via `settings.json` + `gemini mcp` subcommands ([configuration reference](https://geminicli.com/docs/reference/configuration), [google-gemini/gemini-cli](https://github.com/google-gemini/gemini-cli), [headless guide](https://inventivehq.com/knowledge-base/gemini/how-to-use-headless-mode)).

**Takeaway:** all three major agent CLIs have mature headless/JSON modes and MCP clients — a sidecar abstraction can support all of them.

## 3. MCP SSH-exec servers (existing "run command over SSH" tools)

**Verdict: mature options exist, but for LiteTerm you should ship your own MCP tool that bridges into LiteTerm's session pool (visibility + reuse + approval), rather than have the agent open its own SSH connections.**

- **[tufantunc/ssh-mcp](https://github.com/tufantunc/ssh-mcp)** — the most serious one. MIT, ~822 stars, actively maintained (326 commits, recent changelog, CI/codecov, security tooling). 15 tools: `read-command` (allowlisted read-only), `run-command` (destructive/privileged require approval), `privileged-command` (sudo via stdin), SFTP upload/download (in-context and streamed to a `transferRoot` dir), stateful `open-session`/`read-session-output`/`close-session`, `signal-process`. Security model: role × host-tier matrix (viewer/operator/admin × prod/staging/dev), regex denylist, optional OPA/Rego sidecar that can only narrow permissions, human-in-the-loop approval via **MCP elicitation**, host-key TOFU/pinning, audit logging with redaction, daily command quotas. Transports: stdio + HTTP (bearer token). Explicitly designed against the "lethal trifecta" (LLM + private data + network egress).
- Smaller/less proven entries on npm: [@davidfei/ssh-mcp](https://www.npmjs.com/package/@davidfei/ssh-mcp) (Oct 2025), [@uarlouski/ssh-mcp-server](https://www.npmjs.com/package/@uarlouski/ssh-mcp-server) (Feb 2026), [shaike1/mcp-server-ssh](https://www.mcpserverfinder.com/servers/shaike1/mcp-server-ssh) (exec + file upload/download), [mcp-server-ssh-manager](https://classic.yarnpkg.com/en/package/mcp-server-ssh-manager), and an SSH-agent-style server that auto-discovers hosts from `~/.ssh/config` ([aiondadotcom, listed on lobehub](https://lobehub.com)).
- Docker also ships an official MCP server toolkit including SSH execution ([docker/mcp-server](https://github.com/docker/mcp-server)) — useful precedent for vendor-grade MCP SSH tools.

**Why not just use ssh-mcp inside LiteTerm?** You can — it proves demand and design patterns (classification, approval, audit). But its tools open their own SSH connections from the MCP server process, invisible to LiteTerm's UI. LiteTerm's differentiator is routing agent commands through **its own sessions** so the user watches them execute (Termius-style), keeps one auth path, and logs centrally.

## 4. Termius and other AI-integrated terminals/SSH clients

- **Termius AI Agent** (Nov 2024): chat that "can execute commands across multiple terminals and handle the execution results efficiently", with explicit human-in-the-loop: *"The AI Agent will not execute any command on your hosts without your confirmation."* Capabilities: infra metadata (hosts/groups/tags), access to **active connections and their outputs (scrollback)**, multi-terminal execution, memory of context; "never has access to your passwords or keys" ([Termius blog](https://termius.com/blog/ai-agent)). This is exactly the local-agent-over-own-sessions architecture recommended here.
- Termius also ships AI command autocompletion and AI snippet generation, and an experimental DevOps agent "Gloria" for simple infra tasks ([termai.sh breakdown](https://termai.sh/blog/termius-ai-assistant), [Termius blog: Gloria](https://termius.com/blog/devops-ai-agent-gloria)).
- **Warp**: strongest counter-model. The Warp SSH extension installs a **small companion server** (`~/.warp*/remote-server`) on the remote host on first connect — runs as the user, communicates over the existing SSH connection, no root/ports, auto-updates, uninstalls by deleting the directory. It handles file I/O, git, and codebase indexing remotely and streams lightweight incremental updates back, giving remote hosts the full Warp experience including the coding agent. Requirements: Linux/macOS, x86_64/aarch64, glibc 2.31+, bash/zsh; older hosts fall back to a legacy wrapper ([docs.warp.dev/terminal/warpify/ssh](https://docs.warp.dev/terminal/warpify/ssh), [2026 changelog](https://docs.warp.dev/changelog/2026)).
- **Tabby**: has AI assistant capabilities; the community is actively discussing MCP integration letting agents control **existing** Tabby tabs/SSH sessions rather than isolated terminals ([github.com/Eugeny/tabby](https://github.com/eugeny/tabby), [issue #11276](https://github.com/Eugeny/tabby/issues/11276), [Tabby-MCP listing](https://mcpmarket.com/server/tabby)) — same direction as Option B.
- **WindTerm**: no AI features as of 2026 ([ctrlops.io review](https://ctrlops.io/blog/best-ssh-clients-windows), [kingToolbox/WindTerm](https://github.com/kingtoolbox/windterm)).

## 5. OpenHands (OpenDevin)

**Verdict: not a fit for LiteTerm. No first-class "connect to arbitrary existing server over SSH" runtime.**

- Runtime/sandbox options are selected via `RUNTIME` (`docker` default; `remote` points the agent at a hosted **agent-server endpoint** with `SANDBOX_REMOTE_RUNTIME_API_URL` + `SANDBOX_API_KEY`, i.e. OpenHands' own runtime service, not arbitrary SSH hosts) ([remote sandbox docs](https://docs.openhands.dev/openhands/usage/sandboxes/remote), [agent-server architecture](https://docs.openhands.dev/sdk/arch/agent-server)).
- Historical code had an SSH-based runtime implementation (`openhands/runtime/impl/ssh/`) for connecting to existing hosts, but current docs do not surface it as a supported option; community answers point at `RUNTIME=remote` + [issue #6405](https://github.com/OpenHands/OpenHands/issues/6405).
- Self-hosting docs describe running the OpenHands **server** on a VM reachable over SSH and tunneling to the UI — agent-on-server again, not agent-controlling-your-server ([SELF_HOSTING.md](https://github.com/OpenHands/OpenHands/blob/main/docs/SELF_HOSTING.md)).
- Repo: [OpenHands/OpenHands](https://github.com/OpenHands/OpenHands) (~92k stars, verified via GitHub API).

## 6. Pattern: agent-on-server inside tmux

**Verdict: best practice for *persistent, many-agent coding workflows* — but it is exactly the per-server-install model LiteTerm wants to avoid. Useful as an optional feature, not the core.**

- **[BloopAI/vibe-kanban](https://github.com/BloopAI/vibe-kanban)** (Apache-2.0, ~28k stars, active — verified via GitHub API): kanban GUI that runs 10+ coding agents (Claude Code, Codex, Gemini CLI, opencode…) in per-task workspaces, each with its own branch, terminal, and dev server; born from "running multiple agents in tmux" ([vibekanban.com](https://www.vibekanban.com), [VirtusLab writeup](https://virtuslab.com/blog/ai/vibe-kanban)).
- **agentmgr**: runs several AI agents in tmux sessions on a dedicated socket, "one keypress per action," ordinary tmux sessions so you can attach from anywhere ([r/tmux thread](https://www.reddit.com/r/tmux/comments/1vlj9lg/agentmanager_run_several_ai_coding_agents_in_tmux)).
- tmux remains the standard persistence layer for remote Claude Code: install on VPS, run in tmux, use `claude -p` for scheduled jobs ([lumadock guide](https://lumadock.com/tutorials/run-claude-code-on-vps)).
- Key insight for LiteTerm: this pattern's value is **persistence across client disconnects**. LiteTerm already gives users tmux-attach capability by being an SSH client; a future "agent sessions" feature could detect/label tmux sessions running agents. It does not solve per-server agent install.

## 7. Protocols for embedding agents in a GUI

- **Agent Client Protocol (ACP)** — [agentclientprotocol.com](https://agentclientprotocol.com/), repo [zed-industries/agent-client-protocol](https://github.com/zed-industries/agent-client-protocol) (Apache-2.0, ~4.5k stars, active — verified via GitHub API). "A protocol for connecting any editor to any agent"; agents run as client sub-processes speaking **JSON-RPC over stdio**; remote agents (HTTP/WebSocket) are "a work in progress"; reuses MCP JSON formats where possible and adds agentic-UX types (e.g., diffs) ([ACP docs](https://agentclientprotocol.com/)).
- Adoption beyond Zed is real: **JetBrains partnered with Zed (Oct 2025)** to bring ACP into JetBrains IDEs ([JetBrains blog](https://blog.jetbrains.com/ai/2025/10/jetbrains-zed-open-interoperability-for-ai-coding-agents-in-your-ide)); community implementations exist in several editors ([Zed ACP progress report](https://zed.dev/blog/acp-progress-report)); third parties describe adoption by JetBrains, Google, GitHub, and 25+ agents ([morphllm.com explainer](https://www.morphllm.com/agent-client-protocol)); a registry enables editor auto-discovery of agents; 2026 talks include "ACP Beyond the Editor" ([ACP publications](https://agentclientprotocol.com/publications)). Sublime Text community is requesting it ([forum thread](https://forum.sublimetext.com/t/acp-agent-client-protocol/77027)).
- **stream-json** (Claude Code, Gemini CLI) and **JSONL** (Codex) remain the de-facto embeddable formats for headless CLIs; opencode instead exposes **HTTP + SSE with an OpenAPI 3.1 spec** ([opencode server docs](https://opencode.ai/docs/server)).
- **Assessment for LiteTerm:** ACP is editor-shaped (files/diffs, cwd-local workspaces) and its remote transport is unfinished; it is not a natural fit for an SSH client where the agent's tools are remote terminals. The HTTP/SSE + MCP-tool approach (opencode) or stream-json sidecar (Claude Code) is lower-risk today. Watch ACP for its HTTP transport maturing.

## 8. Terminal-side practicalities

- **SSH exec channel vs PTY for agent tooling:** for programmatic command execution, open a session channel and run `channel.exec(true, cmd)` **without** a PTY — you get separate stdout/stderr (`Data` vs `ExtendedData`) and a reliable `ChannelMsg::ExitStatus`. A PTY merges streams and injects terminal control sequences (i.e., scraping). russh's canonical example demonstrates exactly this flow ([russh/examples/client_exec_interactive.rs](https://github.com/warp-tech/russh/blob/main/russh/examples/client_exec_interactive.rs), [ChannelMsg docs](https://docs.rs/russh/latest/russh/enum.ChannelMsg.html)). **Design rule for LiteTerm:** agent tool calls = exec channels on the pooled connection; PTY injection (writer) only when the product goal is "show the command running in the user's interactive shell." A hybrid is possible: run via exec for structured results and `echo` the command into the visible PTY for transparency.
- **OSC 133 shell integration:** the FinalTerm-derived marks — `A` prompt start, `B` prompt end (input begins), `C` command start/output begins, `D;exit_code` command finished with exit code — are the standard way to detect command completion in an interactive PTY stream ([Contour spec](https://contour-terminal.org/vt-extensions/osc-133-shell-integration)). Implemented across the ecosystem: kitty auto-marks ([kitty docs](https://sw.kovidgoyal.net/kitty/shell-integration)), WezTerm/iTerm2 (originator), Zellij selection ([docs](https://zellij.dev/documentation/shell-integration.html)), Windows Terminal ships tutorials for PowerShell/cmd/bash ([learn.microsoft.com](https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration)); tmux forwards the sequences in recent versions ([tmux #5237](https://github.com/tmux/tmux/issues/5237)). `133;D` is the only mark that fires on actual completion and carries the exit code — LiteTerm's PTY stream parser can use it (when the remote shell has integration) as a completion/exit-status signal for agent-written commands, falling back to timeout/idle heuristics otherwise.

---

## Option comparison for LiteTerm

| Dimension | A. Own agent loop in Rust | **B. Local headless sidecar + MCP SSH tool (recommended)** | C. Agent-on-server (tmux), LiteTerm-managed | (Reference) Warp-style remote companion |
|---|---|---|---|---|
| Server-side footprint | None | **None** | Full agent runtime per server (Node or binary, disk, secrets on server) | Small companion binary per host |
| Setup friction per server | None | **None (agent lives on desktop)** | High: install, auth, versions, updates per host | Medium (auto-install, consent-gated) |
| Agent maturity | You own the brain: tool loop, context mgmt, multi-model quirks, error recovery — permanent R&D tax | **Mature brains** (opencode MIT 75+ providers; claude/codex/gemini); JSON/SSE streaming; you own only the SSH tools | Same maturity as installed agent | Warp-grade (not available to us) |
| Approval gates / safety | Fully yours to build | **Strong**: opencode permission API; Claude `--permission-prompt-tool` routes approval into LiteTerm UI; ssh-mcp-style classification/audit as design reference | Hard: agent runs with user privileges on server; approvals live in the agent's own TUI, not LiteTerm | In-client |
| UI integration effort | High (chat, diffs, tools, streaming all from scratch) | **Medium**: render SSE/stream-json events; sidecar lifecycle mgmt; MCP bridge binary | Medium: tmux session detection + attach; agent I/O is opaque scrollback | High |
| Visibility of actions in terminal | Depends on implementation | **Full** if tools route through the session pool (Termius-style; user watches commands) | Poor-to-medium (attach to tmux pane) | Full |
| Persistence across disconnects | Only if you build job queues | Not inherent (sidecar dies with app) — mitigate with `opencode serve` + resumable sessions | **Best** (tmux survives) | Companion persists |
| Dependency risk | None external | External CLI (mitigate: bundle/auto-download opencode, single binary, MIT) | Server constraints (Node/glibc), drift | n/a |
| Precedents | IntelliSSH-style minimal ([reddit](https://www.reddit.com/r/selfhosted/comments/1l0yq85/intellissh_release_today_webbased_ssh_client_with)) | **Termius AI Agent** ([blog](https://termius.com/blog/ai-agent)); Tabby direction ([issue #11276](https://github.com/Eugeny/tabby/issues/11276)) | vibe-kanban ([repo](https://github.com/BloopAI/vibe-kanban)); agentmgr ([r/tmux](https://www.reddit.com/r/tmux/comments/1vlj9lg/agentmanager_run_several_ai_coding_agents_in_tmux)) | Warp SSH extension ([docs](https://docs.warp.dev/terminal/warpify/ssh)) |

**Recommendation detail:** B primary. Concretely:

- Ship a tiny **MCP stdio server inside LiteTerm** (`liteterm-ssh-mcp`) whose tools are `ssh_exec` (routes to a pooled session's fresh **exec channel**, returns stdout/stderr/exit_status), `ssh_read_scrollback`, `ssh_upload`/`ssh_download` (SFTP via russh), `list_sessions`. The shim connects back to LiteTerm over a localhost socket so all execution is visible in the UI and centrally logged. (Design reference: [tufantunc/ssh-mcp](https://github.com/tufantunc/ssh-mcp) tool taxonomy and safety model.)
- Spawn **opencode** as the default brain in `serve` mode on 127.0.0.1 with `OPENCODE_SERVER_PASSWORD` set, register the MCP shim in `opencode.json` (`type: local`), and drive conversations via the OpenAPI/SSE API, including **permission requests rendered as LiteTerm approval dialogs** ([opencode server docs](https://opencode.ai/docs/server), [MCP docs](https://opencode.ai/docs/mcp-servers/)). Adapters for `claude -p --output-format stream-json` and `codex exec --json` can follow the same MCP shim with near-zero extra work since all three are MCP clients.
- Keep Option A as a thin "quick assist" (single-shot command generation from natural language, no agent loop) — this matches Termius's AI autocomplete/snippet tier without sidecar dependency.
- Defer Option C; if demand appears, add "detect & label tmux agent sessions" + one-click bootstrap of a user-chosen agent, not a LiteTerm-managed install.

---

## Suggested MVP plan

**Phase 0 — foundations (Rust, no LLM yet)**
1. Add an `exec_channel` API to the session pool: run a command on a pooled connection without PTY; capture Data/ExtendedData/ExitStatus with a timeout; enforce a per-command byte cap. Basis: [russh exec example](https://github.com/warp-tech/russh/blob/main/russh/examples/client_exec_interactive.rs).
2. Add SFTP helpers (upload/download) on pooled connections.
3. Parse OSC 133 (esp. `133;D;exit_code`) in the PTY stdout stream; store completion events per session. Fallback idle-timeout heuristic. Spec: [Contour OSC 133](https://contour-terminal.org/vt-extensions/osc-133-shell-integration).

**Phase 1 — MCP bridge**
4. Implement `liteterm-ssh-mcp`: stdio MCP server exposing `ssh_exec`, `ssh_read_scrollback`, `sftp_upload/download`, `list_hosts`, `list_sessions`. It authenticates to LiteTerm via a loopback token. Add command classification (read-only vs mutating) and route mutating calls through an approval callback (patterns from [ssh-mcp](https://github.com/tufantunc/ssh-mcp), including MCP elicitation for approvals).
5. Manual test: configure opencode (`mcp: { liteterm: { type: "local", command: [...] } }`) per [opencode MCP docs](https://opencode.ai/docs/mcp-servers/) and drive a remote task end-to-end.

**Phase 2 — sidecar orchestration + UI**
6. Sidecar manager in Tauri: locate/download opencode binary, run `opencode serve --hostname 127.0.0.1` with basic auth, health-check, clean shutdown ([opencode server docs](https://opencode.ai/docs/server)).
7. Vue agent panel: conversation view fed by SSE events; tool-call cards that link to the terminal tab where the command ran; **approval dialog** wired to opencode permission events (pattern: Claude's `--permission-prompt-tool`, opencode's permissions API).
8. Session resumability: map LiteTerm agent conversations to opencode session IDs (`--session`, `/session` endpoints).

**Phase 3 — breadth (post-MVP)**
9. Adapter for `claude -p` (stream-json) and `codex exec --json` as alternative brains behind the same UI; provider settings per brain.
10. Optional "persistent agents" tab: detect tmux sessions on hosts, label agent ones; optional guided bootstrap (user-chosen agent) — explicitly out of MVP scope.
11. Revisit ACP if its remote (HTTP) transport matures, as an alternative embedding protocol ([ACP](https://agentclientprotocol.com/)).

**Success criteria for the MVP:** user points LiteTerm at any existing server, types "check why nginx is down and fix the config" in the agent panel, sees each proposed command as an approval card, watches approved commands execute in the terminal tab (exec-channel results + visible PTY echo), gets a fix summary — with **nothing installed on the server** beyond what SSH already needs.

---

## Source index (primary sources first)

- opencode: [docs/server](https://opencode.ai/docs/server) · [docs/cli](https://opencode.ai/docs/cli) · [docs/mcp-servers](https://opencode.ai/docs/mcp-servers/) · [docs/config](https://opencode.ai/docs/config) · [repo (MIT)](https://github.com/anomalyco/opencode)
- Claude Code: [CLI reference](https://code.claude.com/docs/en/cli-reference) · [Agent SDK TS](https://code.claude.com/docs/en/agent-sdk/typescript) · [agent-sdk-python](https://github.com/anthropics/claude-agent-sdk-python)
- Codex CLI: [non-interactive docs](https://learn.chatgpt.com) · [openai.com App Server](https://openai.com)
- Gemini CLI: [config reference](https://geminicli.com/docs/reference/configuration) · [repo](https://github.com/google-gemini/gemini-cli)
- MCP SSH servers: [tufantunc/ssh-mcp](https://github.com/tufantunc/ssh-mcp) · [docker/mcp-server](https://github.com/docker/mcp-server)
- Termius & clients: [Termius AI Agent](https://termius.com/blog/ai-agent) · [Gloria](https://termius.com/blog/devops-ai-agent-gloria) · [Warp SSH extension](https://docs.warp.dev/terminal/warpify/ssh) · [Warp 2026 changelog](https://docs.warp.dev/changelog/2026) · [Tabby #11276](https://github.com/Eugeny/tabby/issues/11276)
- OpenHands: [remote sandbox docs](https://docs.openhands.dev/openhands/usage/sandboxes/remote) · [agent-server](https://docs.openhands.dev/sdk/arch/agent-server) · [SELF_HOSTING.md](https://github.com/OpenHands/OpenHands/blob/main/docs/SELF_HOSTING.md) · [issue #6405](https://github.com/OpenHands/OpenHands/issues/6405)
- tmux agent managers: [vibe-kanban](https://github.com/BloopAI/vibe-kanban) · [agentmgr thread](https://www.reddit.com/r/tmux/comments/1vlj9lg/agentmanager_run_several_ai_coding_agents_in_tmux)
- Protocols: [ACP](https://agentclientprotocol.com/) · [ACP repo](https://github.com/zed-industries/agent-client-protocol) · [JetBrains×Zed](https://blog.jetbrains.com/ai/2025/10/jetbrains-zed-open-interoperability-for-ai-coding-agents-in-your-ide) · [Zed ACP report](https://zed.dev/blog/acp-progress-report)
- Terminal practicalities: [russh exec example](https://github.com/warp-tech/russh/blob/main/russh/examples/client_exec_interactive.rs) · [ChannelMsg](https://docs.rs/russh/latest/russh/enum.ChannelMsg.html) · [OSC 133 spec](https://contour-terminal.org/vt-extensions/osc-133-shell-integration) · [Windows Terminal shell integration](https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration) · [tmux #5237](https://github.com/tmux/tmux/issues/5237)
