-- Kugutsu の既定のダークテーマ。無彩色を基調にし、色は Agent の状態だけに使う。
return {
  terminal = {
    foreground = "#c9c9c9",
    background = "#0a0a0b",
    cursor = "#b5b5b5",
    ansi = {
      "#1a1b1d", "#c26a6a", "#7aa886", "#d1a257", "#7f9ab8", "#a58bb5", "#7fa8a8", "#c9c9c9",
      "#5a5a5a", "#d68282", "#8fbf9b", "#e0b874", "#98b0cc", "#bba2c9", "#96bfbf", "#ececec",
    },
  },
  ui = {
    bar_background = "#060606",
    bar_border = "#1a1b1d",
    inactive_foreground = "#808080",
    active_pane_border = "#b5b5b5",
    prefix_indicator = "#ececec",
    agent_working = "#b5b5b5",
    agent_blocked = "#d1a257",
    agent_done = "#7aa886",
    error = "#c26a6a",
    foreground = "#c9c9c9",
    accent = "#b5b5b5",
    raised_background = "#111213",
    overlay_background = "#18191b",
    strong_border = "#222326",
  },
}
