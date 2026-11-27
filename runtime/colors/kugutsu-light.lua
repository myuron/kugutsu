-- Kugutsu のライトテーマ。ダークテーマと同じく無彩色を基調にし、色は Agent の状態だけに使う。
return {
  terminal = {
    foreground = "#2a2a2b",
    background = "#fafafa",
    cursor = "#4a4a4a",
    ansi = {
      "#2a2a2b", "#b04a4a", "#3f7d4f", "#9a6b1f", "#3f5f88", "#7a5a8c", "#3f7878", "#8a8a8a",
      "#6a6a6a", "#c25b5b", "#4f8f5f", "#ad7d2d", "#4f71a0", "#8c6c9e", "#4f8a8a", "#b5b5b5",
    },
  },
  ui = {
    bar_background = "#f0f0f0",
    bar_border = "#e0e0e1",
    inactive_foreground = "#7a7a7a",
    active_pane_border = "#4a4a4a",
    prefix_indicator = "#111111",
    agent_working = "#4a4a4a",
    agent_blocked = "#b07d2a",
    agent_done = "#3f7d4f",
    error = "#b04a4a",
    foreground = "#2a2a2b",
    accent = "#4a4a4a",
    raised_background = "#e6e6e7",
    overlay_background = "#ffffff",
    strong_border = "#d4d4d6",
  },
}
