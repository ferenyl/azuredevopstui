# azuredevopstui

A terminal UI for keeping track of your work in Azure DevOps. It shows your pull requests, your work items, other people's pull requests and the items that are ready to be picked up in the current sprint, all in one keyboard-driven screen.

Built with [ratatui](https://ratatui.rs). Catppuccin Mocha is the default theme.

```
┌ My PRs (3) · newest ┐┌ !1234 Add order filter ──────────────────────────────────┐
│!1234 Add order filt ││  Overview │ Comments (2) │ Checks (5)                    │
│!1230 Fix login      ││ ──────────────────────────────────────────────────────── │
└─────────────────────┘│ Status       ● active                                    │
┌ My work items (4) · ┐│ Repo         myapp-api                                   │
│#5678 Order list  P1 ││ Branch       feature/1234 → main                         │
└─────────────────────┘│ Merge        succeeded                                   │
┌ Others' PRs (12) ·  ┐│                                                          │
│!1240 Bump packages  ││ ▍Reviewers ───────────────────────────────────────────── │
└─────────────────────┘│   ✔ Anna Andersson  approved  required                   │
┌ Ready – Sprint 41 ( ┐│   ○ Bo Bengtsson    no vote                              │
│#5700 Export to CSV  ││                                                          │
└─────────────────────┘└──────────────────────────────────────────────────────────┘
 [j/k/↑/↓] move/scroll [ctrl+hjkl] change box/column [tab] next tab …   updated 8s ago
```

## Features

- **Four lists** on the left:
  - **My PRs**: active pull requests you created, across all repositories in the project.
  - **My work items**: open items assigned to you in the current sprint, limited to the chosen work item types.
  - **Others' PRs**: active pull requests created by someone else. You can filter them by reviewer or creator.
  - **Ready**: items in the current sprint that sit in your *ready* board column and are not assigned to you.
- **Detail panel** with tabs:
  - **Pull request**:
    - **Overview** shows status, branches, merge status, reviewers and their votes, and the description.
    - **Comments** shows the comment threads with their status and file path.
    - **Checks** shows the branch policies and statuses, with a summary of passed, failed and pending.
  - **Work item**:
    - **Overview** shows type, state, board column, priority, assignee, sprint, tags, description, repro steps and acceptance criteria.
    - **Children** shows child items grouped by type (Task, Release Task, User Story, …), each with state and assignee.
    - **Comments** shows the 10 newest comments.
- **Actions**:
  - Open a PR or work item in the browser.
  - Move a work item to another board column. The columns come from your team's board, and split columns (for example *Active → Doing / Done*) are supported.
  - Assign a work item to yourself.
- **Sorting** per list type. The choice is saved to the config.
- **Work item type filter**: pick which types (User Story, Bug, Feature, …) the work item lists show. The list of types comes from the project, and the choice is saved to the config.
- **Auto refresh** at a configurable interval. The open detail view refreshes too.
- **Setup wizard** on first start. Organization, project, team and the ready column are picked from lists fetched from Azure DevOps.
- **Authentication** uses the Azure CLI first and falls back to a Personal Access Token stored in the OS keyring.

## Requirements

- **Rust**: a toolchain with edition 2024 support (Rust 1.88 or newer).
- **Azure CLI** (recommended): you must be logged in with `az login`.
- **For PAT authentication**: an OS keyring.
  - **Linux**: a Secret Service compatible keyring, such as GNOME Keyring, KeePassXC or KWallet.
  - **WSL**: one running inside the distro, for example `gnome-keyring-daemon`. Otherwise the PAT cannot be stored.
  - **Windows**: Windows Credential Manager, which is built in.
- **Terminal**: one with true color support. A terminal that supports the kitty keyboard protocol gives you `ctrl+h`/`ctrl+j`. In other terminals, use `ctrl+arrow` instead.

## Install

```sh
git clone <repo-url> azuredevopstui
cd azuredevopstui
cargo install --path .
```

Or build without installing:

```sh
cargo build --release
./target/release/azuredevopstui
```

## Windows

The app runs natively on Windows. Use Windows Terminal; the old console host lacks some of the symbols.

- **Build on Windows**: install Rust with the MSVC toolchain and the Visual Studio Build Tools (C++ workload), then run `cargo install --path .` in PowerShell. Clone the repo on the Windows file system, not under `\\wsl$`.
- **Cross-compile from WSL or Linux**: install the MinGW compiler (`mingw-w64-gcc` on Arch), then run:
  ```sh
  rustup target add x86_64-pc-windows-gnu
  cargo build --release --target x86_64-pc-windows-gnu
  ```
  The binary ends up in `target/x86_64-pc-windows-gnu/release/azuredevopstui.exe`.
- **Azure CLI**: use the Windows version of Azure CLI. It has its own login, separate from WSL, so run `az login` in PowerShell.
- **Run it from Windows**: start the `.exe` from PowerShell or cmd in Windows Terminal, not from a WSL shell.
- **Paths**:
  - Config: `%APPDATA%\azuredevopstui\config.json`
  - Log: `%LOCALAPPDATA%\azuredevopstui\log`
  - PAT: Credential Manager → *Windows Credentials*, as the entry `azuredevopstui`.
- **Keys**: `ctrl+h/j/k/l` work directly in Windows Terminal. No kitty keyboard protocol is needed.

## First start

When there is no config file, the app runs a setup:

1. **Sign in**: uses the Azure CLI token, or asks for a PAT if the CLI is not available.
2. **Organization**: picked from the organizations your account belongs to. If the list cannot be fetched, you type the name instead.
3. **Project** and then **team**: picked from lists.
4. **Ready column**: picked from your team's board columns.

The config is then written to disk, with the default colors filled in. If `ready_column` is ever removed from the config, the app asks for it again on the next start.

## Authentication

The method is set by `auth.method` in the config.

| Method | Behaviour |
|---|---|
| `auto` (default) | Use the Azure CLI. If that fails, use the PAT from the keyring. If there is none, ask for one. |
| `azcli` | Only use the Azure CLI. |
| `pat` | Only use the PAT from the keyring. |

### Azure CLI

The app runs `az account get-access-token --resource 499b84ac-1321-427f-aa17-267ca6975798` and renews the token when it expires.

If your tenant has no Azure subscriptions, log in with:

```sh
az login --allow-no-subscriptions --tenant <tenant-id>
```

### Personal Access Token

Create a PAT in Azure DevOps (*User settings → Personal access tokens*) with these scopes:

- **Code**: Read
- **Work Items**: Read & Write
- **Project and Team**: Read
- **Identity**: Read, needed only if you use `other_prs_filter`

The token is stored in the OS keyring under the service `azuredevopstui`. If Azure DevOps rejects it, the app removes it and asks for a new one. Press `t` to replace it at any time.

## Configuration

The config file is `$XDG_CONFIG_HOME/azuredevopstui/config.json`, which is usually `~/.config/azuredevopstui/config.json`. On Windows it is `%APPDATA%\azuredevopstui\config.json`.

```json
{
  "organization": "contoso",
  "project": "MyProject",
  "team": "My Team",
  "refresh_interval_secs": 120,
  "browser_command": null,
  "auth": { "method": "auto" },
  "other_prs_filter": {
    "reviewers": ["[MyProject]\\Developers", "anna.andersson@example.com"],
    "creators": []
  },
  "ready_column": "Ready",
  "work_item_types": [],
  "sort": {
    "pull_requests": "newest",
    "work_items": "priority"
  },
  "colors": { "background": "#1E1E2E", "...": "..." }
}
```

| Key | Default | Description |
|---|---|---|
| `organization` | set by setup | Azure DevOps organization (`dev.azure.com/<organization>`). |
| `project` | set by setup | Project name. |
| `team` | set by setup | Team. Used for the current sprint and the board. |
| `refresh_interval_secs` | `120` | Auto refresh interval in seconds. Set it to `0` to turn auto refresh off. Auto refresh pauses while a popup is open. |
| `browser_command` | `null` | Command used to open URLs, with the URL appended as the last argument, for example `"wslview"` or `"firefox --new-tab"`. The command is split on spaces, so it must be on `PATH` (use `"chrome"`, not `"C:\\Program Files\\…"`). When `null`, the system default is used. |
| `auth.method` | `"auto"` | `auto`, `azcli` or `pat`. See [Authentication](#authentication). |
| `other_prs_filter.reviewers` | `[]` | Show PRs where any of these users or groups is a reviewer. |
| `other_prs_filter.creators` | `[]` | Show PRs created by any of these users or groups. |
| `ready_column` | set by setup | The board column treated as *ready*. |
| `work_item_types` | `[]` | Work item types shown in *My work items* and *Ready*, for example `["User Story", "Bug"]`. An empty array shows all types. |
| `sort.pull_requests` | `"newest"` | Sort order for both PR lists. |
| `sort.work_items` | `"priority"` | Sort order for both work item lists. |
| `colors` | Catppuccin Mocha | Theme colors. See [Colors](#colors). |

### Others' PRs filter

When both arrays are empty, the list shows every active PR in the project, across all repositories, except your own.

Entries can be users, given as email or display name, or groups, given as `[Project]\Group` (escape the backslash in JSON: `"[MyProject]\\Developers"`). Each entry is resolved through the identities API. A PR is shown if it matches any entry, and duplicates are removed.

### Sorting

Press `S` on a list to choose the order. PR lists and work item lists each have their own setting, which is shared by the two lists of that kind and saved to the config straight away.

| `sort.pull_requests` | Order |
|---|---|
| `newest` | Most recently created first |
| `oldest` | Oldest first |
| `title` | Title A–Z |
| `repository` | Repository A–Z, then newest |

| `sort.work_items` | Order |
|---|---|
| `priority` | Priority 1–4, with items that have no priority last; ties go to the most recently changed |
| `changed` | Most recently changed first |
| `created` | Most recently created first |
| `state` | State A–Z, then most recently changed |
| `id` | ID ascending |

### Work item types

Press `f` to open a list of all work item types used in the project. Hidden types such as test plans and code reviews are left out. Use `space` to tick or untick a type, and `enter` to save the choice to `work_item_types` and reload the lists. The filter applies to both work item lists. All types are shown by default, and ticking every type (or none) also shows all of them.

### Colors

Each value is a hex color, such as `"#89B4FA"`. A key that is missing falls back to its Mocha default.

| Key | Default | Used for |
|---|---|---|
| `background` | `#1E1E2E` | App background |
| `foreground` | `#CDD6F4` | Text |
| `border` | `#45475A` | Borders, rules, comment bars |
| `border_focused` | `#89B4FA` | Focused box, active tab |
| `selection_bg` | `#313244` | Selected row, tag background |
| `selection_fg` | `#F5E0DC` | Selected row text (focused box), tag text |
| `title` | `#CBA6F7` | Box titles, section headings |
| `toolbar_bg` | `#181825` | Toolbar background |
| `toolbar_key` | `#F9E2AF` | Key hints |
| `pr_approved` | `#A6E3A1` | Approved votes, completed PRs, done states |
| `pr_waiting` | `#F9E2AF` | Waiting votes, drafts, active threads |
| `pr_rejected` | `#F38BA8` | Rejected votes |
| `build_succeeded` | `#A6E3A1` | Passed checks |
| `build_failed` | `#F38BA8` | Failed checks |
| `build_running` | `#89DCEB` | Pending checks, active PRs, active states |
| `workitem_bug` | `#F38BA8` | Bugs |
| `workitem_story` | `#89B4FA` | Stories and other types |
| `workitem_task` | `#F9E2AF` | Tasks |
| `comment_author` | `#FAB387` | Comment authors |
| `muted` | `#6C7086` | Labels and secondary text |
| `error` | `#F38BA8` | Errors, merge conflicts |

## Keys

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Move in a list, or scroll the detail panel |
| `ctrl+j` / `ctrl+k`, `ctrl+↓` / `ctrl+↑` | Previous or next box in the left column |
| `ctrl+l` / `ctrl+h`, `ctrl+→` / `ctrl+←` | Switch between the left column and the detail panel |
| `enter` | Open the selected item in the detail panel |
| `tab` / `shift+tab` | Next or previous detail tab |
| `S` | Choose the sort order for the focused list |
| `f` | Choose which work item types are shown |
| `space` | Tick or untick an item in the type list |
| `s` | Move a work item to another board column |
| `a` | Assign a work item to yourself |
| `o` | Open the PR or work item in the browser |
| `r` | Reload everything |
| `t` | Enter a new PAT |
| `?` | Show all keys |
| `esc` | Close a popup or cancel |
| `q`, `ctrl+c` | Quit |

Actions apply to the selected row in the focused list. When the detail panel has focus, they apply to the item shown there. The toolbar always shows the keys that work in the current context.

## What the lists contain

- **My PRs**: `status=active` with you as the creator, across all repositories in the project.
- **Others' PRs**: `status=active`, filtered by `other_prs_filter` and with your own PRs excluded.
- **My work items**: `AssignedTo = @Me`, a state other than *Closed* or *Removed*, a type in `work_item_types` (all types when it is empty), and the team's current sprint.
- **Ready**: `BoardColumn = <ready_column>`, `AssignedTo <> @Me`, a type in `work_item_types` (all types when it is empty), and the team's current sprint.

Moving a work item sets the board column field, the column's *Done* field (for split columns) and the matching `System.State`.

## Logging

Logs are written to `$XDG_STATE_HOME/azuredevopstui/log`, which is usually `~/.local/state/azuredevopstui/log`. On Windows the log is `%LOCALAPPDATA%\azuredevopstui\log`. The default level is `info`. Set it with `RUST_LOG`:

```sh
RUST_LOG=azuredevopstui=debug azuredevopstui
```

At `debug` level every API request is logged.

## Troubleshooting

| Problem | Fix |
|---|---|
| The Azure CLI token fails with *refresh token has expired* | Run `az login` again. Add `--allow-no-subscriptions` if the tenant has no subscriptions. |
| *unauthorized* (401 or 203) with a PAT | The PAT has expired or lacks a scope. The app asks for a new one; you can also press `t`. |
| The PAT cannot be saved | No Secret Service keyring is running. Start one (on WSL, `gnome-keyring-daemon`), or use the Azure CLI. On Windows, Credential Manager is always available. |
| *failed to run az* | Azure CLI is not installed or not on `PATH`. On Windows the app runs `az.cmd`. |
| `$env:RUST_LOG` on Windows | In PowerShell, set it with `$env:RUST_LOG="azuredevopstui=debug"` before you start the app. |
| *team has no current sprint* | Set the current iteration for the team in *Project settings → Team configuration → Iterations*. |
| *X items are not on the board* | The work item type has no column on the team's board, so it cannot be moved. |
| `ctrl+h` / `ctrl+j` do nothing | Your terminal does not support the kitty keyboard protocol. Use `ctrl+←` / `ctrl+↓`. |
| Wrong organization, project or team | Edit the config file, or delete it to run the setup again. |

## Project structure

```
src/
  main.rs              terminal setup, logging, start
  config.rs            config file, sort options
  theme.rs             colors (Catppuccin Mocha defaults)
  auth/                Azure CLI token and keyring PAT
  api/                 Azure DevOps REST client and models
    pull_requests.rs   PR lists, threads, statuses, policies
    work_items.rs      sprint queries (WIQL), work item details
    boards.rs          board columns, move and assign
    projects.rs        organizations, projects, teams, current user
  app/                 state, actions, key mapping, event loop
    setup.rs           first-run wizard steps
  ui/                  rendering
    pr_detail.rs       PR tabs
    workitem_detail.rs work item tabs
    popup.rs           help, column, sort and type pickers
    toolbar.rs         key hints and status
```
