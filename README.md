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

Keys without a binding are discarded after the prefix. The prefix can be changed with `kugutsu.opt.prefix`, and the bindings with `kugutsu.keymap` (see [Key mappings](#key-mappings)).

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
kugutsu.opt.theme = "kugutsu-light"
kugutsu.opt.background_opacity = 0.9
```

| Option        | Type   | Default    | Description |
| ------------- | ------ | ---------- | ----------- |
| `prefix`      | string | `"ctrl-b"` | Prefix key, written like `ctrl-a` or `alt-space` |
| `font_family` | string | first installed font of a built-in monospace list | Terminal font. If it is not installed, the built-in list is used |
| `font_size`   | number | `14`       | Terminal font size in pixels |
| `line_height` | number | `1.3`      | Line height as a multiple of the font size |
| `theme`       | string | `"kugutsu-dark"` | Color theme (see [Themes](#themes)) |
| `background_opacity` | number | `1.0` | Opacity of the background, from `0.0` (fully transparent) to `1.0` (opaque) |
| `background_blur` | boolean | `false` | Blur what is behind a translucent window |

The following are errors:

- Syntax errors and runtime errors in `init.lua`
- Options other than the ones above (e.g. `kugutsu.opt.foo = 1`)
- Values of the wrong type (e.g. `kugutsu.opt.font_size = "big"`)
- A `prefix` that is not a valid keystroke, or an empty `font_family`
- A `font_size` or `line_height` that is zero, negative, NaN or infinite
- A `theme` that is not found (see [Themes](#themes)) or a theme file that is invalid
- A `background_opacity` outside `0.0`–`1.0`, or NaN
- Invalid key mappings (see [Key mappings](#key-mappings))
- An invalid plugin spec, and errors while loading a plugin (see [Plugins](#plugins))

Errors are shown on the right of the status line with the line number in `init.lua`, and Kugutsu keeps running. Options set before the error still apply; the rest keep their defaults.

`~/.config/kugutsu/lua/` (the `lua/` directory next to `init.lua`) is on the module path, so a large configuration can be split into modules and loaded with `require`.

### Key mappings

`kugutsu.keymap.set(key, action, opts)` maps a key to an action, and `kugutsu.keymap.del(key)` removes a mapping. The [default key bindings](#default-keybind) stay unless you replace or delete them.

```lua
-- Ctrl-b t opens a tab; the description is shown in the which-key list and in Ctrl-b ?
kugutsu.keymap.set("<prefix>t", "new_tab", { desc = "Open a tab" })
-- Ctrl-b | splits the pane left / right
kugutsu.keymap.set("<prefix>|", "split_horizontal")
-- Ctrl-Shift-t opens a tab without the prefix
kugutsu.keymap.set("ctrl-shift-t", "new_tab")
-- A Lua function can run several actions
kugutsu.keymap.set("<prefix>w", function()
  kugutsu.action.new_tab()
  kugutsu.action.split_horizontal()
end, { desc = "Tab with two panes" })
-- Ctrl-b x no longer closes the pane
kugutsu.keymap.del("<prefix>x")
```

`key` is a keystroke written like `ctrl-a`, `shift-c` or `?`:

- `<prefix>t` — press the prefix, then `t`. `<prefix>` stands for the prefix set with `kugutsu.opt.prefix`, so mappings keep working when you change it.
- `ctrl-shift-t` — without `<prefix>`, the key runs the action by itself and is not sent to the terminal. The prefix key itself cannot be mapped this way.

Mapping a key that is already mapped replaces it in place; a new key is added to the end of the list.

`action` is either the name of an action or a Lua function:

| Action             | Default description       |
| ------------------ | ------------------------- |
| `new_tab`          | New tab                   |
| `next_tab`         | Next tab                  |
| `prev_tab`         | Previous tab              |
| `split_horizontal` | Split pane left / right   |
| `split_vertical`   | Split pane top / bottom   |
| `focus_left`       | Focus left pane           |
| `focus_down`       | Focus pane below          |
| `focus_up`         | Focus pane above          |
| `focus_right`      | Focus right pane          |
| `close_pane`       | Close pane                |
| `toggle_sidebar`   | Toggle agent sidebar      |
| `new_workspace`    | New workspace             |
| `next_workspace`   | Next workspace            |
| `prev_workspace`   | Previous workspace        |
| `show_keymap`      | Show key bindings         |

A Lua function runs when the key is pressed. Inside it, `kugutsu.action.<name>()` runs one of the actions above, and `kugutsu.action.select_tab(n)` selects tab `n` (1–9). The actions run in order after the function returns. `kugutsu.action` can only be called from a mapped function, not while `init.lua` is loading. Errors raised by the function are shown in the status line.

`opts.desc` is the description shown in the which-key list above the status line and in the key binding list (`Ctrl-b ?`). Without it, the default description of the action is used, or `Lua function` for a function. Adjacent mappings with the same description are shown as one row (e.g. `1-9  Select tab N`). The which-key list shows only `<prefix>` mappings.

The following are errors:

- A `key` that is not a string or not a valid keystroke, or `<prefix>` without a key after it
- An unknown action name, or an `action` that is neither a string nor a function
- An `opts` that is not a table, or a `desc` that is not a string
- Deleting a key that is not mapped
- Mapping the prefix key without `<prefix>`

### Themes

A theme sets the colors of the terminal and of the Kugutsu UI. Kugutsu bundles two themes:

- `kugutsu-dark` (default)
- `kugutsu-light`

Select one with `kugutsu.opt.theme`. The theme is looked up after `init.lua` and the plugins have been loaded, so the order of `kugutsu.opt.theme` and `kugutsu.plugins.setup` does not matter. The first match wins:

1. `~/.config/kugutsu/colors/<name>.lua` (your own themes, next to `init.lua`)
2. `colors/<name>.lua` of each loaded [plugin](#plugins), in the order of the spec
3. The bundled themes

An unknown theme name is reported as an error and the default theme is used.

A theme is a Lua file `colors/<name>.lua` that returns a table of colors written as `"#rrggbb"`. The bundled themes are in [`runtime/colors/`](runtime/colors/):

```lua
return {
  terminal = {
    foreground = "#d4d4d4",
    background = "#1e1e1e",
    cursor = "#aeafad",
    -- 16 colors: black, red, green, yellow, blue, magenta, cyan, white, then the bright ones
    ansi = { "#000000", "#cd3131", ... },
  },
  ui = {
    bar_background = "#181818",      -- tab bars, sidebar and status line
    bar_border = "#2b2b2b",          -- borders of the bars and pane separators
    inactive_foreground = "#8a8a8a", -- inactive tabs and labels
    active_pane_border = "#569cd6",  -- border around the active pane when split
    prefix_indicator = "#dcdcaa",    -- prefix key in the status line
    agent_working = "#569cd6",
    agent_blocked = "#e5c07b",
    agent_done = "#89d185",
    error = "#f14c4c",               -- errors in the status line
  },
}
```

Every key is required.

### Background transparency

`background_opacity` below `1.0` makes the window background translucent: the default background of the terminal and the UI bars become see-through. Cells with a background color set by the application and text stay opaque. With `background_blur = true`, Kugutsu also asks the compositor to blur what is behind the window.

Whether transparency and blur work depends on the platform and the compositor (e.g. a Wayland compositor that supports it). Where they are not supported, the window stays opaque.

## Plugins

Kugutsu has a built-in plugin manager. Plugins are declared in `init.lua`, pinned in a lockfile, and cloned into the XDG data directory, so `~/.config/kugutsu/` only holds your own configuration and can be kept in your dotfiles.

Like Nix flakes, **only explicit commands change plugins or the lockfile**. Starting Kugutsu only loads what is on disk: it never touches the network and never writes the lockfile or the plugins directory.

### Declaring plugins

```lua
kugutsu.plugins.setup({
  "catppuccin/catppuccin.kugutsu",                       -- GitHub shorthand
  { "folke/tokyonight.kugutsu", rev = "v1.0.0" },        -- tag, branch or commit
  { url = "https://git.example.com/me/foo.kugutsu.git" },
  { "bob/theme.kugutsu", name = "bob-theme.kugutsu" },   -- custom name
  { dir = "~/src/my-plugin.kugutsu" },                   -- local plugin for development, not managed
})
```

- `owner/repo` expands to `https://github.com/owner/repo.git`.
- The name of a plugin is the last part of its URL or directory without `.git` (e.g. `catppuccin.kugutsu`), or `name` if given. It is used for the directory under `plugins/`, the lockfile and `kugutsu update NAME`. Two plugins with the same name are an error.
- `rev` is a tag, a branch or a full commit. Without it, the default branch is used.
- `dir` plugins are always loaded from that directory and are not locked or installed. A relative `dir` is relative to the directory of `init.lua`.
- Unknown keys are an error, to catch typos such as `opt =`.
- If `kugutsu.plugins.setup` is called more than once, the last call wins.

Per-plugin configuration lives in the spec:

```lua
kugutsu.plugins.setup({
  { "bob/notes.kugutsu", opts = { width = 80 } },              -- require("notes").setup(opts)
  { "carol/notify.kugutsu", config = function(opts) ... end },  -- custom setup
  { "dave/foo-kugutsu", main = "foo", opts = {} },              -- module name differs from the plugin name
})
```

| Key      | Meaning |
| -------- | ------- |
| `opts`   | Table passed to `require(main).setup(opts)` |
| `config` | `function(opts)` called instead of `setup`; receives `opts` |
| `main`   | Module whose `setup` is called. Defaults to the plugin name without the `.kugutsu` suffix (`notes.kugutsu` -> `notes`) |

The spec is plain Lua, so it can be split into modules. Nested lists are flattened:

```lua
-- init.lua
kugutsu.plugins.setup({
  require("plugins.ui"),
  require("plugins.agents"),
  "catppuccin/catppuccin.kugutsu",
})

-- ~/.config/kugutsu/lua/plugins/agents.lua
return {
  { "alice/aider.kugutsu", opts = { width = 80 } },
}
```

### Where things live

| Path | Content | In dotfiles? |
| ---- | ------- | ------------ |
| `$XDG_CONFIG_HOME/kugutsu/init.lua` | Plugin spec and options | yes |
| `$XDG_CONFIG_HOME/kugutsu/kugutsu.lock` | Pinned commit of each plugin | yes |
| `$XDG_DATA_HOME/kugutsu/plugins/<name>/` | Cloned plugins | no |

Without the XDG variables, `~/.config` and `~/.local/share` are used. The lockfile always lives next to `init.lua` (also with `$KUGUTSU_CONFIG`).

### Commands

```
kugutsu                      start Kugutsu
kugutsu lock                 spec -> lockfile
kugutsu update [NAME...]     spec -> lockfile, moving plugins to their latest commits
kugutsu install              lockfile -> plugins directory
```

| Command | Writes | Nix analogy |
| ------- | ------ | ----------- |
| `lock` | Lockfile only: resolves plugins that are new or whose `url` / `rev` changed, removes plugins no longer in the spec, keeps the rest | `nix flake lock` |
| `update [NAME...]` | Lockfile only: resolves the given plugins (all if none) to the latest commit of their `rev` or default branch | `nix flake update` |
| `install` | Plugins directory only: clones or checks out the locked commits, and removes plugins that are no longer locked | `nix build` |

- `lock` and `update` only ask the remote for commits (`git ls-remote`); applying them always takes `kugutsu install`.
- Each command prints one line per plugin. A failing plugin does not stop the others, and the command exits with code 1.
- `lock` and `update` write the lockfile only if every plugin resolved.
- `git` must be installed. The Nix package brings its own `git`, used when no other `git` is on `PATH`.

A typical lifecycle:

1. Add a plugin to the spec. The status line shows `plugins: 1 missing — run kugutsu install`.
2. Run `kugutsu lock` and `kugutsu install`, then restart Kugutsu.
3. Commit `init.lua` and `kugutsu.lock` to your dotfiles. On another machine, `kugutsu install` checks out the same commits.
4. `kugutsu update` and `kugutsu install` move plugins to their latest commits. To roll back, revert the lockfile and run `kugutsu install`.
5. Remove a plugin from the spec. It is no longer loaded; `kugutsu lock` and `kugutsu install` remove it from the lockfile and from disk.

The lockfile is TOML, sorted by name:

```toml
version = 1

[plugins."catppuccin.kugutsu"]
url = "https://github.com/catppuccin/catppuccin.kugutsu.git"
rev = "v1.0.0"
commit = "def4567890abcdef1234567890abcdef12345678"
```

### Loading

A plugin is loaded on startup if it is in the spec and its directory exists; the lockfile is not read. Plugins are loaded in the order of the spec:

```
notes.kugutsu/
  plugin/*.lua    -- run when the plugin is loaded
  lua/notes/...   -- modules, require("notes")
  colors/*.lua    -- themes
```

1. `lua/` is added to the module path and `colors/` to the [theme](#themes) lookup.
2. `plugin/*.lua` are run in file name order.
3. `config(opts)` is called if given; otherwise, if `opts` is given, `require(main).setup(opts)` is called. `opts` without a `setup` function is an error.

Errors are shown in the status line with the plugin name (e.g. `notes.kugutsu: plugin/notes.lua:12: ...`). The rest of that plugin is skipped; other plugins are not affected.

### External plugins (Nix)

A symlink or a directory without `.git` under `$XDG_DATA_HOME/kugutsu/plugins/` is **external**: `kugutsu install` never clones, checks out or removes it, and Kugutsu loads it like any other plugin when its name is in the spec. This way plugins can be provided from the Nix store, and the same `init.lua` works on machines with and without Nix.
