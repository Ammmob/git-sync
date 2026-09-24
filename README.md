<div align="center">

# Git Sync

A lightweight desktop app that keeps local repositories in sync with Overleaf and GitHub.

![Platform Windows](https://img.shields.io/badge/Platform-Windows%20x64-0078D4.svg) ![Tauri 2](https://img.shields.io/badge/Tauri-2-24C8D8.svg) ![React TypeScript](https://img.shields.io/badge/UI-React%20%2B%20TypeScript-3178C6.svg) ![Version 0.1.0](https://img.shields.io/badge/version-0.1.0-4C8BF5.svg)

**English** · [简体中文](README.zh-CN.md)

</div>

Git Sync manages two-way Git synchronization from one desktop window. Choose a local directory,
select its platform and credentials, and set a synchronization
interval. Each project can run automatically or be synchronized on demand.

## ✨ Highlights

- Manage Overleaf and GitHub projects together, with platform filters and search.
- Fetch remote changes, commit local edits, merge compatible histories, and push updates.
- Set a global interval or override it for individual projects.
- Select projects to start, pause, or delete in bulk, or pause/resume all projects.
- Open repository folders and inspect recent sync records.
- Store multiple named Tokens per platform and display only masked values.
- Choose a platform default, a specific Token, or existing system Git credentials.
- Stop automatic integration when conflicts or unfinished Git operations are detected.
- Sort the project list by any column.
- Run as a Windows desktop application without installing Python or Node.js.

## 📋 Roadmap

- [x] Two-way sync engine (fetch, commit, merge, push)
- [x] Overleaf and GitHub platforms
- [x] Token management
- [x] Bulk management (pause, resume, delete, pause/resume all)
- [x] Sync duration (days, months, or permanent with expiry)
- [x] Sortable project columns
- [x] Theme settings (system / light / dark)
- [ ] Hugging Face platform (planned)
- [ ] Overleaf empty-directory initialization (planned)

## 🖼️ Preview

![Project list](docs/images/projects.png)

![Settings and Token management](docs/images/settings.png)

The screenshots use demonstration projects and masked sample credentials.

## 🚀 Quick Start

### Run the desktop app

Requirements:

- Windows 10/11 x64.
- [Git for Windows](https://git-scm.com/downloads/win), available on `PATH`.
- [Microsoft Edge WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/), normally available on current Windows installations.
- Git access to the remote repository. Overleaf currently requires an existing local clone.

Run `Git Sync_0.1.0_x64-setup.exe` if you have a built installation package. For a
local development checkout that has already been built with `scripts/build.ps1`,
`Start.cmd` opens the compiled application.

### Add a project

1. Open **Global settings → Access Token** to add credentials, or use system Git credentials.
2. Select **New sync**, then choose **Overleaf** or **GitHub**.
3. Choose a local directory or enter a new absolute path. For existing repositories, the app reads the remote URL and branch.
4. Set the project name, Token, and optional interval override.
5. Choose whether to enable automatic synchronization, then save. A nonempty ordinary directory requires confirmation; cancel returns to the form without initializing it.

The interface currently uses Simplified Chinese. GitHub projects support existing clones,
empty directories, nonempty ordinary directories, and paths that do not exist yet.
New directories are created on save. Remote files are downloaded while existing local
files are retained; same-name files keep the local version as changes against the remote
history. Enabled synchronization will commit and upload these changes. An empty remote
repository receives its first commit once local files are available. File/directory type
conflicts stop initialization without replacing local files.

Overleaf still requires an existing local clone. Existing Git repositories must match the
configured remote and branch. Configure Git's author name and email before automatic commits.

### Bulk management

Use the leftmost checkboxes to select projects; the header checkbox selects the current filtered list. Bulk controls apply to selected projects. Pause all / Resume all apply to every project, including those hidden by filters. Changing the filter or search clears the selection.

Pausing prevents subsequent automatic cycles while allowing a running cycle to finish. Resumed projects queue with at most four concurrent tasks. Deletion requires confirmation and only removes configuration; running projects are skipped and reported.

### Sync duration

New projects default to one calendar month. Choose 1–30 days, 1–12 months, or permanent synchronization. The list shows the expiry date; hover for the exact time. Projects created by earlier versions remain permanent.

Expired projects pause automatically, retaining configuration and local/remote files. A running cycle is allowed to finish. Projects that expire while the app is closed pause on the next launch. Manual sync and Resume all cannot bypass expiry.

Editing defaults to keeping the current deadline. Choose Reset duration to recalculate from this save, without adding time to the old deadline, or choose permanent synchronization. Changing only its name, Token, or other unrelated settings does not extend it. Days are 24-hour periods. Months use the local UTC offset at save time, clamping to the last day of the destination month when necessary. Permanent projects have no expiry.

## 🧭 Synchronization

Each cycle checks the repository, fetches the configured remote branch, waits for local
files to stop changing, commits local changes, merges remote history, and pushes if needed.
Changes on both sides can be combined automatically when Git can merge them cleanly.

| Behavior | Details |
| --- | --- |
| Default interval | 10 seconds; project overrides are optional. |
| File-save delay | 2 seconds by default; changing files defer synchronization. |
| Local changes | Includes additions, edits, deletions, and already-staged changes. |
| Ignore rules | Untracked ignored files are excluded. Already-tracked files matching ignore rules require review. |
| Conflicts | Preserves the conflict state and pauses automatic integration. Resolve or abort the Git operation manually, then resume. |
| Network or authentication failure | Shows the error and retries for enabled tasks. |
| Remove a project | Removes the task configuration; keeps local files and the remote repository. |
| Window behavior | Minimizing keeps synchronization running. Exiting stops it; active Git operations must finish first. |

The app does not force-push, automatically reset repositories, compile LaTeX, or build
websites. Use one automatic synchronizer per repository to avoid competing Git operations.

## ⚙️ Settings and Data

Each platform has its own Token list and optional default. A project can inherit the
default, use a specific Token, or use system Git credentials. Tokens explicitly referenced
by a project must be replaced in that project before deletion.

Application data is stored outside the source checkout:

```text
%LOCALAPPDATA%/io.gitsync.desktop/
├─ config.json     Settings, tasks, Token metadata, and task status
├─ credentials/   Windows user-protected Token files
├─ helpers/       Git credential helper scripts
└─ logs/          Per-project sync records
```

Moving the source checkout does not relocate or delete this data. Removing a Token from
the app removes its local copy; it does not revoke the Token on the remote platform.

## 🛠️ Development

Install Node.js 22, Rust through rustup, and the Microsoft C++ Build Tools required by
[Tauri on Windows](https://v2.tauri.app/start/prerequisites/). The toolchain version is
specified in [rust-toolchain.toml](rust-toolchain.toml).

From the project root:

```powershell
npm.cmd ci
powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
```

Build a local executable or a Windows installer:

```powershell
# Executable, also copied to .local/bin/ for Start.cmd
powershell -ExecutionPolicy Bypass -File scripts/build.ps1

# Executable and NSIS installer
powershell -ExecutionPolicy Bypass -File scripts/build.ps1 -Installer
```

The installer is written to `src-tauri/target/release/bundle/nsis/`.

Run checks:

```powershell
npm.cmd run build
npm.cmd run test:engine
npx.cmd playwright install chromium
npm.cmd run test:ui
```

Engine tests use temporary local repositories. UI tests use demonstration data and cover
project and Token management, sorting, theming, table alignment, and scaling. The Windows GitHub Actions
workflow runs checks and uploads an installer artifact; it does not publish a release.

## 📁 Project Structure

```text
git-sync/
├─ src/                 React interface, components, and styles
├─ src-tauri/           Rust desktop app and synchronization engine
├─ public/              Platform icons
├─ tests/               Browser-based interface tests
├─ scripts/             Development, build, and desktop smoke-test helpers
├─ docs/images/         README screenshots
├─ .github/workflows/   Windows checks and installer builds
├─ README.md
└─ README.zh-CN.md
```

Build outputs, local executables, and test reports are ignored by Git. Platform mark
attribution is recorded in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
