<p align="center">
  <img src="assets/kugutsu.png" alt="Kugutsu" width="480">
</p>

# Kugutsu(傀儡)

## What Kugutsu?

Kugutsu is an ADE (Agentic Development Environment) written in Rust with [gpui](https://www.gpui.rs/).

It is a terminal workspace for running coding agents side by side:

- **Workspaces, tabs and panes** — group terminals into workspaces, open multiple tabs, and split panes vertically or horizontally, all driven by tmux-style prefix keys.
- **Agent sidebar** — detects coding agents running in each terminal (Claude Code, Codex) and shows their state at a glance:
  - `working` — the agent is processing
  - `blocked` — the agent is waiting for your answer (e.g. a permission prompt)
  - `done` — the agent finished while you were looking elsewhere
  - `idle` — the agent is waiting for input
- **IME support** — compose and commit text with an input method inside the terminal.

## Quick Start

### Run with Nix

```sh
nix run github:myuron/kugutsu
```

### Build from source

The Rust toolchain and the libraries gpui depends on (Wayland, Vulkan, …) are provided by the Nix devShell.

```sh
git clone https://github.com/myuron/kugutsu.git
cd kugutsu
nix develop --command cargo run --release
```

If you use [direnv](https://direnv.net/), run `direnv allow` once and you can drop the `nix develop --command` prefix.

## Default Keybind

Kugutsu uses a tmux-style prefix key. Press the prefix `Ctrl-b`, then press one of the keys below. While the prefix is pressed, the keys that can follow it are listed above the status line.

| Key               | Action                    |
| ----------------- | ------------------------- |
| `Ctrl-b` `c`      | New tab                   |
| `Ctrl-b` `n`      | Next tab                  |
| `Ctrl-b` `p`      | Previous tab              |
| `Ctrl-b` `1`-`9`  | Select tab N              |
| `Ctrl-b` `v`      | Split pane left / right   |
| `Ctrl-b` `s`      | Split pane top / bottom   |
| `Ctrl-b` `h`      | Focus left pane           |
| `Ctrl-b` `j`      | Focus pane below          |
| `Ctrl-b` `k`      | Focus pane above          |
| `Ctrl-b` `l`      | Focus right pane          |
| `Ctrl-b` `x`      | Close pane                |
| `Ctrl-b` `b`      | Toggle agent sidebar      |
| `Ctrl-b` `C`      | New workspace             |
| `Ctrl-b` `N`      | Next workspace            |
| `Ctrl-b` `P`      | Previous workspace        |
| `Ctrl-b` `?`      | Show key bindings         |
| `Ctrl-b` `Ctrl-b` | Send `Ctrl-b` to the terminal |
| `Esc`             | Cancel the prefix         |

Keys without a binding are discarded after the prefix.
