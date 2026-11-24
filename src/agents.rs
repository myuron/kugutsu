//! ターミナルで動いているコーディング Agent（Claude Code / Codex など）の検出と状態判定。
//!
//! herdr と同様に、フォアグラウンドのプロセスから Agent を特定し、
//! 画面下部の内容を Agent ごとの既知の UI パターンと照合して状態を推定する。

use crate::terminal::process::ProcessInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    ClaudeCode,
    Codex,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    /// 処理を実行している。
    Working,
    /// 権限確認などでユーザーの応答を待っている。
    Blocked,
    /// 見ていない間に処理が終わった（フォーカスすると [`AgentState::Idle`] に戻る）。
    Done,
    /// 入力待ち。
    Idle,
}

impl AgentState {
    pub fn label(self) -> &'static str {
        match self {
            AgentState::Working => "working",
            AgentState::Blocked => "blocked",
            AgentState::Done => "done",
            AgentState::Idle => "idle",
        }
    }
}

/// 状態の判定に使う、画面末尾（空行を除く）の行数。
const SCAN_LINES: usize = 15;

/// Agent ごとの画面パターン（小文字で、行に含まれるかで照合する）。
struct Patterns {
    working: &'static [&'static str],
    blocked: &'static [&'static str],
}

const CLAUDE_CODE_PATTERNS: Patterns = Patterns {
    working: &["esc to interrupt"],
    blocked: &[
        "do you want to proceed?",
        "do you want to make this edit",
        "do you want to create",
    ],
};

const CODEX_PATTERNS: Patterns = Patterns {
    working: &["esc to interrupt"],
    blocked: &[
        "would you like to run the following command",
        "would you like to make the following edits",
        "allow command?",
        "yes, proceed",
    ],
};

/// スクリプトを引数に取って Agent を起動するインタプリタ。
const INTERPRETERS: &[&str] = &["node", "bun", "deno"];

/// パスを除いたファイル名。
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl AgentKind {
    fn from_program(path: &str) -> Option<Self> {
        let mut name = basename(path);
        name = name.strip_suffix(".js").unwrap_or(name);
        // Nix の makeWrapper は実体を `.<name>-wrapped` にする
        if let Some(wrapped) = name
            .strip_prefix('.')
            .and_then(|n| n.strip_suffix("-wrapped"))
        {
            name = wrapped;
        }
        match name {
            "claude" => Some(AgentKind::ClaudeCode),
            "codex" => Some(AgentKind::Codex),
            _ => None,
        }
    }

    fn patterns(self) -> &'static Patterns {
        match self {
            AgentKind::ClaudeCode => &CLAUDE_CODE_PATTERNS,
            AgentKind::Codex => &CODEX_PATTERNS,
        }
    }
}

/// フォアグラウンドのプロセスがどの Agent か。
pub fn detect_agent(process: &ProcessInfo) -> Option<AgentKind> {
    let program = process.argv.first().map(|path| basename(path));
    let names = || std::iter::once(process.comm.as_str()).chain(program);
    if let Some(kind) = names().find_map(AgentKind::from_program) {
        return Some(kind);
    }
    // node などで起動された場合はスクリプト名で判断する
    if !names().any(|name| INTERPRETERS.contains(&name)) {
        return None;
    }
    AgentKind::from_program(process.argv.get(1)?)
}

/// 画面の行から Agent の状態を判定する（[`AgentState::Done`] は返さない）。
pub fn detect_state(kind: AgentKind, lines: &[String]) -> AgentState {
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map_or(0, |last| last + 1);
    let bottom: Vec<String> = lines[end.saturating_sub(SCAN_LINES)..end]
        .iter()
        .map(|line| line.to_lowercase())
        .collect();
    let contains_any = |patterns: &[&str]| {
        bottom
            .iter()
            .any(|line| patterns.iter().any(|pattern| line.contains(pattern)))
    };
    let patterns = kind.patterns();
    if contains_any(patterns.blocked) {
        AgentState::Blocked
    } else if contains_any(patterns.working) {
        AgentState::Working
    } else {
        AgentState::Idle
    }
}

/// Agent の状態を追跡し、見ていない間の完了を [`AgentState::Done`] として残す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTracker {
    pub kind: AgentKind,
    pub state: AgentState,
}

impl AgentTracker {
    pub fn new(kind: AgentKind) -> Self {
        Self {
            kind,
            state: AgentState::Idle,
        }
    }

    /// 画面から判定した状態で更新する。`focused` はユーザーがそのペインを見ているか。
    /// 状態が変わったら true を返す。
    pub fn update(&mut self, detected: AgentState, focused: bool) -> bool {
        let state = match detected {
            // 見ていない間に処理が終わったら、ユーザーが確認するまで Done として残す
            AgentState::Idle
                if !focused && matches!(self.state, AgentState::Working | AgentState::Done) =>
            {
                AgentState::Done
            }
            state => state,
        };
        let changed = self.state != state;
        self.state = state;
        changed
    }
}

/// 検出した Agent（無ければ `None`）と画面から追跡状態を更新する。画面は Agent がいるときだけ読む。
/// 変化があれば true を返す。
pub fn refresh(
    tracker: &mut Option<AgentTracker>,
    kind: Option<AgentKind>,
    lines: impl FnOnce() -> Vec<String>,
    focused: bool,
) -> bool {
    let Some(kind) = kind else {
        return tracker.take().is_some();
    };
    let replaced = tracker.is_none_or(|current| current.kind != kind);
    if replaced {
        *tracker = Some(AgentTracker::new(kind));
    }
    let tracker = tracker.as_mut().expect("tracker was just set");
    tracker.update(detect_state(kind, &lines()), focused) || replaced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(comm: &str, argv: &[&str]) -> ProcessInfo {
        ProcessInfo {
            comm: comm.into(),
            argv: argv.iter().map(|arg| arg.to_string()).collect(),
        }
    }

    fn screen(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn detects_agents_by_process_name() {
        assert_eq!(
            detect_agent(&process("claude", &["claude"])),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            detect_agent(&process("codex", &["/usr/local/bin/codex", "--full-auto"])),
            Some(AgentKind::Codex)
        );
        assert_eq!(detect_agent(&process("zsh", &["-zsh"])), None);
        assert_eq!(detect_agent(&process("vim", &["vim", "claude"])), None);
    }

    #[test]
    fn detects_agents_behind_nix_wrappers() {
        assert_eq!(
            detect_agent(&process(
                ".claude-wrapped",
                &["/nix/store/abc-claude-code/bin/.claude-wrapped"]
            )),
            Some(AgentKind::ClaudeCode)
        );
    }

    #[test]
    fn detects_agents_launched_through_node() {
        assert_eq!(
            detect_agent(&process("node", &["node", "/opt/bin/claude"])),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            detect_agent(&process(
                "node",
                &["node", "/usr/lib/node_modules/@openai/codex/bin/codex.js"]
            )),
            Some(AgentKind::Codex)
        );
    }

    #[test]
    fn claude_code_states() {
        let idle = screen(&["> ", "  ? for shortcuts", ""]);
        assert_eq!(detect_state(AgentKind::ClaudeCode, &idle), AgentState::Idle);

        let working = screen(&["✻ Thinking… (12s · ↑ 1.2k tokens · esc to interrupt)", "> "]);
        assert_eq!(
            detect_state(AgentKind::ClaudeCode, &working),
            AgentState::Working
        );

        let blocked = screen(&[
            "Bash command",
            "  cargo test",
            "Do you want to proceed?",
            "❯ 1. Yes",
            "  2. No, and tell Claude what to do differently (esc)",
        ]);
        assert_eq!(
            detect_state(AgentKind::ClaudeCode, &blocked),
            AgentState::Blocked
        );

        let edit = screen(&["Do you want to make this edit to main.rs?", "❯ 1. Yes"]);
        assert_eq!(
            detect_state(AgentKind::ClaudeCode, &edit),
            AgentState::Blocked
        );
    }

    #[test]
    fn codex_states() {
        let idle = screen(&["▌ Ask Codex to do anything", "  ⏎ send"]);
        assert_eq!(detect_state(AgentKind::Codex, &idle), AgentState::Idle);

        let working = screen(&["• Working (5s • Esc to interrupt)"]);
        assert_eq!(
            detect_state(AgentKind::Codex, &working),
            AgentState::Working
        );

        let blocked = screen(&[
            "Would you like to run the following command?",
            "$ cargo test",
            "› 1. Yes, proceed (y)",
        ]);
        assert_eq!(
            detect_state(AgentKind::Codex, &blocked),
            AgentState::Blocked
        );
    }

    #[test]
    fn blocked_wins_over_working() {
        let lines = screen(&["(esc to interrupt)", "Do you want to proceed?"]);
        assert_eq!(
            detect_state(AgentKind::ClaudeCode, &lines),
            AgentState::Blocked
        );
    }

    #[test]
    fn only_the_bottom_of_the_screen_is_considered() {
        let mut lines = screen(&["Do you want to proceed?"]);
        lines.extend(std::iter::repeat_n(String::new(), 3));
        lines.extend(std::iter::repeat_n("output".to_string(), 30));
        assert_eq!(
            detect_state(AgentKind::ClaudeCode, &lines),
            AgentState::Idle
        );
    }

    #[test]
    fn finishing_while_unfocused_marks_done_until_focused() {
        let mut tracker = AgentTracker::new(AgentKind::ClaudeCode);
        assert!(tracker.update(AgentState::Working, false));
        assert!(tracker.update(AgentState::Idle, false));
        assert_eq!(tracker.state, AgentState::Done);
        // 見ていない間は Done のまま
        assert!(!tracker.update(AgentState::Idle, false));
        assert_eq!(tracker.state, AgentState::Done);
        // フォーカスすると Idle に戻る
        assert!(tracker.update(AgentState::Idle, true));
        assert_eq!(tracker.state, AgentState::Idle);
    }

    #[test]
    fn finishing_while_focused_goes_straight_to_idle() {
        let mut tracker = AgentTracker::new(AgentKind::Codex);
        tracker.update(AgentState::Working, true);
        assert!(tracker.update(AgentState::Idle, true));
        assert_eq!(tracker.state, AgentState::Idle);
    }

    #[test]
    fn idle_agent_does_not_become_done() {
        let mut tracker = AgentTracker::new(AgentKind::Codex);
        assert!(!tracker.update(AgentState::Idle, false));
        assert_eq!(tracker.state, AgentState::Idle);
    }

    #[test]
    fn done_agent_can_start_working_again() {
        let mut tracker = AgentTracker::new(AgentKind::ClaudeCode);
        tracker.update(AgentState::Working, false);
        tracker.update(AgentState::Idle, false);
        assert!(tracker.update(AgentState::Blocked, false));
        assert_eq!(tracker.state, AgentState::Blocked);
    }

    #[test]
    fn refresh_starts_tracking_a_new_agent() {
        let mut tracker = None;
        let lines = || screen(&["(esc to interrupt)"]);
        assert!(refresh(
            &mut tracker,
            Some(AgentKind::ClaudeCode),
            lines,
            true
        ));
        assert_eq!(
            tracker,
            Some(AgentTracker {
                kind: AgentKind::ClaudeCode,
                state: AgentState::Working
            })
        );
        assert!(!refresh(
            &mut tracker,
            Some(AgentKind::ClaudeCode),
            lines,
            true
        ));
    }

    #[test]
    fn refresh_forgets_agent_that_exited() {
        let mut tracker = Some(AgentTracker::new(AgentKind::Codex));
        assert!(refresh(&mut tracker, None, Vec::new, true));
        assert_eq!(tracker, None);
        assert!(!refresh(&mut tracker, None, Vec::new, true));
    }

    #[test]
    fn refresh_replaces_tracker_when_agent_changes() {
        let mut tracker = Some(AgentTracker::new(AgentKind::Codex));
        assert!(refresh(
            &mut tracker,
            Some(AgentKind::ClaudeCode),
            Vec::new,
            true
        ));
        assert_eq!(tracker, Some(AgentTracker::new(AgentKind::ClaudeCode)));
    }

    #[test]
    fn refresh_skips_screen_when_no_agent() {
        let mut tracker = None;
        refresh(&mut tracker, None, || panic!("screen read"), true);
    }

    #[test]
    fn state_labels() {
        assert_eq!(AgentState::Working.label(), "working");
        assert_eq!(AgentState::Blocked.label(), "blocked");
        assert_eq!(AgentState::Done.label(), "done");
        assert_eq!(AgentState::Idle.label(), "idle");
    }
}
