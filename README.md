<p align="center">
  <img src="assets/kugutsu.png" alt="Kugutsu" width="480">
</p>

# Kugutsu(傀儡)

## What Kugutsu?

Kugutsu is an ADE (Agentic Development Environment) written in Rust with [gpui](https://www.gpui.rs/).

It is a terminal workspace for running coding agents side by side:

- **Workspaces, tabs and panes** — group terminals into workspaces, open multiple tabs, and split panes vertically or horizontally, all driven by tmux-style prefix keys.
- **Agent sidebar** — detects coding agents running in each terminal (Claude Code, Codex) and lists them under their workspace with their tab and pane. Each state has its own icon and color, so the agents that need you stand out:
  - `working` (spinner) — the agent is processing
  - `blocked` (alert, pulsing on a tinted row) — the agent is waiting for your answer (e.g. a permission prompt)
  - `done` (check) — the agent finished while you were looking elsewhere
  - `idle` (hollow circle) — the agent is waiting for input

  The sidebar header shows how many agents are blocked, and the workspace and tab bars show a dot for each agent state. Click an agent to jump to its pane.
- **Status line** — the prefix key on the left, where you are (`Workspace 1 › 1: kugutsu › pane 1/3`) in the middle, and the number of working, blocked and done agents on the right.
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
| `Ctrl-b` `r`      | Reload `init.lua`         |
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

Kugutsu reads `init.lua` on startup from the first of these locations whose environment variable is set:

1. `$KUGUTSU_CONFIG` (the path of the file itself)
2. `$XDG_CONFIG_HOME/kugutsu/init.lua`
3. `~/.config/kugutsu/init.lua`

The file is plain Lua 5.4. Without the file, every option keeps its default.

Press `Ctrl-b` `r` (the `reload_config` action) to read `init.lua` again without restarting. It is evaluated in a fresh Lua state, plugins are loaded again, and the new options, key mappings, theme and background transparency apply to every open pane. Errors are shown in the status line as on startup, and options set before an error still apply.

```lua
kugutsu.opt.prefix = "ctrl-a"
kugutsu.opt.font_family = "JetBrains Mono"
kugutsu.opt.font_size = 14
kugutsu.opt.line_height = 1.3
kugutsu.opt.theme = "kugutsu-light"
kugutsu.opt.background_opacity = 0.9
kugutsu.opt.ui_font_family = "Inter"
kugutsu.opt.inactive_pane_opacity = 0.85
kugutsu.ui.item_width = 150
```

| Option        | Type   | Default    | Description |
| ------------- | ------ | ---------- | ----------- |
| `prefix`      | string | `"ctrl-b"` | Prefix key, written like `ctrl-a` or `alt-space` |
| `font_family` | string | first installed font of a built-in monospace list | Terminal font. If it is not installed, the built-in list is used |
| `font_size`   | number | `14`       | Terminal font size in pixels |
| `line_height` | number | `1.3`      | Line height as a multiple of the font size |
| `theme`       | string | `"kugutsu-dark"` | Color theme (see [Themes](#themes)) |
| `theme_overrides` | table | none | Colors that override the selected theme (see [Themes](#themes)) |
| `background_opacity` | number | `1.0` | Opacity of the background, from `0.0` (fully transparent) to `1.0` (opaque) |
| `background_blur` | boolean | `false` | Blur what is behind a translucent window |
| `ui_font_family` | string | first installed font of a built-in list of proportional fonts (Inter, Noto Sans, …) | Font of the bars, sidebar and status line. The terminal keeps `font_family` |
| `inactive_pane_opacity` | number | `0.85` | Opacity of the panes that are not active when a tab is split, from `0.0` to `1.0` (`1.0` turns dimming off) |

`kugutsu.ui` holds options for the layout of the UI:

| Option        | Type   | Default    | Description |
| ------------- | ------ | ---------- | ----------- |
| `item_width`  | number | `150`      | Width in pixels of each item in the workspace and tab bars. Both rows use the same width so they line up; longer labels are truncated |

The following are errors:

- Syntax errors and runtime errors in `init.lua`
- Options other than the ones above (e.g. `kugutsu.opt.foo = 1` or `kugutsu.ui.foo = 1`)
- Values of the wrong type (e.g. `kugutsu.opt.font_size = "big"`)
- A `prefix` that is not a valid keystroke, or an empty `font_family` or `ui_font_family`
- A `font_size`, `line_height` or `item_width` that is zero, negative, NaN or infinite
- A `theme` that is not found (see [Themes](#themes)) or a theme file that is invalid
- Invalid `theme_overrides`, or an invalid theme passed to `kugutsu.theme.register`
- A `background_opacity` or `inactive_pane_opacity` outside `0.0`–`1.0`, or NaN
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
| `reload_config`    | Reload init.lua           |

A Lua function runs when the key is pressed. Inside it, `kugutsu.action.<name>()` runs one of the actions above, and `kugutsu.action.select_tab(n)` selects tab `n` (1–9). The actions run in order after the function returns. `kugutsu.action` can only be called from a mapped function, not while `init.lua` is loading. Errors raised by the function are shown in the status line.

`opts.desc` is the description shown in the which-key list above the status line and in the key binding list (`Ctrl-b ?`). Without it, the default description of the action is used, or `Lua function` for a function. Adjacent mappings with the same description are shown as one row (e.g. `1-9  Select tab N`). The which-key list shows only `<prefix>` mappings. The key binding list groups the mappings into tabs, panes, workspaces and general; mapped Lua functions are listed under general.

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

Select one with `kugutsu.opt.theme`. The theme is looked up after `init.lua` and the plugins have been loaded, so the order of `kugutsu.opt.theme`, `kugutsu.plugins.setup` and `kugutsu.theme.register` does not matter. The first match wins:

1. `~/.config/kugutsu/colors/<name>.lua` (your own themes, next to `init.lua`)
2. Themes registered with [`kugutsu.theme.register`](#registering-themes-from-lua) (the last registration of a name wins)
3. `colors/<name>.lua` of each loaded [plugin](#plugins), in the order of the spec
4. The bundled themes

An unknown theme name is reported as an error and the default theme is used.

A theme is a Lua file `colors/<name>.lua` that returns a table of colors written as `"#rrggbb"`. The bundled themes are in [`runtime/colors/`](runtime/colors/):

```lua
return {
  terminal = {
    foreground = "#c9c9c9",
    background = "#0a0a0b",          -- also the background of the panes
    cursor = "#b5b5b5",
    -- 16 colors: black, red, green, yellow, blue, magenta, cyan, white, then the bright ones
    ansi = { "#1a1b1d", "#c26a6a", ... },
  },
  ui = {
    bar_background = "#060606",      -- window, bars, sidebar and status line
    bar_border = "#1a1b1d",          -- borders of the bars
    inactive_foreground = "#808080", -- inactive items and secondary labels
    active_pane_border = "#b5b5b5",  -- border around the active pane when split
    prefix_indicator = "#ececec",    -- prefix key in the status line while it is pressed
    agent_working = "#b5b5b5",
    agent_blocked = "#d1a257",
    agent_done = "#7aa886",
    error = "#c26a6a",               -- errors in the status line

    -- Optional
    foreground = "#c9c9c9",          -- UI text
    accent = "#b5b5b5",              -- glow around the active pane
    raised_background = "#111213",   -- active items
    overlay_background = "#18191b",  -- which-key and the key binding list
    strong_border = "#222326",       -- borders of panes and floating panels
  },
}
```

Every key is optional. The keys in `terminal` and the first group of `ui` keys that a theme leaves out come from `kugutsu-dark`. The second group of `ui` keys falls back to colors derived from the first group (after that fill-in), so themes written before they existed keep working:

| Key | Fallback |
| --- | --- |
| `foreground` | `terminal.foreground` |
| `accent` | `active_pane_border` |
| `raised_background` | `bar_background` mixed with 6% of `foreground` |
| `overlay_background` | `bar_background` mixed with 10% of `foreground` |
| `strong_border` | `bar_border` |

Unknown keys, colors that are not `"#rrggbb"` and an `ansi` list without exactly 16 colors are errors. The theme's name is the file name; a `name` key is allowed but ignored.

The bundled themes keep colors for the agent states and use grays for everything else, so the agents that need attention stand out.

#### Overriding colors

`kugutsu.opt.theme_overrides` sets single colors on top of the selected theme. It takes the same `terminal` and `ui` tables as a theme file (without `name`), and every key is optional:

```lua
kugutsu.opt.theme = "kugutsu-light"
kugutsu.opt.theme_overrides = {
  ui = { active_pane_border = "#ff8800" },
}
```

Only the given colors change: overriding `bar_background` does not recompute the derived colors such as `raised_background`, so set those too if needed. The overrides also apply when the theme is not found, on top of the default theme.

#### Registering themes from Lua

`kugutsu.theme.register(theme)` registers a theme table with a required `name`. A plugin can use it to build several flavors from one palette, e.g. in its `plugin/*.lua`:

```lua
local palettes = {
  ["mytheme-night"] = { bg = "#1a1b26", fg = "#c0caf5" },
  ["mytheme-day"] = { bg = "#e1e2e7", fg = "#3760bf" },
}
for name, p in pairs(palettes) do
  kugutsu.theme.register({
    name = name,
    terminal = { background = p.bg, foreground = p.fg },
    ui = { bar_background = p.bg },
  })
end
```

### Writing a theme plugin

A theme plugin is a [plugin](#plugins) with a `colors/` directory. Each `colors/<name>.lua` is a theme named `<name>`, and a plugin may ship several:

```
mytheme.kugutsu/
  colors/mytheme-night.lua
  colors/mytheme-day.lua
```

Install it like any plugin and select one of its themes:

```lua
kugutsu.plugins.setup({ "you/mytheme.kugutsu" })
kugutsu.opt.theme = "mytheme-night"
```

### Background transparency

`background_opacity` below `1.0` makes the window background translucent: the default background of the terminal and the UI bars become see-through. Cells with a background color set by the application and text stay opaque. With `background_blur = true`, Kugutsu also asks the compositor to blur what is behind the window.

Whether transparency and blur work depends on the platform and the compositor (e.g. a Wayland compositor that supports it). Where they are not supported, the window stays opaque.

## Plugins

Kugutsu has a built-in plugin manager. Plugins are declared in `init.lua`, pinned in a lockfile, and cloned into the XDG data directory, so `~/.config/kugutsu/` only holds your own configuration and can be kept in your dotfiles.

Like Nix flakes, **only explicit commands change plugins or the lockfile**. Starting Kugutsu only loads what is on disk: it never touches the network and never writes the lockfile or the plugins directory.

### Declaring plugins

```lua
kugutsu.plugins.setup({
  "myuron/catppuccin.kugutsu",                           -- GitHub shorthand
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
  "myuron/catppuccin.kugutsu",
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
2. Run `kugutsu lock` and `kugutsu install`, then reload the configuration with `Ctrl-b` `r` (or restart Kugutsu).
3. Commit `init.lua` and `kugutsu.lock` to your dotfiles. On another machine, `kugutsu install` checks out the same commits.
4. `kugutsu update` and `kugutsu install` move plugins to their latest commits. To roll back, revert the lockfile and run `kugutsu install`.
5. Remove a plugin from the spec. It is no longer loaded; `kugutsu lock` and `kugutsu install` remove it from the lockfile and from disk.

The lockfile is TOML, sorted by name:

```toml
version = 1

[plugins."catppuccin.kugutsu"]
url = "https://github.com/myuron/catppuccin.kugutsu.git"
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

### Home Manager

The flake provides a [Home Manager](https://github.com/nix-community/home-manager) module that installs Kugutsu and links plugins from the Nix store, so a Nix setup does not need `kugutsu lock` or `kugutsu install`:

```nix
# flake.nix
inputs.kugutsu.url = "github:myuron/kugutsu";

# home.nix
{ inputs, pkgs, ... }:
{
  imports = [ inputs.kugutsu.homeManagerModules.default ];

  programs.kugutsu = {
    enable = true;
    plugins = [
      (pkgs.fetchFromGitHub {
        name = "catppuccin.kugutsu"; # the plugin name in the spec
        owner = "myuron";
        repo = "catppuccin.kugutsu";
        rev = "v1.0.0";
        hash = "sha256-...";
      })
      { name = "notes.kugutsu"; src = ./notes; }
    ];
    initLua = ''
      kugutsu.plugins.setup({ "myuron/catppuccin.kugutsu", "me/notes.kugutsu" })
    '';
  };
}
```

| Option | Default | Description |
| ------ | ------- | ----------- |
| `enable` | `false` | Install Kugutsu |
| `package` | the package of this flake | The Kugutsu package |
| `plugins` | `[ ]` | Plugins linked to `$XDG_DATA_HOME/kugutsu/plugins/<name>`: a derivation, whose name is the plugin name, or `{ name, src }` |
| `initLua` | `null` | Contents of `$XDG_CONFIG_HOME/kugutsu/init.lua`. When `null`, the file is not managed |

- The links are external plugins, so `kugutsu install` leaves them alone, and the same `init.lua` works on machines without Nix.
- The name of each plugin must match its name in the spec (the last part of its URL without `.git`, or `name`). A plugin is loaded only when it is in the spec.
- `fetchFromGitHub` names its result `source` unless `name` is given; pass `name`, or use the `{ name, src }` form.
- Two plugins with the same name are an error.
