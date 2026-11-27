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

Inside the devShell, `KUGUTSU_CONFIG` points to `examples/config/kugutsu/init.lua`, so `cargo run` reads that file instead of your own `init.lua`. The `examples` directory is ignored by Git: create the file there to try out settings locally. Without it, every option keeps its default.

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

Keys without a binding are discarded after the prefix. The prefix can be changed with `kugutsu.opt.prefix` (see [Configuration](#configuration)).

### Scrolling

| Input                    | Action                          |
| ------------------------ | ------------------------------- |
| Mouse wheel / touchpad   | Scroll through the scrollback   |
| `Shift-PageUp`           | Scroll up one page              |
| `Shift-PageDown`         | Scroll down one page            |

Typing returns to the bottom. New output keeps the current scroll position. Programs that enable mouse reporting receive wheel events, and full-screen programs such as `less` or `vim` receive them as arrow keys.

## Configuration

Kugutsu reads `init.lua` once on startup from the first of these locations whose environment variable is set:

1. `$KUGUTSU_CONFIG` (the path of the file itself)
2. `$XDG_CONFIG_HOME/kugutsu/init.lua`
3. `~/.config/kugutsu/init.lua`

The file is plain Lua 5.4. Without the file, every option keeps its default.

```lua
kugutsu.opt.prefix = "ctrl-a"
kugutsu.opt.font_family = "JetBrains Mono"
kugutsu.opt.font_size = 14
kugutsu.opt.line_height = 1.3
```

| Option        | Type   | Default    | Description |
| ------------- | ------ | ---------- | ----------- |
| `prefix`      | string | `"ctrl-b"` | Prefix key, written like `ctrl-a` or `alt-space` |
| `font_family` | string | first installed font of a built-in monospace list | Terminal font. If it is not installed, the built-in list is used |
| `font_size`   | number | `14`       | Terminal font size in pixels |
| `line_height` | number | `1.3`      | Line height as a multiple of the font size |

The following are errors:

- Syntax errors and runtime errors in `init.lua`
- Options other than the ones above (e.g. `kugutsu.opt.foo = 1`)
- Values of the wrong type (e.g. `kugutsu.opt.font_size = "big"`)
- A `prefix` that is not a valid keystroke, or an empty `font_family`
- A `font_size` or `line_height` that is zero, negative, NaN or infinite

Errors are shown on the right of the status line with the line number in `init.lua`, and Kugutsu keeps running. Options set before the error still apply; the rest keep their defaults.

Colors and themes are not configurable yet.
