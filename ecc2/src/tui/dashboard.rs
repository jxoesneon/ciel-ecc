use chrono::{Duration, Utc};
use crossterm::event::KeyEvent;
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Table, TableState, Tabs,
        Wrap,
    },
};
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::UNIX_EPOCH;
use tokio::sync::broadcast;

use super::widgets::{budget_state, format_currency, format_token_count, BudgetState, TokenMeter};
use crate::comms;
use crate::config::{Config, PaneLayout, PaneNavigationAction, Theme};
use crate::notifications::{DesktopNotifier, NotificationEvent, WebhookNotifier};
use crate::observability::ToolLogEntry;
use crate::session::manager;
use crate::session::output::{
    OutputEvent, OutputLine, OutputStream, SessionOutputStore, OUTPUT_BUFFER_LIMIT,
};
use crate::session::store::{DaemonActivity, FileActivityOverlap, StateStore};
use crate::session::{
    ContextObservationPriority, DecisionLogEntry, FileActivityEntry, Session, SessionGrouping,
    SessionBoardMeta, SessionHarnessInfo, SessionMessage, SessionState,
};
use crate::worktree;

#[cfg(test)]
use crate::session::{SessionMetrics, WorktreeInfo};

const DEFAULT_GRID_SIZE_PERCENT: u16 = 50;
const OUTPUT_PANE_PERCENT: u16 = 70;
const MIN_PANE_SIZE_PERCENT: u16 = 20;
const MAX_PANE_SIZE_PERCENT: u16 = 80;
const PANE_RESIZE_STEP_PERCENT: u16 = 5;
const MAX_LOG_ENTRIES: u64 = 12;
const MAX_DIFF_PREVIEW_LINES: usize = 6;
const MAX_DIFF_PATCH_LINES: usize = 80;
const MAX_METRICS_GRAPH_RELATIONS: usize = 6;
const MAX_FILE_ACTIVITY_PATCH_LINES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeDiffColumns {
    removals: Text<'static>,
    additions: Text<'static>,
    hunk_offsets: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
struct ThemePalette {
    accent: Color,
    row_highlight_bg: Color,
    muted: Color,
    help_border: Color,
}

#[derive(Debug, Clone)]
struct SessionCompletionSummary {
    session_id: String,
    task: String,
    state: SessionState,
    files_changed: u32,
    tokens_used: u64,
    duration_secs: u64,
    cost_usd: f64,
    tests_run: usize,
    tests_passed: usize,
    recent_files: Vec<String>,
    key_decisions: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TestRunSummary {
    total: usize,
    passed: usize,
}

pub struct Dashboard {
    db: StateStore,
    cfg: Config,
    output_store: SessionOutputStore,
    output_rx: broadcast::Receiver<OutputEvent>,
    notifier: DesktopNotifier,
    webhook_notifier: WebhookNotifier,
    sessions: Vec<Session>,
    session_harnesses: HashMap<String, SessionHarnessInfo>,
    session_output_cache: HashMap<String, Vec<OutputLine>>,
    unread_message_counts: HashMap<String, usize>,
    approval_queue_counts: HashMap<String, usize>,
    approval_queue_preview: Vec<SessionMessage>,
    handoff_backlog_counts: HashMap<String, usize>,
    board_meta_by_session: HashMap<String, SessionBoardMeta>,
    worktree_health_by_session: HashMap<String, worktree::WorktreeHealth>,
    global_handoff_backlog_leads: usize,
    global_handoff_backlog_messages: usize,
    daemon_activity: DaemonActivity,
    selected_messages: Vec<SessionMessage>,
    selected_parent_session: Option<String>,
    selected_child_sessions: Vec<DelegatedChildSummary>,
    focused_delegate_session_id: Option<String>,
    selected_team_summary: Option<TeamSummary>,
    selected_route_preview: Option<String>,
    logs: Vec<ToolLogEntry>,
    selected_diff_summary: Option<String>,
    selected_diff_preview: Vec<String>,
    selected_diff_patch: Option<String>,
    selected_diff_hunk_offsets_unified: Vec<usize>,
    selected_diff_hunk_offsets_split: Vec<usize>,
    selected_diff_hunk: usize,
    diff_view_mode: DiffViewMode,
    selected_conflict_protocol: Option<String>,
    selected_merge_readiness: Option<worktree::MergeReadiness>,
    selected_git_status_entries: Vec<worktree::GitStatusEntry>,
    selected_git_status: usize,
    selected_git_patch: Option<worktree::GitStatusPatchView>,
    selected_git_patch_hunk_offsets_unified: Vec<usize>,
    selected_git_patch_hunk_offsets_split: Vec<usize>,
    selected_git_patch_hunk: usize,
    output_mode: OutputMode,
    graph_entity_filter: GraphEntityFilter,
    output_filter: OutputFilter,
    output_time_filter: OutputTimeFilter,
    timeline_event_filter: TimelineEventFilter,
    timeline_scope: SearchScope,
    selected_pane: Pane,
    selected_session: usize,
    show_help: bool,
    operator_note: Option<String>,
    pane_command_mode: bool,
    output_follow: bool,
    output_scroll_offset: usize,
    last_output_height: usize,
    metrics_scroll_offset: usize,
    last_metrics_height: usize,
    pane_size_percent: u16,
    collapsed_panes: HashSet<Pane>,
    search_input: Option<String>,
    spawn_input: Option<String>,
    commit_input: Option<String>,
    pr_input: Option<String>,
    search_query: Option<String>,
    search_scope: SearchScope,
    search_agent_filter: SearchAgentFilter,
    search_matches: Vec<SearchMatch>,
    selected_search_match: usize,
    active_completion_popup: Option<SessionCompletionSummary>,
    queued_completion_popups: VecDeque<SessionCompletionSummary>,
    session_table_state: TableState,
    last_cost_metrics_signature: Option<(u64, u128)>,
    last_tool_activity_signature: Option<(u64, u128)>,
    last_budget_alert_state: BudgetState,
    last_session_states: HashMap<String, SessionState>,
    last_seen_approval_message_id: Option<i64>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct SessionSummary {
    total: usize,
    projects: usize,
    task_groups: usize,
    pending: usize,
    running: usize,
    idle: usize,
    stale: usize,
    completed: usize,
    failed: usize,
    stopped: usize,
    unread_messages: usize,
    inbox_sessions: usize,
    conflicted_worktrees: usize,
    in_progress_worktrees: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Pane {
    Sessions,
    Output,
    Metrics,
    Board,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    SessionOutput,
    Timeline,
    ContextGraph,
    WorktreeDiff,
    ConflictProtocol,
    GitStatus,
    GitPatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphEntityFilter {
    All,
    Decisions,
    Files,
    Functions,
    Sessions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffViewMode {
    Split,
    Unified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFilter {
    All,
    ErrorsOnly,
    ToolCallsOnly,
    FileChangesOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputTimeFilter {
    AllTime,
    Last15Minutes,
    LastHour,
    Last24Hours,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineEventFilter {
    All,
    Lifecycle,
    Messages,
    ToolCalls,
    FileChanges,
    Decisions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchScope {
    SelectedSession,
    AllSessions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchAgentFilter {
    AllAgents,
    SelectedAgentType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchMatch {
    session_id: String,
    line_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphDisplayLine {
    session_id: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PrPromptSpec {
    title: String,
    base_branch: Option<String>,
    labels: Vec<String>,
    reviewers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineEventType {
    Lifecycle,
    Message,
    ToolCall,
    FileChange,
    Decision,
}

#[derive(Debug, Clone)]
struct TimelineEvent {
    occurred_at: chrono::DateTime<Utc>,
    session_id: String,
    event_type: TimelineEventType,
    summary: String,
    detail_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnRequest {
    AdHoc {
        requested_count: usize,
        task: String,
    },
    Template {
        name: String,
        task: Option<String>,
        variables: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnPlan {
    AdHoc {
        requested_count: usize,
        spawn_count: usize,
        task: String,
    },
    Template {
        name: String,
        task: Option<String>,
        variables: BTreeMap<String, String>,
        step_count: usize,
    },
}

#[derive(Debug, Clone, Copy)]
struct PaneAreas {
    sessions: Rect,
    output: Option<Rect>,
    metrics: Option<Rect>,
    log: Option<Rect>,
}

impl PaneAreas {
    fn assign(&mut self, pane: Pane, area: Rect) {
        match pane {
            Pane::Sessions => self.sessions = area,
            Pane::Output => self.output = Some(area),
            Pane::Metrics | Pane::Board => self.metrics = Some(area),
            Pane::Log => self.log = Some(area),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AggregateUsage {
    total_tokens: u64,
    total_cost_usd: f64,
    token_state: BudgetState,
    cost_state: BudgetState,
    overall_state: BudgetState,
}

#[derive(Debug, Clone)]
struct DelegatedChildSummary {
    session_id: String,
    state: SessionState,
    worktree_health: Option<worktree::WorktreeHealth>,
    approval_backlog: usize,
    handoff_backlog: usize,
    tokens_used: u64,
    files_changed: u32,
    duration_secs: u64,
    task_preview: String,
    branch: Option<String>,
    last_output_preview: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TeamSummary {
    total: usize,
    idle: usize,
    running: usize,
    pending: usize,
    stale: usize,
    failed: usize,
    stopped: usize,
}

impl SessionCompletionSummary {
    fn title(&self) -> String {
        match self.state {
            SessionState::Completed => "ECC 2.0: Session completed".to_string(),
            SessionState::Failed => "ECC 2.0: Session failed".to_string(),
            _ => "ECC 2.0: Session summary".to_string(),
        }
    }

    fn subtitle(&self) -> String {
        format!(
            "{} | {}",
            format_session_id(&self.session_id),
            truncate_for_dashboard(&self.task, 88)
        )
    }

    fn notification_body(&self) -> String {
        let tests_line = if self.tests_run > 0 {
            format!(
                "Tests {} run / {} passed",
                self.tests_run, self.tests_passed
            )
        } else {
            "Tests not detected".to_string()
        };

        let warnings_line = if self.warnings.is_empty() {
            "Warnings none".to_string()
        } else {
            format!(
                "Warnings {}",
                truncate_for_dashboard(&self.warnings.join("; "), 88)
            )
        };

        [
            self.subtitle(),
            format!(
                "Files {} | Tokens {} | Duration {}",
                self.files_changed,
                format_token_count(self.tokens_used),
                format_duration(self.duration_secs)
            ),
            tests_line,
            warnings_line,
        ]
        .join("\n")
    }

    fn popup_text(&self) -> String {
        let mut lines = vec![
            self.subtitle(),
            String::new(),
            format!(
                "Files {} | Tokens {} | Cost {} | Duration {}",
                self.files_changed,
                format_token_count(self.tokens_used),
                format_currency(self.cost_usd),
                format_duration(self.duration_secs)
            ),
        ];

        if self.tests_run > 0 {
            lines.push(format!(
                "Tests {} run / {} passed",
                self.tests_run, self.tests_passed
            ));
        } else {
            lines.push("Tests not detected".to_string());
        }

        if !self.recent_files.is_empty() {
            lines.push(String::new());
            lines.push("Recent files".to_string());
            for item in &self.recent_files {
                lines.push(format!("- {item}"));
            }
        }

        if !self.key_decisions.is_empty() {
            lines.push(String::new());
            lines.push("Key decisions".to_string());
            for item in &self.key_decisions {
                lines.push(format!("- {item}"));
            }
        }

        if !self.warnings.is_empty() {
            lines.push(String::new());
            lines.push("Warnings".to_string());
            for item in &self.warnings {
                lines.push(format!("- {item}"));
            }
        }

        lines.push(String::new());
        lines.push("[Enter]/[Space]/[Esc] dismiss".to_string());
        lines.join("\n")
    }
}

fn load_session_harnesses(
    db: &StateStore,
    cfg: &Config,
    sessions: &[Session],
) -> HashMap<String, SessionHarnessInfo> {
    let working_dirs = sessions
        .iter()
        .map(|session| (session.id.as_str(), session.working_dir.as_path()))
        .collect::<HashMap<_, _>>();
    db.list_session_harnesses()
        .unwrap_or_default()
        .into_iter()
        .map(|(session_id, info)| {
            let info = if let Some(working_dir) = working_dirs.get(session_id.as_str()) {
                info.with_config_detection(cfg, working_dir)
            } else {
                info
            };
            (session_id, info)
        })
        .collect()
}

impl Dashboard {
    pub fn new(db: StateStore, cfg: Config) -> Self {
        Self::with_output_store(db, cfg, SessionOutputStore::default())
    }

    pub fn with_output_store(
        db: StateStore,
        cfg: Config,
        output_store: SessionOutputStore,
    ) -> Self {
        let pane_size_percent = configured_pane_size(&cfg, cfg.pane_layout);
        let initial_cost_metrics_signature = metrics_file_signature(&cfg.cost_metrics_path());
        let initial_tool_activity_signature =
            metrics_file_signature(&cfg.tool_activity_metrics_path());
        let _ = db.refresh_session_durations();
        if initial_cost_metrics_signature.is_some() {
            let _ = db.sync_cost_tracker_metrics(&cfg.cost_metrics_path());
        }
        if initial_tool_activity_signature.is_some() {
            let _ = db.sync_tool_activity_metrics(&cfg.tool_activity_metrics_path());
        }
        let sessions = db.list_sessions().unwrap_or_default();
        let session_harnesses = load_session_harnesses(&db, &cfg, &sessions);
        let initial_session_states = sessions
            .iter()
            .map(|session| (session.id.clone(), session.state.clone()))
            .collect();
        let initial_approval_message_id = db
            .latest_unread_approval_message()
            .ok()
            .flatten()
            .map(|message| message.id);
        let output_rx = output_store.subscribe();
        let notifier = DesktopNotifier::new(cfg.desktop_notifications.clone());
        let webhook_notifier = WebhookNotifier::new(cfg.webhook_notifications.clone());
        let mut session_table_state = TableState::default();
        if !sessions.is_empty() {
            session_table_state.select(Some(0));
        }

        let mut dashboard = Self {
            db,
            cfg,
            output_store,
            output_rx,
            notifier,
            webhook_notifier,
            sessions,
            session_harnesses,
            session_output_cache: HashMap::new(),
            unread_message_counts: HashMap::new(),
            approval_queue_counts: HashMap::new(),
            approval_queue_preview: Vec::new(),
            handoff_backlog_counts: HashMap::new(),
            board_meta_by_session: HashMap::new(),
            worktree_health_by_session: HashMap::new(),
            global_handoff_backlog_leads: 0,
            global_handoff_backlog_messages: 0,
            daemon_activity: DaemonActivity::default(),
            selected_messages: Vec::new(),
            selected_parent_session: None,
            selected_child_sessions: Vec::new(),
            focused_delegate_session_id: None,
            selected_team_summary: None,
            selected_route_preview: None,
            logs: Vec::new(),
            selected_diff_summary: None,
            selected_diff_preview: Vec::new(),
            selected_diff_patch: None,
            selected_diff_hunk_offsets_unified: Vec::new(),
            selected_diff_hunk_offsets_split: Vec::new(),
            selected_diff_hunk: 0,
            diff_view_mode: DiffViewMode::Split,
            selected_conflict_protocol: None,
            selected_merge_readiness: None,
            selected_git_status_entries: Vec::new(),
            selected_git_status: 0,
            selected_git_patch: None,
            selected_git_patch_hunk_offsets_unified: Vec::new(),
            selected_git_patch_hunk_offsets_split: Vec::new(),
            selected_git_patch_hunk: 0,
            output_mode: OutputMode::SessionOutput,
            graph_entity_filter: GraphEntityFilter::All,
            output_filter: OutputFilter::All,
            output_time_filter: OutputTimeFilter::AllTime,
            timeline_event_filter: TimelineEventFilter::All,
            timeline_scope: SearchScope::SelectedSession,
            selected_pane: Pane::Sessions,
            selected_session: 0,
            show_help: false,
            operator_note: None,
            pane_command_mode: false,
            output_follow: true,
            output_scroll_offset: 0,
            last_output_height: 0,
            metrics_scroll_offset: 0,
            last_metrics_height: 0,
            pane_size_percent,
            collapsed_panes: HashSet::new(),
            search_input: None,
            spawn_input: None,
            commit_input: None,
            pr_input: None,
            search_query: None,
            search_scope: SearchScope::SelectedSession,
            search_agent_filter: SearchAgentFilter::AllAgents,
            search_matches: Vec::new(),
            selected_search_match: 0,
            active_completion_popup: None,
            queued_completion_popups: VecDeque::new(),
            session_table_state,
            last_cost_metrics_signature: initial_cost_metrics_signature,
            last_tool_activity_signature: initial_tool_activity_signature,
            last_budget_alert_state: BudgetState::Normal,
            last_session_states: initial_session_states,
            last_seen_approval_message_id: initial_approval_message_id,
        };
        sort_sessions_for_display(&mut dashboard.sessions);
        dashboard.unread_message_counts = dashboard.db.unread_message_counts().unwrap_or_default();
        dashboard.sync_approval_queue();
        dashboard.sync_handoff_backlog_counts();
        dashboard.sync_board_meta();
        dashboard.sync_global_handoff_backlog();
        dashboard.sync_selected_output();
        dashboard.sync_selected_diff();
        dashboard.sync_selected_messages();
        dashboard.sync_selected_lineage();
        dashboard.refresh_logs();
        dashboard.last_budget_alert_state = dashboard.aggregate_usage().overall_state;
        dashboard
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .split(frame.area());

        self.render_header(frame, chunks[0]);

        if self.show_help {
            self.render_help(frame, chunks[1]);
        } else {
            let pane_areas = self.pane_areas(chunks[1]);
            self.render_sessions(frame, pane_areas.sessions);
            if let Some(output_area) = pane_areas.output {
                self.render_output(frame, output_area);
            }
            if let Some(metrics_area) = pane_areas.metrics {
                self.render_metrics(frame, metrics_area);
            }

            if let Some(log_area) = pane_areas.log {
                self.render_log(frame, log_area);
            }
        }

        self.render_status_bar(frame, chunks[2]);

        if let Some(summary) = self.active_completion_popup.as_ref() {
            self.render_completion_popup(frame, summary);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let running = self
            .sessions
            .iter()
            .filter(|session| session.state == SessionState::Running)
            .count();
        let total = self.sessions.len();
        let palette = self.theme_palette();

        let title = format!(
            " ECC 2.0 | {running} running / {total} total | {} {}% | {} ",
            self.layout_label(),
            self.pane_size_percent,
            self.theme_label()
        );
        let tabs = Tabs::new(
            self.visible_panes()
                .iter()
                .map(|pane| pane.title())
                .collect::<Vec<_>>(),
        )
        .block(Block::default().borders(Borders::ALL).title(title))
        .select(self.selected_pane_index())
        .highlight_style(
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        );

        frame.render_widget(tabs, area);
    }

    fn render_sessions(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Sessions ")
            .border_style(self.pane_border_style(Pane::Sessions));
        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if inner_area.is_empty() {
            return;
        }

        let stabilized = self
            .daemon_activity
            .stabilized_after_recovery_at()
            .is_some();
        let summary = SessionSummary::from_sessions(
            &self.sessions,
            &self.handoff_backlog_counts,
            &self.worktree_health_by_session,
            stabilized,
        );
        let mut overview_lines = vec![
            summary_line(&summary),
            attention_queue_line(&summary, stabilized),
            approval_queue_line(&self.approval_queue_counts),
        ];
        if let Some(preview) = approval_queue_preview_line(&self.approval_queue_preview) {
            overview_lines.push(preview);
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(overview_lines.len() as u16),
                Constraint::Min(3),
            ])
            .split(inner_area);

        frame.render_widget(Paragraph::new(overview_lines), chunks[0]);

        let mut previous_project: Option<&str> = None;
        let mut previous_task_group: Option<&str> = None;
        let rows = self.sessions.iter().map(|session| {
            let project_cell = if previous_project == Some(session.project.as_str()) {
                None
            } else {
                previous_project = Some(session.project.as_str());
                previous_task_group = None;
                Some(session.project.clone())
            };
            let task_group_cell = if previous_task_group == Some(session.task_group.as_str()) {
                None
            } else {
                previous_task_group = Some(session.task_group.as_str());
                Some(session.task_group.clone())
            };

            session_row(
                session,
                project_cell,
                task_group_cell,
                self.approval_queue_counts
                    .get(&session.id)
                    .copied()
                    .unwrap_or(0),
                self.handoff_backlog_counts
                    .get(&session.id)
                    .copied()
                    .unwrap_or(0),
            )
        });
        let header = Row::new([
            "ID",
            "Project",
            "Group",
            "Agent",
            "State",
            "Branch",
            "Approvals",
            "Backlog",
            "Tokens",
            "Tools",
            "Files",
            "Duration",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));
        let widths = [
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(12),
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(8),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .highlight_symbol(">> ")
            .highlight_spacing(HighlightSpacing::Always)
            .row_highlight_style(
                Style::default()
                    .bg(self.theme_palette().row_highlight_bg)
                    .add_modifier(Modifier::BOLD),
            );

        let selected = if self.sessions.is_empty() {
            None
        } else {
            Some(self.selected_session.min(self.sessions.len() - 1))
        };
        if self.session_table_state.selected() != selected {
            self.session_table_state.select(selected);
        }

        frame.render_stateful_widget(table, chunks[1], &mut self.session_table_state);
    }

    fn render_output(&mut self, frame: &mut Frame, area: Rect) {
        self.sync_output_scroll(area.height.saturating_sub(2) as usize);

        if self.sessions.get(self.selected_session).is_some()
            && matches!(
                self.output_mode,
                OutputMode::WorktreeDiff | OutputMode::GitPatch
            )
            && self.active_patch_text().is_some()
            && self.diff_view_mode == DiffViewMode::Split
        {
            self.render_split_diff_output(frame, area);
            return;
        }

        let (title, content) = if self.sessions.get(self.selected_session).is_some() {
            match self.output_mode {
                OutputMode::SessionOutput => {
                    let lines = self.visible_output_lines();
                    let content = if lines.is_empty() {
                        Text::from(self.empty_output_message())
                    } else if self.search_query.is_some() {
                        self.render_searchable_output(&lines)
                    } else {
                        Text::from(
                            lines
                                .iter()
                                .map(|line| Line::from(line.text.clone()))
                                .collect::<Vec<_>>(),
                        )
                    };
                    (self.output_title(), content)
                }
                OutputMode::Timeline => {
                    let lines = self.visible_timeline_lines();
                    let content = if lines.is_empty() {
                        Text::from(self.empty_timeline_message())
                    } else {
                        Text::from(lines)
                    };
                    (self.output_title(), content)
                }
                OutputMode::ContextGraph => {
                    let lines = self.visible_graph_lines();
                    let content = if lines.is_empty() {
                        Text::from(self.empty_graph_message())
                    } else if self.search_query.is_some() {
                        self.render_searchable_graph(&lines)
                    } else {
                        Text::from(
                            lines
                                .into_iter()
                                .map(|line| Line::from(line.text))
                                .collect::<Vec<_>>(),
                        )
                    };
                    (self.output_title(), content)
                }
                OutputMode::WorktreeDiff => {
                    let content = if let Some(patch) = self.selected_diff_patch.as_ref() {
                        build_unified_diff_text(patch, self.theme_palette())
                    } else {
                        Text::from(
                            self.selected_diff_summary
                                .as_ref()
                                .map(|summary| {
                                    format!(
                                        "{summary}\n\nNo patch content to preview yet. The worktree may be clean or only have summary-level changes."
                                    )
                                })
                                .unwrap_or_else(|| {
                                    "No worktree diff available for the selected session."
                                        .to_string()
                                }),
                        )
                    };
                    (self.output_title(), content)
                }
                OutputMode::GitPatch => {
                    let content = if let Some(patch) = self.selected_git_patch.as_ref() {
                        build_unified_diff_text(&patch.patch, self.theme_palette())
                    } else {
                        Text::from(
                            "No selected-file patch available for the current git-status entry.",
                        )
                    };
                    (self.output_title(), content)
                }
                OutputMode::ConflictProtocol => {
                    let content = self.selected_conflict_protocol.clone().unwrap_or_else(|| {
                        "No conflicted worktree available for the selected session.".to_string()
                    });
                    (" Conflict Protocol ".to_string(), Text::from(content))
                }
                OutputMode::GitStatus => {
                    let content = if self.selected_git_status_entries.is_empty() {
                        Text::from(self.empty_git_status_message())
                    } else {
                        Text::from(self.visible_git_status_lines())
                    };
                    (self.output_title(), content)
                }
            }
        } else {
            (
                self.output_title(),
                Text::from("No sessions. Press 'n' to start one."),
            )
        };

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(self.pane_border_style(Pane::Output)),
            )
            .scroll((self.output_scroll_offset as u16, 0));
        frame.render_widget(paragraph, area);
    }

    fn render_split_diff_output(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.output_title())
            .border_style(self.pane_border_style(Pane::Output));
        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if inner_area.is_empty() {
            return;
        }

        let Some(patch) = self.active_patch_text() else {
            return;
        };
        let columns = build_worktree_diff_columns(patch, self.theme_palette());
        let column_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner_area);

        let removals = Paragraph::new(columns.removals)
            .block(Block::default().borders(Borders::ALL).title(" Removals "))
            .scroll((self.output_scroll_offset as u16, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(removals, column_chunks[0]);

        let additions = Paragraph::new(columns.additions)
            .block(Block::default().borders(Borders::ALL).title(" Additions "))
            .scroll((self.output_scroll_offset as u16, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(additions, column_chunks[1]);
    }

    fn output_title(&self) -> String {
        if self.output_mode == OutputMode::Timeline {
            return format!(
                " Timeline{}{}{} ",
                self.timeline_scope.title_suffix(),
                self.timeline_event_filter.title_suffix(),
                self.output_time_filter.title_suffix()
            );
        }

        if self.output_mode == OutputMode::ContextGraph {
            let scope = self.search_scope.title_suffix();
            let filter = self.graph_entity_filter.title_suffix();
            let time = self.output_time_filter.title_suffix();
            if let Some(input) = self.search_input.as_ref() {
                return format!(" Graph{scope}{filter}{time} /{input}_ ");
            }
            if let Some(query) = self.search_query.as_ref() {
                let total = self.search_matches.len();
                let current = if total == 0 {
                    0
                } else {
                    self.selected_search_match.min(total.saturating_sub(1)) + 1
                };
                return format!(" Graph{scope}{filter}{time} /{query} {current}/{total} ");
            }
            return format!(" Graph{scope}{filter}{time} ");
        }

        if self.output_mode == OutputMode::WorktreeDiff {
            return format!(
                " Diff{}{} ",
                self.diff_view_mode.title_suffix(),
                self.diff_hunk_title_suffix()
            );
        }

        if self.output_mode == OutputMode::GitPatch {
            let path = self
                .selected_git_patch
                .as_ref()
                .map(|patch| patch.display_path.as_str())
                .unwrap_or("selected file");
            return format!(
                " Git patch {}{}{} ",
                path,
                self.diff_view_mode.title_suffix(),
                self.diff_hunk_title_suffix()
            );
        }

        if self.output_mode == OutputMode::GitStatus {
            let staged = self
                .selected_git_status_entries
                .iter()
                .filter(|entry| entry.staged)
                .count();
            let unstaged = self
                .selected_git_status_entries
                .iter()
                .filter(|entry| entry.unstaged || entry.untracked)
                .count();
            let total = self.selected_git_status_entries.len();
            let current = if total == 0 {
                0
            } else {
                self.selected_git_status.min(total.saturating_sub(1)) + 1
            };
            return format!(" Git status staged:{staged} unstaged:{unstaged} {current}/{total} ");
        }

        let filter = format!(
            "{}{}",
            self.output_filter.title_suffix(),
            self.output_time_filter.title_suffix()
        );
        let scope = self.search_scope.title_suffix();
        let agent = self.search_agent_title_suffix();
        if let Some(input) = self.search_input.as_ref() {
            return format!(" Output{filter}{scope}{agent} /{input}_ ");
        }

        if let Some(query) = self.search_query.as_ref() {
            let total = self.search_matches.len();
            let current = if total == 0 {
                0
            } else {
                self.selected_search_match.min(total.saturating_sub(1)) + 1
            };
            return format!(" Output{filter}{scope}{agent} /{query} {current}/{total} ");
        }

        format!(" Output{filter}{scope}{agent} ")
    }

    fn empty_output_message(&self) -> &'static str {
        match (self.output_filter, self.output_time_filter) {
            (OutputFilter::All, OutputTimeFilter::AllTime) => "Waiting for session output...",
            (OutputFilter::ErrorsOnly, OutputTimeFilter::AllTime) => {
                "No stderr output for this session yet."
            }
            (OutputFilter::ToolCallsOnly, OutputTimeFilter::AllTime) => {
                "No tool-call output for this session yet."
            }
            (OutputFilter::FileChangesOnly, OutputTimeFilter::AllTime) => {
                "No file-change output for this session yet."
            }
            (OutputFilter::All, _) => "No output lines in the selected time range.",
            (OutputFilter::ErrorsOnly, _) => "No stderr output in the selected time range.",
            (OutputFilter::ToolCallsOnly, _) => "No tool-call output in the selected time range.",
            (OutputFilter::FileChangesOnly, _) => {
                "No file-change output in the selected time range."
            }
        }
    }

    fn empty_git_status_message(&self) -> &'static str {
        "No staged or unstaged changes for this worktree."
    }

    fn empty_timeline_message(&self) -> &'static str {
        match (
            self.timeline_scope,
            self.timeline_event_filter,
            self.output_time_filter,
        ) {
            (SearchScope::AllSessions, TimelineEventFilter::All, OutputTimeFilter::AllTime) => {
                "No timeline events across all sessions yet."
            }
            (
                SearchScope::AllSessions,
                TimelineEventFilter::Lifecycle,
                OutputTimeFilter::AllTime,
            ) => "No lifecycle events across all sessions yet.",
            (
                SearchScope::AllSessions,
                TimelineEventFilter::Messages,
                OutputTimeFilter::AllTime,
            ) => "No message events across all sessions yet.",
            (
                SearchScope::AllSessions,
                TimelineEventFilter::ToolCalls,
                OutputTimeFilter::AllTime,
            ) => "No tool-call events across all sessions yet.",
            (
                SearchScope::AllSessions,
                TimelineEventFilter::FileChanges,
                OutputTimeFilter::AllTime,
            ) => "No file-change events across all sessions yet.",
            (
                SearchScope::AllSessions,
                TimelineEventFilter::Decisions,
                OutputTimeFilter::AllTime,
            ) => "No decision-log events across all sessions yet.",
            (SearchScope::AllSessions, TimelineEventFilter::All, _) => {
                "No timeline events across all sessions in the selected time range."
            }
            (SearchScope::AllSessions, TimelineEventFilter::Lifecycle, _) => {
                "No lifecycle events across all sessions in the selected time range."
            }
            (SearchScope::AllSessions, TimelineEventFilter::Messages, _) => {
                "No message events across all sessions in the selected time range."
            }
            (SearchScope::AllSessions, TimelineEventFilter::ToolCalls, _) => {
                "No tool-call events across all sessions in the selected time range."
            }
            (SearchScope::AllSessions, TimelineEventFilter::FileChanges, _) => {
                "No file-change events across all sessions in the selected time range."
            }
            (SearchScope::AllSessions, TimelineEventFilter::Decisions, _) => {
                "No decision-log events across all sessions in the selected time range."
            }
            (SearchScope::SelectedSession, TimelineEventFilter::All, OutputTimeFilter::AllTime) => {
                "No timeline events for this session yet."
            }
            (
                SearchScope::SelectedSession,
                TimelineEventFilter::Lifecycle,
                OutputTimeFilter::AllTime,
            ) => "No lifecycle events for this session yet.",
            (
                SearchScope::SelectedSession,
                TimelineEventFilter::Messages,
                OutputTimeFilter::AllTime,
            ) => "No message events for this session yet.",
            (
                SearchScope::SelectedSession,
                TimelineEventFilter::ToolCalls,
                OutputTimeFilter::AllTime,
            ) => "No tool-call events for this session yet.",
            (
                SearchScope::SelectedSession,
                TimelineEventFilter::FileChanges,
                OutputTimeFilter::AllTime,
            ) => "No file-change events for this session yet.",
            (
                SearchScope::SelectedSession,
                TimelineEventFilter::Decisions,
                OutputTimeFilter::AllTime,
            ) => "No decision-log events for this session yet.",
            (SearchScope::SelectedSession, TimelineEventFilter::All, _) => {
                "No timeline events in the selected time range."
            }
            (SearchScope::SelectedSession, TimelineEventFilter::Lifecycle, _) => {
                "No lifecycle events in the selected time range."
            }
            (SearchScope::SelectedSession, TimelineEventFilter::Messages, _) => {
                "No message events in the selected time range."
            }
            (SearchScope::SelectedSession, TimelineEventFilter::ToolCalls, _) => {
                "No tool-call events in the selected time range."
            }
            (SearchScope::SelectedSession, TimelineEventFilter::FileChanges, _) => {
                "No file-change events in the selected time range."
            }
            (SearchScope::SelectedSession, TimelineEventFilter::Decisions, _) => {
                "No decision-log events in the selected time range."
            }
        }
    }

    fn empty_graph_message(&self) -> &'static str {
        match (
            self.search_scope,
            self.graph_entity_filter,
            self.output_time_filter,
        ) {
            (SearchScope::SelectedSession, GraphEntityFilter::All, OutputTimeFilter::AllTime) => {
                "No graph entities for this session yet."
            }
            (_, GraphEntityFilter::Decisions, OutputTimeFilter::AllTime) => {
                "No decision graph entities in the current scope yet."
            }
            (_, GraphEntityFilter::Files, OutputTimeFilter::AllTime) => {
                "No file graph entities in the current scope yet."
            }
            (_, GraphEntityFilter::Functions, OutputTimeFilter::AllTime) => {
                "No function graph entities in the current scope yet."
            }
            (_, GraphEntityFilter::Sessions, OutputTimeFilter::AllTime) => {
                "No session graph entities in the current scope yet."
            }
            (SearchScope::AllSessions, GraphEntityFilter::All, OutputTimeFilter::AllTime) => {
                "No graph entities across all sessions yet."
            }
            (_, _, _) => "No graph entities in the selected filter/time range.",
        }
    }

    fn render_searchable_output(&self, lines: &[&OutputLine]) -> Text<'static> {
        let Some(query) = self.search_query.as_deref() else {
            return Text::from(
                lines
                    .iter()
                    .map(|line| Line::from(line.text.clone()))
                    .collect::<Vec<_>>(),
            );
        };

        let selected_session_id = self.selected_session_id();
        let active_match = self.search_matches.get(self.selected_search_match);

        Text::from(
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    highlight_output_line(
                        &line.text,
                        query,
                        active_match
                            .zip(selected_session_id)
                            .map(|(search_match, session_id)| {
                                search_match.session_id == session_id
                                    && search_match.line_index == index
                            })
                            .unwrap_or(false),
                        self.theme_palette(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    fn render_searchable_graph(&self, lines: &[GraphDisplayLine]) -> Text<'static> {
        let Some(query) = self.search_query.as_deref() else {
            return Text::from(
                lines
                    .iter()
                    .map(|line| Line::from(line.text.clone()))
                    .collect::<Vec<_>>(),
            );
        };

        let active_match = self.search_matches.get(self.selected_search_match);

        Text::from(
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    highlight_output_line(
                        &line.text,
                        query,
                        active_match
                            .map(|search_match| {
                                search_match.session_id == line.session_id
                                    && search_match.line_index == index
                            })
                            .unwrap_or(false),
                        self.theme_palette(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    fn render_metrics(&mut self, frame: &mut Frame, area: Rect) {
        let side_pane = if self.selected_pane == Pane::Board {
            Pane::Board
        } else {
            Pane::Metrics
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(match side_pane {
                Pane::Board => " Board ",
                _ => " Metrics ",
            })
            .border_style(self.pane_border_style(side_pane));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.is_empty() {
            return;
        }

        if side_pane == Pane::Board {
            frame.render_widget(
                Paragraph::new(self.board_text())
                    .scroll((self.metrics_scroll_offset as u16, 0))
                    .wrap(Wrap { trim: true }),
                inner,
            );
            self.sync_metrics_scroll(inner.height as usize);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(inner);

        let aggregate = self.aggregate_usage();
        let thresholds = self.cfg.effective_budget_alert_thresholds();
        frame.render_widget(
            TokenMeter::tokens(
                "Token Budget",
                aggregate.total_tokens,
                self.cfg.token_budget,
                thresholds,
            ),
            chunks[0],
        );
        frame.render_widget(
            TokenMeter::currency(
                "Cost Budget",
                aggregate.total_cost_usd,
                self.cfg.cost_budget_usd,
                thresholds,
            ),
            chunks[1],
        );
        frame.render_widget(
            Paragraph::new(self.selected_session_metrics_text())
                .scroll((self.metrics_scroll_offset as u16, 0))
                .wrap(Wrap { trim: true }),
            chunks[2],
        );
        self.sync_metrics_scroll(chunks[2].height as usize);
    }

    fn render_log(&self, frame: &mut Frame, area: Rect) {
        let content = if self.sessions.get(self.selected_session).is_none() {
            "No session selected.".to_string()
        } else if self.logs.is_empty() {
            "No tool logs available for this session yet.".to_string()
        } else {
            self.logs
                .iter()
                .map(|entry| {
                    let mut block = format!(
                        "[{}] {} | {}ms | risk {:.0}%",
                        self.short_timestamp(&entry.timestamp),
                        entry.tool_name,
                        entry.duration_ms,
                        entry.risk_score * 100.0,
                    );
                    if !entry.trigger_summary.trim().is_empty() {
                        block.push_str(&format!(
                            "\nwhy: {}",
                            self.log_field(&entry.trigger_summary)
                        ));
                    }
                    if entry.input_params_json.trim() != "{}" {
                        block.push_str(&format!(
                            "\nparams: {}",
                            self.log_field(&entry.input_params_json)
                        ));
                    }
                    block.push_str(&format!(
                        "\ninput: {}\noutput: {}",
                        self.log_field(&entry.input_summary),
                        self.log_field(&entry.output_summary)
                    ));
                    block
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        };

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Log ")
                    .border_style(self.pane_border_style(Pane::Log)),
            )
            .scroll((self.output_scroll_offset as u16, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let base_text = format!(
            " [n]ew session  natural spawn [N]  [a]ssign  re[b]alance  global re[B]alance  dra[i]n inbox  approval jump [I]  [g]lobal dispatch  coordinate [G]lobal  collapse pane [h]  restore panes [H]  timeline [y]  timeline filter [E]  file patch [v]  git status [z]  stage [S]  unstage [U]  reset [R]  commit [C]  create PR [P]  diff mode [V]  hunks [{{/}}]  conflict proto[c]ol  cont[e]nt filter  time [f]ilter  scope [A]  agent filter [o]  [m]erge  merge ready [M]  auto-worktree [t]  auto-merge [w]  toggle [p]olicy  [,/.] dispatch limit  [s]top  [u]resume  [x]cleanup  prune inactive [X]  [d]elete  [r]efresh  [{}] focus pane  [Tab] cycle pane  [{}] move pane  [j/k] scroll  delegate [ or ]  [Enter] open  [+/-] resize  [l]ayout {}  [T]heme {}  [?] help  [q]uit ",
            self.pane_focus_shortcuts_label(),
            self.pane_move_shortcuts_label(),
            self.layout_label(),
            self.theme_label()
        );

        let search_prefix = if self.active_completion_popup.is_some() {
            " completion summary | [Enter]/[Space]/[Esc] dismiss |".to_string()
        } else if let Some(input) = self.spawn_input.as_ref() {
            format!(" spawn>{input}_ | [Enter] queue [Esc] cancel |")
        } else if let Some(input) = self.commit_input.as_ref() {
            format!(" commit>{input}_ | [Enter] commit [Esc] cancel |")
        } else if let Some(input) = self.pr_input.as_ref() {
            format!(
                " pr>{input}_ | [Enter] create draft PR | title | base=branch | labels=a,b | reviewers=a,b | [Esc] cancel |"
            )
        } else if let Some(input) = self.search_input.as_ref() {
            format!(
                " /{input}_ | {} | {} | [Enter] apply [Esc] cancel |",
                self.search_scope.label(),
                self.search_agent_filter_label()
            )
        } else if let Some(query) = self.search_query.as_ref() {
            let total = self.search_matches.len();
            let current = if total == 0 {
                0
            } else {
                self.selected_search_match.min(total.saturating_sub(1)) + 1
            };
            format!(
                " /{query} {current}/{total} | {} | {} | [n/N] navigate [Esc] clear |",
                self.search_scope.label(),
                self.search_agent_filter_label()
            )
        } else if self.pane_command_mode {
            " Ctrl+w | [h/j/k/l] move [1-4] focus [s/v/g] layout [+/-] resize [Esc] cancel |"
                .to_string()
        } else {
            String::new()
        };

        let text = if self.active_completion_popup.is_some()
            || self.spawn_input.is_some()
            || self.commit_input.is_some()
            || self.pr_input.is_some()
            || self.search_input.is_some()
            || self.search_query.is_some()
            || self.pane_command_mode
        {
            format!(" {search_prefix}")
        } else if let Some(note) = self.operator_note.as_ref() {
            format!(" {} |{}", truncate_for_dashboard(note, 96), base_text)
        } else {
            base_text
        };
        let aggregate = self.aggregate_usage();
        let (summary_text, summary_style) = self.aggregate_cost_summary();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(aggregate.overall_state.style());
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.is_empty() {
            return;
        }

        let summary_width = summary_text
            .len()
            .min(inner.width.saturating_sub(1) as usize) as u16;
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(summary_width)])
            .split(inner);

        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(self.theme_palette().muted)),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(summary_text)
                .style(summary_style)
                .alignment(Alignment::Right),
            chunks[1],
        );
    }

    fn render_completion_popup(&self, frame: &mut Frame, summary: &SessionCompletionSummary) {
        let popup_area = centered_rect(72, 65, frame.area());
        if popup_area.is_empty() {
            return;
        }

        frame.render_widget(Clear, popup_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", summary.title()))
            .border_style(self.pane_border_style(Pane::Output));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);
        if inner.is_empty() {
            return;
        }

        frame.render_widget(
            Paragraph::new(summary.popup_text())
                .wrap(Wrap { trim: true })
                .scroll((0, 0)),
            inner,
        );
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let help = vec![
            "Keyboard Shortcuts:".to_string(),
            "".to_string(),
            "  n       New session".to_string(),
            "  N       Natural-language multi-agent or template spawn prompt".to_string(),
            "  a       Assign follow-up work from selected session".to_string(),
            "  b       Rebalance backed-up delegate handoff backlog for selected lead".to_string(),
            "  B       Rebalance backed-up delegate handoff backlog across lead teams".to_string(),
            "  i       Drain unread task handoffs from selected lead".to_string(),
            "  I       Jump to the next unread approval/conflict target session".to_string(),
            "  g       Auto-dispatch unread handoffs across lead sessions".to_string(),
            "  G       Dispatch then rebalance backlog across lead teams".to_string(),
            "  K       Toggle selected-session context graph view".to_string(),
            "  h       Collapse the focused non-session pane".to_string(),
            "  H       Restore all collapsed panes".to_string(),
            "  y       Toggle selected-session timeline view".to_string(),
            "  E       Cycle timeline event filter or graph entity filter".to_string(),
            "  v       Toggle selected worktree diff or selected-file patch in output pane"
                .to_string(),
            "  z       Toggle selected worktree git status in output pane".to_string(),
            "  V       Toggle diff view mode between split and unified".to_string(),
            "  {/}     Jump to previous/next diff hunk in the active diff view".to_string(),
            "  S/U/R   Stage, unstage, or reset the selected file or active diff hunk".to_string(),
            "  C       Commit staged changes for the selected worktree".to_string(),
            "  P       Create a draft PR; supports title | base=branch | labels=a,b | reviewers=a,b".to_string(),
            "  c       Show conflict-resolution protocol for selected conflicted worktree"
                .to_string(),
            "  e       Cycle output content filter: all/errors/tool calls/file changes".to_string(),
            "  f       Cycle output or timeline time range between all/15m/1h/24h".to_string(),
            "  A       Toggle search, graph, or timeline scope between selected session and all sessions"
                .to_string(),
            "  o       Toggle search agent filter between all agents and selected agent type"
                .to_string(),
            "  m       Merge selected ready worktree into base and clean it up".to_string(),
            "  M       Merge all ready inactive worktrees and clean them up".to_string(),
            "  l       Cycle pane layout and persist it".to_string(),
            "  T       Toggle theme and persist it".to_string(),
            "  t       Toggle default worktree creation for new sessions and delegated work"
                .to_string(),
            "  p       Toggle daemon auto-dispatch policy and persist config".to_string(),
            "  w       Toggle daemon auto-merge for ready inactive worktrees".to_string(),
            "  ,/.     Decrease/increase auto-dispatch limit per lead".to_string(),
            "  s       Stop selected session".to_string(),
            "  u       Resume selected session".to_string(),
            "  x       Cleanup selected worktree".to_string(),
            "  X       Prune inactive worktrees globally".to_string(),
            "  d       Delete selected inactive session".to_string(),
            format!(
                "  {:<7} Focus Sessions/Output/Metrics/Log directly",
                self.pane_focus_shortcuts_label()
            ),
            "  Ctrl+w  Pane command mode: h/j/k/l move, s/v/g layout, 1-4 focus, +/- resize"
                .to_string(),
            "  Tab     Next pane".to_string(),
            "  S-Tab   Previous pane".to_string(),
            format!(
                "  {:<7} Move pane focus left/down/up/right",
                self.pane_move_shortcuts_label()
            ),
            "  j/↓     Scroll down".to_string(),
            "  k/↑     Scroll up".to_string(),
            "  [ or ]  Focus previous/next delegate in lead Metrics board".to_string(),
            "  Enter   Open focused delegate from lead Metrics board".to_string(),
            "  /       Search session output or graph lines".to_string(),
            "  n/N     Next/previous search match when search is active".to_string(),
            "  Esc     Clear active search or cancel search input".to_string(),
            "  +/=     Increase pane size and persist it".to_string(),
            "  -       Decrease pane size and persist it".to_string(),
            "  r       Refresh".to_string(),
            "  ?       Toggle help".to_string(),
            "  q/C-c   Quit".to_string(),
        ];

        let paragraph = Paragraph::new(help.join("\n")).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(self.theme_palette().help_border)),
        );
        frame.render_widget(paragraph, area);
    }

    pub fn next_pane(&mut self) {
        let visible_panes = self.visible_panes();
        let next_index = self
            .selected_pane_index()
            .checked_add(1)
            .map(|index| index % visible_panes.len())
            .unwrap_or(0);

        self.selected_pane = visible_panes[next_index];
    }

    pub fn prev_pane(&mut self) {
        let visible_panes = self.visible_panes();
        let previous_index = if self.selected_pane_index() == 0 {
            visible_panes.len() - 1
        } else {
            self.selected_pane_index() - 1
        };

        self.selected_pane = visible_panes[previous_index];
    }

    pub fn focus_pane_number(&mut self, slot: usize) {
        let Some(target) = Pane::from_shortcut(slot) else {
            self.set_operator_note(format!("pane {slot} is not available"));
            return;
        };

        if !self.is_pane_visible(target) {
            self.set_operator_note(format!(
                "{} pane is not visible",
                target.title().to_lowercase()
            ));
            return;
        }

        self.focus_pane(target);
    }

    pub fn focus_pane_left(&mut self) {
        self.move_pane_focus(PaneDirection::Left);
    }

    pub fn focus_pane_right(&mut self) {
        self.move_pane_focus(PaneDirection::Right);
    }

    pub fn focus_pane_up(&mut self) {
        self.move_pane_focus(PaneDirection::Up);
    }

    pub fn focus_pane_down(&mut self) {
        self.move_pane_focus(PaneDirection::Down);
    }

    pub fn begin_pane_command_mode(&mut self) {
        self.pane_command_mode = true;
        self.set_operator_note(
            "pane command mode | h/j/k/l move | s/v/g layout | 1-4 focus | +/- resize".to_string(),
        );
    }

    pub fn is_pane_command_mode(&self) -> bool {
        self.pane_command_mode
    }

    pub fn handle_pane_navigation_key(&mut self, key: KeyEvent) -> bool {
        match self.cfg.pane_navigation.action_for_key(key) {
            Some(PaneNavigationAction::FocusSlot(slot)) => {
                self.focus_pane_number(slot);
                true
            }
            Some(PaneNavigationAction::MoveLeft) => {
                self.focus_pane_left();
                true
            }
            Some(PaneNavigationAction::MoveDown) => {
                self.focus_pane_down();
                true
            }
            Some(PaneNavigationAction::MoveUp) => {
                self.focus_pane_up();
                true
            }
            Some(PaneNavigationAction::MoveRight) => {
                self.focus_pane_right();
                true
            }
            None => false,
        }
    }

    pub fn handle_pane_command_key(&mut self, key: KeyEvent) -> bool {
        if !self.pane_command_mode {
            return false;
        }

        self.pane_command_mode = false;
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.set_operator_note("pane command cancelled".to_string());
            }
            crossterm::event::KeyCode::Char('h') => self.focus_pane_left(),
            crossterm::event::KeyCode::Char('j') => self.focus_pane_down(),
            crossterm::event::KeyCode::Char('k') => self.focus_pane_up(),
            crossterm::event::KeyCode::Char('l') => self.focus_pane_right(),
            crossterm::event::KeyCode::Char('1') => self.focus_pane_number(1),
            crossterm::event::KeyCode::Char('2') => self.focus_pane_number(2),
            crossterm::event::KeyCode::Char('3') => self.focus_pane_number(3),
            crossterm::event::KeyCode::Char('4') => self.focus_pane_number(4),
            crossterm::event::KeyCode::Char('5') => self.focus_pane_number(5),
            crossterm::event::KeyCode::Char('+') | crossterm::event::KeyCode::Char('=') => {
                self.increase_pane_size()
            }
            crossterm::event::KeyCode::Char('-') => self.decrease_pane_size(),
            crossterm::event::KeyCode::Char('s') => self.set_pane_layout(PaneLayout::Horizontal),
            crossterm::event::KeyCode::Char('v') => self.set_pane_layout(PaneLayout::Vertical),
            crossterm::event::KeyCode::Char('g') => self.set_pane_layout(PaneLayout::Grid),
            _ => self.set_operator_note("unknown pane command".to_string()),
        }
        true
    }

    pub fn collapse_selected_pane(&mut self) {
        if self.selected_pane == Pane::Sessions {
            self.set_operator_note("cannot collapse sessions pane".to_string());
            return;
        }

        if self.visible_detail_panes().len() <= 1 {
            self.set_operator_note("cannot collapse last detail pane".to_string());
            return;
        }

        let collapsed = self.selected_pane;
        self.collapsed_panes.insert(collapsed);
        self.ensure_selected_pane_visible();
        self.set_operator_note(format!(
            "collapsed {} pane",
            collapsed.title().to_lowercase()
        ));
    }

    pub fn restore_collapsed_panes(&mut self) {
        if self.collapsed_panes.is_empty() {
            self.set_operator_note("no collapsed panes".to_string());
            return;
        }

        let restored_count = self.collapsed_panes.len();
        self.collapsed_panes.clear();
        self.ensure_selected_pane_visible();
        self.set_operator_note(format!("restored {restored_count} collapsed pane(s)"));
    }

    pub fn cycle_pane_layout(&mut self) {
        let config_path = crate::config::Config::config_path();
        self.cycle_pane_layout_with_save(&config_path, |cfg| cfg.save());
    }

    pub fn set_pane_layout(&mut self, layout: PaneLayout) {
        let config_path = crate::config::Config::config_path();
        self.set_pane_layout_with_save(layout, &config_path, |cfg| cfg.save());
    }

    fn cycle_pane_layout_with_save<F>(&mut self, config_path: &std::path::Path, save: F)
    where
        F: FnOnce(&Config) -> anyhow::Result<()>,
    {
        let previous_layout = self.cfg.pane_layout;
        let previous_pane_size = self.pane_size_percent;
        let previous_selected_pane = self.selected_pane;

        self.cfg.pane_layout = match self.cfg.pane_layout {
            PaneLayout::Horizontal => PaneLayout::Vertical,
            PaneLayout::Vertical => PaneLayout::Grid,
            PaneLayout::Grid => PaneLayout::Horizontal,
        };
        self.pane_size_percent = configured_pane_size(&self.cfg, self.cfg.pane_layout);
        self.persist_current_pane_size();
        self.ensure_selected_pane_visible();

        match save(&self.cfg) {
            Ok(()) => self.set_operator_note(format!(
                "pane layout set to {} | saved to {}",
                self.layout_label(),
                config_path.display()
            )),
            Err(error) => {
                self.cfg.pane_layout = previous_layout;
                self.pane_size_percent = previous_pane_size;
                self.selected_pane = previous_selected_pane;
                self.set_operator_note(format!("failed to persist pane layout: {error}"));
            }
        }
    }

    fn set_pane_layout_with_save<F>(
        &mut self,
        layout: PaneLayout,
        config_path: &std::path::Path,
        save: F,
    ) where
        F: FnOnce(&Config) -> anyhow::Result<()>,
    {
        if self.cfg.pane_layout == layout {
            self.set_operator_note(format!("pane layout already {}", self.layout_label()));
            return;
        }

        let previous_layout = self.cfg.pane_layout;
        let previous_pane_size = self.pane_size_percent;
        let previous_selected_pane = self.selected_pane;

        self.cfg.pane_layout = layout;
        self.pane_size_percent = configured_pane_size(&self.cfg, self.cfg.pane_layout);
        self.persist_current_pane_size();
        self.ensure_selected_pane_visible();

        match save(&self.cfg) {
            Ok(()) => self.set_operator_note(format!(
                "pane layout set to {} | saved to {}",
                self.layout_label(),
                config_path.display()
            )),
            Err(error) => {
                self.cfg.pane_layout = previous_layout;
                self.pane_size_percent = previous_pane_size;
                self.selected_pane = previous_selected_pane;
                self.set_operator_note(format!("failed to persist pane layout: {error}"));
            }
        }
    }

    fn auto_split_layout_after_spawn(&mut self, spawned_count: usize) -> Option<String> {
        let config_path = crate::config::Config::config_path();
        self.auto_split_layout_after_spawn_with_save(spawned_count, &config_path, |cfg| cfg.save())
    }

    fn auto_split_layout_after_spawn_with_save<F>(
        &mut self,
        spawned_count: usize,
        config_path: &std::path::Path,
        save: F,
    ) -> Option<String>
    where
        F: FnOnce(&Config) -> anyhow::Result<()>,
    {
        if spawned_count <= 1 {
            return None;
        }

        let live_session_count = self.active_session_count();
        let target_layout = recommended_spawn_layout(live_session_count);
        if self.cfg.pane_layout == target_layout {
            self.selected_pane = Pane::Sessions;
            self.ensure_selected_pane_visible();
            return Some(format!(
                "auto-focused sessions in {} layout for {} live session(s)",
                pane_layout_name(target_layout),
                live_session_count
            ));
        }

        let previous_layout = self.cfg.pane_layout;
        let previous_pane_size = self.pane_size_percent;
        let previous_selected_pane = self.selected_pane;

        self.cfg.pane_layout = target_layout;
        self.pane_size_percent = configured_pane_size(&self.cfg, target_layout);
        self.persist_current_pane_size();
        self.selected_pane = Pane::Sessions;
        self.ensure_selected_pane_visible();

        match save(&self.cfg) {
            Ok(()) => Some(format!(
                "auto-split {} layout for {} live session(s)",
                pane_layout_name(target_layout),
                live_session_count
            )),
            Err(error) => {
                self.cfg.pane_layout = previous_layout;
                self.pane_size_percent = previous_pane_size;
                self.selected_pane = previous_selected_pane;
                Some(format!(
                    "spawned {} session(s) but failed to persist auto-split layout to {}: {error}",
                    spawned_count,
                    config_path.display()
                ))
            }
        }
    }

    fn adjust_pane_size_with_save<F>(
        &mut self,
        delta: isize,
        config_path: &std::path::Path,
        save: F,
    ) where
        F: FnOnce(&Config) -> anyhow::Result<()>,
    {
        let previous_size = self.pane_size_percent;
        let previous_linear = self.cfg.linear_pane_size_percent;
        let previous_grid = self.cfg.grid_pane_size_percent;
        let next = (self.pane_size_percent as isize + delta).clamp(
            MIN_PANE_SIZE_PERCENT as isize,
            MAX_PANE_SIZE_PERCENT as isize,
        ) as u16;

        if next == self.pane_size_percent {
            self.set_operator_note(format!(
                "pane size unchanged at {}% for {} layout",
                self.pane_size_percent,
                self.layout_label()
            ));
            return;
        }

        self.pane_size_percent = next;
        self.persist_current_pane_size();

        match save(&self.cfg) {
            Ok(()) => self.set_operator_note(format!(
                "pane size set to {}% for {} layout | saved to {}",
                self.pane_size_percent,
                self.layout_label(),
                config_path.display()
            )),
            Err(error) => {
                self.pane_size_percent = previous_size;
                self.cfg.linear_pane_size_percent = previous_linear;
                self.cfg.grid_pane_size_percent = previous_grid;
                self.set_operator_note(format!("failed to persist pane size: {error}"));
            }
        }
    }

    fn persist_current_pane_size(&mut self) {
        match self.cfg.pane_layout {
            PaneLayout::Horizontal | PaneLayout::Vertical => {
                self.cfg.linear_pane_size_percent = self.pane_size_percent;
            }
            PaneLayout::Grid => {
                self.cfg.grid_pane_size_percent = self.pane_size_percent;
            }
        }
    }

    pub fn toggle_theme(&mut self) {
        let config_path = crate::config::Config::config_path();
        self.toggle_theme_with_save(&config_path, |cfg| cfg.save());
    }

    fn toggle_theme_with_save<F>(&mut self, config_path: &std::path::Path, save: F)
    where
        F: FnOnce(&Config) -> anyhow::Result<()>,
    {
        let previous_theme = self.cfg.theme;
        self.cfg.theme = match self.cfg.theme {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        };

        match save(&self.cfg) {
            Ok(()) => self.set_operator_note(format!(
                "theme set to {} | saved to {}",
                self.theme_label(),
                config_path.display()
            )),
            Err(error) => {
                self.cfg.theme = previous_theme;
                self.set_operator_note(format!("failed to persist theme: {error}"));
            }
        }
    }

    pub fn increase_pane_size(&mut self) {
        let config_path = crate::config::Config::config_path();
        self.adjust_pane_size_with_save(PANE_RESIZE_STEP_PERCENT as isize, &config_path, |cfg| {
            cfg.save()
        });
    }

    pub fn decrease_pane_size(&mut self) {
        let config_path = crate::config::Config::config_path();
        self.adjust_pane_size_with_save(
            -(PANE_RESIZE_STEP_PERCENT as isize),
            &config_path,
            |cfg| cfg.save(),
        );
    }

    pub fn scroll_down(&mut self) {
        match self.selected_pane {
            Pane::Sessions if !self.sessions.is_empty() => {
                self.selected_session = (self.selected_session + 1).min(self.sessions.len() - 1);
                self.sync_selection();
                self.reset_output_view();
                self.reset_metrics_view();
                self.sync_selected_output();
                self.sync_selected_diff();
                self.sync_selected_messages();
                self.sync_selected_lineage();
                self.refresh_logs();
            }
            Pane::Output => {
                if self.output_mode == OutputMode::GitStatus {
                    self.output_follow = false;
                    if self.selected_git_status + 1 < self.selected_git_status_entries.len() {
                        self.selected_git_status += 1;
                        self.sync_output_scroll(self.last_output_height.max(1));
                    }
                    return;
                }
                let max_scroll = self.max_output_scroll();
                if self.output_follow {
                    return;
                }

                if self.output_scroll_offset >= max_scroll.saturating_sub(1) {
                    self.output_follow = true;
                    self.output_scroll_offset = max_scroll;
                } else {
                    self.output_scroll_offset = self.output_scroll_offset.saturating_add(1);
                }
            }
            Pane::Metrics | Pane::Board => {
                let max_scroll = self.max_metrics_scroll();
                self.metrics_scroll_offset =
                    self.metrics_scroll_offset.saturating_add(1).min(max_scroll);
            }
            Pane::Log => {
                self.output_follow = false;
                self.output_scroll_offset = self.output_scroll_offset.saturating_add(1);
            }
            Pane::Sessions => {}
        }
    }

    pub fn scroll_up(&mut self) {
        match self.selected_pane {
            Pane::Sessions => {
                self.selected_session = self.selected_session.saturating_sub(1);
                self.sync_selection();
                self.reset_output_view();
                self.reset_metrics_view();
                self.sync_selected_output();
                self.sync_selected_diff();
                self.sync_selected_messages();
                self.sync_selected_lineage();
                self.refresh_logs();
            }
            Pane::Output => {
                if self.output_mode == OutputMode::GitStatus {
                    self.output_follow = false;
                    self.selected_git_status = self.selected_git_status.saturating_sub(1);
                    self.sync_output_scroll(self.last_output_height.max(1));
                    return;
                }
                if self.output_follow {
                    self.output_follow = false;
                    self.output_scroll_offset = self.max_output_scroll();
                }

                self.output_scroll_offset = self.output_scroll_offset.saturating_sub(1);
            }
            Pane::Metrics | Pane::Board => {
                self.metrics_scroll_offset = self.metrics_scroll_offset.saturating_sub(1);
            }
            Pane::Log => {
                self.output_follow = false;
                self.output_scroll_offset = self.output_scroll_offset.saturating_sub(1);
            }
        }
    }

    pub fn focus_next_delegate(&mut self) {
        let Some(current_index) = self.focused_delegate_index() else {
            return;
        };
        let next_index = (current_index + 1) % self.selected_child_sessions.len();
        self.set_focused_delegate_by_index(next_index);
    }

    pub fn focus_previous_delegate(&mut self) {
        let Some(current_index) = self.focused_delegate_index() else {
            return;
        };
        let previous_index = if current_index == 0 {
            self.selected_child_sessions.len() - 1
        } else {
            current_index - 1
        };
        self.set_focused_delegate_by_index(previous_index);
    }

    pub fn open_focused_delegate(&mut self) {
        let Some(delegate_session_id) = self
            .focused_delegate_index()
            .and_then(|index| self.selected_child_sessions.get(index))
            .map(|delegate| delegate.session_id.clone())
        else {
            return;
        };

        self.sync_selection_by_id(Some(&delegate_session_id));
        self.reset_output_view();
        self.reset_metrics_view();
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();
        self.set_operator_note(format!(
            "opened delegate {}",
            format_session_id(&delegate_session_id)
        ));
    }

    pub fn focus_next_approval_target(&mut self) {
        self.sync_approval_queue();
        let Some(target_session_id) = self.next_approval_target_session_id() else {
            self.set_operator_note("approval queue clear".to_string());
            return;
        };

        self.sync_selection_by_id(Some(&target_session_id));
        self.reset_output_view();
        self.reset_metrics_view();
        self.sync_selected_output();
        self.sync_selected_diff();
        self.unread_message_counts = self.db.unread_message_counts().unwrap_or_default();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();
        self.set_operator_note(format!(
            "focused approval target {}",
            format_session_id(&target_session_id)
        ));
    }

    pub async fn new_session(&mut self) {
        if self.active_session_count() >= self.cfg.max_parallel_sessions {
            tracing::warn!(
                "Cannot queue new session: active session limit reached ({})",
                self.cfg.max_parallel_sessions
            );
            self.set_operator_note(format!(
                "cannot queue new session: active session limit reached ({})",
                self.cfg.max_parallel_sessions
            ));
            return;
        }

        let task = self.new_session_task();
        let agent = self.cfg.default_agent.clone();
        let grouping = self
            .sessions
            .get(self.selected_session)
            .map(|session| SessionGrouping {
                project: Some(session.project.clone()),
                task_group: Some(session.task_group.clone()),
            })
            .unwrap_or_default();

        let session_id = match manager::create_session_with_grouping(
            &self.db,
            &self.cfg,
            &task,
            &agent,
            self.cfg.auto_create_worktrees,
            grouping,
        )
        .await
        {
            Ok(session_id) => session_id,
            Err(error) => {
                tracing::warn!("Failed to create new session from dashboard: {error}");
                self.set_operator_note(format!("new session failed: {error}"));
                return;
            }
        };

        if let Some(source_session) = self.sessions.get(self.selected_session) {
            let context = format!(
                "Dashboard handoff from {} [{}] | cwd {}{}",
                format_session_id(&source_session.id),
                source_session.agent_type,
                source_session.working_dir.display(),
                source_session
                    .worktree
                    .as_ref()
                    .map(|worktree| format!(
                        " | worktree {} ({})",
                        worktree.branch,
                        worktree.path.display()
                    ))
                    .unwrap_or_default()
            );
            if let Err(error) = comms::send(
                &self.db,
                &source_session.id,
                &session_id,
                &comms::MessageType::TaskHandoff {
                    task: source_session.task.clone(),
                    context,
                    priority: comms::TaskPriority::Normal,
                },
            ) {
                tracing::warn!(
                    "Failed to send handoff from session {} to {}: {error}",
                    source_session.id,
                    session_id
                );
            }
        }

        self.refresh();
        self.sync_selection_by_id(Some(&session_id));
        let queued_for_worktree = self
            .db
            .pending_worktree_queue_contains(&session_id)
            .unwrap_or(false);
        if queued_for_worktree {
            self.set_operator_note(format!(
                "queued session {} pending worktree slot",
                format_session_id(&session_id)
            ));
        } else {
            self.set_operator_note(format!(
                "spawned session {}",
                format_session_id(&session_id)
            ));
        }
        self.reset_output_view();
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();
        self.sync_budget_alerts();
    }

    pub fn toggle_output_mode(&mut self) {
        match self.output_mode {
            OutputMode::SessionOutput => {
                if self.selected_diff_patch.is_some() || self.selected_diff_summary.is_some() {
                    self.output_mode = OutputMode::WorktreeDiff;
                    self.selected_pane = Pane::Output;
                    self.output_follow = false;
                    self.output_scroll_offset = self.current_diff_hunk_offset();
                    self.set_operator_note("showing selected worktree diff".to_string());
                } else {
                    self.set_operator_note("no worktree diff for selected session".to_string());
                }
            }
            OutputMode::WorktreeDiff => {
                self.output_mode = OutputMode::SessionOutput;
                self.reset_output_view();
                self.set_operator_note("showing session output".to_string());
            }
            OutputMode::Timeline => {
                self.output_mode = OutputMode::SessionOutput;
                self.reset_output_view();
                self.set_operator_note("showing session output".to_string());
            }
            OutputMode::ContextGraph => {
                self.output_mode = OutputMode::SessionOutput;
                self.reset_output_view();
                self.set_operator_note("showing session output".to_string());
            }
            OutputMode::ConflictProtocol => {
                self.output_mode = OutputMode::SessionOutput;
                self.reset_output_view();
                self.set_operator_note("showing session output".to_string());
            }
            OutputMode::GitStatus => {
                self.sync_selected_git_patch();
                if self.selected_git_patch.is_some() {
                    self.output_mode = OutputMode::GitPatch;
                    self.selected_pane = Pane::Output;
                    self.output_follow = false;
                    self.output_scroll_offset = self.current_diff_hunk_offset();
                    self.set_operator_note("showing selected file patch".to_string());
                } else {
                    self.set_operator_note(
                        "no patch hunks available for the selected git-status entry".to_string(),
                    );
                }
            }
            OutputMode::GitPatch => {
                self.output_mode = OutputMode::GitStatus;
                self.output_follow = false;
                self.sync_output_scroll(self.last_output_height.max(1));
                self.set_operator_note("showing selected worktree git status".to_string());
            }
        }
    }

    pub fn toggle_git_status_mode(&mut self) {
        match self.output_mode {
            OutputMode::GitStatus | OutputMode::GitPatch => {
                self.output_mode = OutputMode::SessionOutput;
                self.reset_output_view();
                self.set_operator_note("showing session output".to_string());
            }
            _ => {
                let has_worktree = self
                    .sessions
                    .get(self.selected_session)
                    .and_then(|session| session.worktree.as_ref())
                    .is_some();
                if !has_worktree {
                    self.set_operator_note("selected session has no worktree".to_string());
                    return;
                }

                self.sync_selected_git_status();
                self.output_mode = OutputMode::GitStatus;
                self.selected_pane = Pane::Output;
                self.output_follow = false;
                self.sync_output_scroll(self.last_output_height.max(1));
                self.set_operator_note("showing selected worktree git status".to_string());
            }
        }
    }

    pub fn stage_selected_git_status(&mut self) {
        if self.output_mode == OutputMode::GitPatch {
            self.stage_selected_git_hunk();
            return;
        }

        if self.output_mode != OutputMode::GitStatus {
            self.set_operator_note(
                "git staging controls are only available in git status view".to_string(),
            );
            return;
        }

        let Some((entry, worktree)) = self.selected_git_status_context() else {
            self.set_operator_note("no git status entry selected".to_string());
            return;
        };

        if let Err(error) = worktree::stage_path(&worktree, &entry.path) {
            tracing::warn!("Failed to stage {}: {error}", entry.path);
            self.set_operator_note(format!("stage failed for {}: {error}", entry.display_path));
            return;
        }

        self.refresh_after_git_status_action(Some(&entry.path));
        self.set_operator_note(format!("staged {}", entry.display_path));
    }

    pub fn unstage_selected_git_status(&mut self) {
        if self.output_mode == OutputMode::GitPatch {
            self.unstage_selected_git_hunk();
            return;
        }

        if self.output_mode != OutputMode::GitStatus {
            self.set_operator_note(
                "git staging controls are only available in git status view".to_string(),
            );
            return;
        }

        let Some((entry, worktree)) = self.selected_git_status_context() else {
            self.set_operator_note("no git status entry selected".to_string());
            return;
        };

        if let Err(error) = worktree::unstage_path(&worktree, &entry.path) {
            tracing::warn!("Failed to unstage {}: {error}", entry.path);
            self.set_operator_note(format!(
                "unstage failed for {}: {error}",
                entry.display_path
            ));
            return;
        }

        self.refresh_after_git_status_action(Some(&entry.path));
        self.set_operator_note(format!("unstaged {}", entry.display_path));
    }

    pub fn reset_selected_git_status(&mut self) {
        if self.output_mode == OutputMode::GitPatch {
            self.reset_selected_git_hunk();
            return;
        }

        if self.output_mode != OutputMode::GitStatus {
            self.set_operator_note(
                "git staging controls are only available in git status view".to_string(),
            );
            return;
        }

        let Some((entry, worktree)) = self.selected_git_status_context() else {
            self.set_operator_note("no git status entry selected".to_string());
            return;
        };

        if let Err(error) = worktree::reset_path(&worktree, &entry) {
            tracing::warn!("Failed to reset {}: {error}", entry.path);
            self.set_operator_note(format!("reset failed for {}: {error}", entry.display_path));
            return;
        }

        self.refresh_after_git_status_action(Some(&entry.path));
        self.set_operator_note(format!("reset {}", entry.display_path));
    }

    pub fn begin_commit_prompt(&mut self) {
        if !matches!(
            self.output_mode,
            OutputMode::GitStatus | OutputMode::GitPatch
        ) {
            self.set_operator_note(
                "commit prompt is only available in git status view".to_string(),
            );
            return;
        }

        if self
            .sessions
            .get(self.selected_session)
            .and_then(|session| session.worktree.as_ref())
            .is_none()
        {
            self.set_operator_note("selected session has no worktree".to_string());
            return;
        }

        if !self
            .selected_git_status_entries
            .iter()
            .any(|entry| entry.staged)
        {
            self.set_operator_note("no staged changes to commit".to_string());
            return;
        }

        self.commit_input = Some(String::new());
        self.set_operator_note("commit mode | type a message and press Enter".to_string());
    }

    pub fn begin_pr_prompt(&mut self) {
        let Some(session) = self.sessions.get(self.selected_session) else {
            self.set_operator_note("no session selected".to_string());
            return;
        };
        let Some(worktree) = session.worktree.as_ref() else {
            self.set_operator_note("selected session has no worktree".to_string());
            return;
        };
        if worktree::has_uncommitted_changes(worktree).unwrap_or(false) {
            self.set_operator_note(
                "commit or reset worktree changes before creating a PR".to_string(),
            );
            return;
        }

        let seed = worktree::latest_commit_subject(worktree)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| session.task.clone());
        self.pr_input = Some(seed);
        self.set_operator_note(
            "pr mode | title | base=branch | labels=a,b | reviewers=a,b".to_string(),
        );
    }

    fn stage_selected_git_hunk(&mut self) {
        let Some((entry, worktree, _, hunk)) = self.selected_git_patch_context() else {
            self.set_operator_note("no git hunk selected".to_string());
            return;
        };

        if let Err(error) = worktree::stage_hunk(&worktree, &hunk) {
            tracing::warn!("Failed to stage hunk for {}: {error}", entry.path);
            self.set_operator_note(format!(
                "stage hunk failed for {}: {error}",
                entry.display_path
            ));
            return;
        }

        self.refresh_after_git_status_action(Some(&entry.path));
        self.set_operator_note(format!("staged hunk in {}", entry.display_path));
    }

    fn unstage_selected_git_hunk(&mut self) {
        let Some((entry, worktree, _, hunk)) = self.selected_git_patch_context() else {
            self.set_operator_note("no git hunk selected".to_string());
            return;
        };

        if let Err(error) = worktree::unstage_hunk(&worktree, &hunk) {
            tracing::warn!("Failed to unstage hunk for {}: {error}", entry.path);
            self.set_operator_note(format!(
                "unstage hunk failed for {}: {error}",
                entry.display_path
            ));
            return;
        }

        self.refresh_after_git_status_action(Some(&entry.path));
        self.set_operator_note(format!("unstaged hunk in {}", entry.display_path));
    }

    fn reset_selected_git_hunk(&mut self) {
        let Some((entry, worktree, _, hunk)) = self.selected_git_patch_context() else {
            self.set_operator_note("no git hunk selected".to_string());
            return;
        };

        if let Err(error) = worktree::reset_hunk(&worktree, &entry, &hunk) {
            tracing::warn!("Failed to reset hunk for {}: {error}", entry.path);
            self.set_operator_note(format!(
                "reset hunk failed for {}: {error}",
                entry.display_path
            ));
            return;
        }

        self.refresh_after_git_status_action(Some(&entry.path));
        self.set_operator_note(format!("reset hunk in {}", entry.display_path));
    }

    pub fn toggle_diff_view_mode(&mut self) {
        if !matches!(
            self.output_mode,
            OutputMode::WorktreeDiff | OutputMode::GitPatch
        ) || self.active_patch_text().is_none()
        {
            self.set_operator_note("no active worktree diff view to toggle".to_string());
            return;
        }

        self.diff_view_mode = match self.diff_view_mode {
            DiffViewMode::Split => DiffViewMode::Unified,
            DiffViewMode::Unified => DiffViewMode::Split,
        };
        self.output_follow = false;
        self.output_scroll_offset = self.current_diff_hunk_offset();
        self.set_operator_note(format!("diff view set to {}", self.diff_view_mode.label()));
    }

    pub fn next_diff_hunk(&mut self) {
        self.move_diff_hunk(1);
    }

    pub fn prev_diff_hunk(&mut self) {
        self.move_diff_hunk(-1);
    }

    fn move_diff_hunk(&mut self, delta: isize) {
        if !matches!(
            self.output_mode,
            OutputMode::WorktreeDiff | OutputMode::GitPatch
        ) || self.active_patch_text().is_none()
        {
            self.set_operator_note("no active worktree diff to navigate".to_string());
            return;
        }

        let (len, next_offset) = {
            let offsets = self.current_diff_hunk_offsets();
            if offsets.is_empty() {
                self.set_operator_note("no diff hunks in bounded preview".to_string());
                return;
            }

            let len = offsets.len();
            let next =
                (self.current_diff_hunk_index() as isize + delta).rem_euclid(len as isize) as usize;
            (len, offsets[next])
        };

        let next =
            (self.current_diff_hunk_index() as isize + delta).rem_euclid(len as isize) as usize;
        self.set_current_diff_hunk_index(next);
        self.output_follow = false;
        self.output_scroll_offset = next_offset;
        self.set_operator_note(format!("diff hunk {}/{}", next + 1, len));
    }

    pub fn toggle_timeline_mode(&mut self) {
        match self.output_mode {
            OutputMode::Timeline => {
                self.output_mode = OutputMode::SessionOutput;
                self.reset_output_view();
                self.set_operator_note("showing session output".to_string());
            }
            _ => {
                if self.sessions.get(self.selected_session).is_some() {
                    self.output_mode = OutputMode::Timeline;
                    self.selected_pane = Pane::Output;
                    self.output_follow = false;
                    self.output_scroll_offset = 0;
                    self.set_operator_note("showing selected session timeline".to_string());
                } else {
                    self.set_operator_note("no session selected for timeline view".to_string());
                }
            }
        }
    }

    pub fn toggle_conflict_protocol_mode(&mut self) {
        match self.output_mode {
            OutputMode::ConflictProtocol => {
                self.output_mode = OutputMode::SessionOutput;
                self.reset_output_view();
                self.set_operator_note("showing session output".to_string());
            }
            _ => {
                if self.selected_conflict_protocol.is_some() {
                    self.output_mode = OutputMode::ConflictProtocol;
                    self.selected_pane = Pane::Output;
                    self.output_follow = false;
                    self.output_scroll_offset = 0;
                    self.set_operator_note("showing worktree conflict protocol".to_string());
                } else {
                    self.set_operator_note(
                        "no conflicted worktree for selected session".to_string(),
                    );
                }
            }
        }
    }

    pub async fn assign_selected(&mut self) {
        let Some(source_session) = self.sessions.get(self.selected_session) else {
            return;
        };

        let task = self.new_session_task();
        let agent = self.cfg.default_agent.clone();

        let outcome = match manager::assign_session(
            &self.db,
            &self.cfg,
            &source_session.id,
            &task,
            &agent,
            self.cfg.auto_create_worktrees,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                tracing::warn!(
                    "Failed to assign follow-up work from session {}: {error}",
                    source_session.id
                );
                self.set_operator_note(format!("assignment failed: {error}"));
                return;
            }
        };

        self.refresh();
        self.sync_selection_by_id(Some(&outcome.session_id));
        self.set_operator_note(format!(
            "assigned via {} -> {}",
            assignment_action_label(outcome.action),
            format_session_id(&outcome.session_id)
        ));
        self.reset_output_view();
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();
    }

    pub async fn rebalance_selected_team(&mut self) {
        let Some(source_session) = self.sessions.get(self.selected_session) else {
            return;
        };

        let agent = self.cfg.default_agent.clone();
        let source_session_id = source_session.id.clone();
        let outcomes = match manager::rebalance_team_backlog(
            &self.db,
            &self.cfg,
            &source_session_id,
            &agent,
            self.cfg.auto_create_worktrees,
            self.cfg.auto_dispatch_limit_per_session,
        )
        .await
        {
            Ok(outcomes) => outcomes,
            Err(error) => {
                tracing::warn!(
                    "Failed to rebalance team backlog for session {}: {error}",
                    source_session_id
                );
                self.set_operator_note(format!(
                    "rebalance failed for {}: {error}",
                    format_session_id(&source_session_id)
                ));
                return;
            }
        };

        self.refresh();
        self.sync_selection_by_id(Some(&source_session_id));
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();

        if outcomes.is_empty() {
            self.set_operator_note(format!(
                "no delegate backlog needed rebalancing for {}",
                format_session_id(&source_session_id)
            ));
        } else {
            self.set_operator_note(format!(
                "rebalanced {} delegate handoff(s) for {}",
                outcomes.len(),
                format_session_id(&source_session_id)
            ));
        }
    }

    pub async fn drain_inbox_selected(&mut self) {
        let Some(source_session) = self.sessions.get(self.selected_session) else {
            return;
        };

        let agent = self.cfg.default_agent.clone();
        let source_session_id = source_session.id.clone();

        let outcomes = match manager::drain_inbox(
            &self.db,
            &self.cfg,
            &source_session_id,
            &agent,
            self.cfg.auto_create_worktrees,
            self.cfg.max_parallel_sessions,
        )
        .await
        {
            Ok(outcomes) => outcomes,
            Err(error) => {
                tracing::warn!(
                    "Failed to drain inbox for session {}: {error}",
                    source_session_id
                );
                self.set_operator_note(format!(
                    "drain inbox failed for {}: {error}",
                    format_session_id(&source_session_id)
                ));
                return;
            }
        };

        self.refresh();
        self.sync_selection_by_id(Some(&source_session_id));
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();

        if outcomes.is_empty() {
            self.set_operator_note(format!(
                "no unread handoffs for {}",
                format_session_id(&source_session_id)
            ));
        } else {
            self.set_operator_note(format!(
                "drained {} handoff(s) from {}",
                outcomes.len(),
                format_session_id(&source_session_id)
            ));
        }
    }

    pub async fn auto_dispatch_backlog(&mut self) {
        let agent = self.cfg.default_agent.clone();
        let lead_limit = self.sessions.len().max(1);

        let outcomes = match manager::auto_dispatch_backlog(
            &self.db,
            &self.cfg,
            &agent,
            self.cfg.auto_create_worktrees,
            lead_limit,
        )
        .await
        {
            Ok(outcomes) => outcomes,
            Err(error) => {
                tracing::warn!("Failed to auto-dispatch backlog from dashboard: {error}");
                self.set_operator_note(format!("global auto-dispatch failed: {error}"));
                return;
            }
        };

        let total_processed: usize = outcomes.iter().map(|outcome| outcome.routed.len()).sum();
        let total_routed: usize = outcomes
            .iter()
            .map(|outcome| {
                outcome
                    .routed
                    .iter()
                    .filter(|item| manager::assignment_action_routes_work(item.action))
                    .count()
            })
            .sum();
        let total_deferred = total_processed.saturating_sub(total_routed);
        let selected_session_id = self
            .sessions
            .get(self.selected_session)
            .map(|session| session.id.clone());

        self.refresh();
        self.sync_selection_by_id(selected_session_id.as_deref());
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();

        if total_processed == 0 {
            self.set_operator_note("no unread handoff backlog found".to_string());
        } else {
            self.set_operator_note(format!(
                "auto-dispatch processed {} handoff(s) across {} lead session(s) ({} routed, {} deferred)",
                total_processed,
                outcomes.len(),
                total_routed,
                total_deferred
            ));
        }
    }

    pub async fn rebalance_all_teams(&mut self) {
        let agent = self.cfg.default_agent.clone();
        let lead_limit = self.sessions.len().max(1);

        let outcomes = match manager::rebalance_all_teams(
            &self.db,
            &self.cfg,
            &agent,
            self.cfg.auto_create_worktrees,
            lead_limit,
        )
        .await
        {
            Ok(outcomes) => outcomes,
            Err(error) => {
                tracing::warn!("Failed to rebalance teams from dashboard: {error}");
                self.set_operator_note(format!("global rebalance failed: {error}"));
                return;
            }
        };

        let total_rerouted: usize = outcomes.iter().map(|outcome| outcome.rerouted.len()).sum();
        let selected_session_id = self
            .sessions
            .get(self.selected_session)
            .map(|session| session.id.clone());

        self.refresh();
        self.sync_selection_by_id(selected_session_id.as_deref());
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();

        if total_rerouted == 0 {
            self.set_operator_note("no delegate backlog needed global rebalancing".to_string());
        } else {
            self.set_operator_note(format!(
                "rebalanced {} handoff(s) across {} lead session(s)",
                total_rerouted,
                outcomes.len()
            ));
        }
    }

    pub async fn coordinate_backlog(&mut self) {
        let agent = self.cfg.default_agent.clone();
        let lead_limit = self.sessions.len().max(1);

        let outcome = match manager::coordinate_backlog(
            &self.db,
            &self.cfg,
            &agent,
            self.cfg.auto_create_worktrees,
            lead_limit,
        )
        .await
        {
            Ok(outcomes) => outcomes,
            Err(error) => {
                tracing::warn!("Failed to coordinate backlog from dashboard: {error}");
                self.set_operator_note(format!("global coordinate failed: {error}"));
                return;
            }
        };
        let total_processed: usize = outcome
            .dispatched
            .iter()
            .map(|dispatch| dispatch.routed.len())
            .sum();
        let total_routed: usize = outcome
            .dispatched
            .iter()
            .map(|dispatch| {
                dispatch
                    .routed
                    .iter()
                    .filter(|item| manager::assignment_action_routes_work(item.action))
                    .count()
            })
            .sum();
        let total_deferred = total_processed.saturating_sub(total_routed);
        let total_rerouted: usize = outcome
            .rebalanced
            .iter()
            .map(|rebalance| rebalance.rerouted.len())
            .sum();

        let selected_session_id = self
            .sessions
            .get(self.selected_session)
            .map(|session| session.id.clone());

        self.refresh();
        self.sync_selection_by_id(selected_session_id.as_deref());
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();

        if total_processed == 0 && total_rerouted == 0 && outcome.remaining_backlog_sessions == 0 {
            self.set_operator_note("backlog already clear".to_string());
        } else {
            self.set_operator_note(format!(
                "coordinated backlog: processed {} across {} lead(s) ({} routed, {} deferred), rebalanced {} across {} lead(s), remaining {} across {} session(s) [{} absorbable, {} saturated]",
                total_processed,
                outcome.dispatched.len(),
                total_routed,
                total_deferred,
                total_rerouted,
                outcome.rebalanced.len(),
                outcome.remaining_backlog_messages,
                outcome.remaining_backlog_sessions,
                outcome.remaining_absorbable_sessions,
                outcome.remaining_saturated_sessions
            ));
        }
    }

    pub async fn stop_selected(&mut self) {
        let Some(session) = self.sessions.get(self.selected_session) else {
            return;
        };

        let session_id = session.id.clone();
        if let Err(error) = manager::stop_session(&self.db, &session_id).await {
            tracing::warn!("Failed to stop session {}: {error}", session.id);
            self.set_operator_note(format!(
                "stop failed for {}: {error}",
                format_session_id(&session_id)
            ));
            return;
        }

        self.refresh();
        self.set_operator_note(format!(
            "stopped session {}",
            format_session_id(&session_id)
        ));
    }

    pub async fn resume_selected(&mut self) {
        let Some(session) = self.sessions.get(self.selected_session) else {
            return;
        };

        let session_id = session.id.clone();
        if let Err(error) = manager::resume_session(&self.db, &self.cfg, &session_id).await {
            tracing::warn!("Failed to resume session {}: {error}", session.id);
            self.set_operator_note(format!(
                "resume failed for {}: {error}",
                format_session_id(&session_id)
            ));
            return;
        }

        self.refresh();
        self.set_operator_note(format!(
            "resumed session {}",
            format_session_id(&session_id)
        ));
    }

    pub async fn cleanup_selected_worktree(&mut self) {
        let Some(session) = self.sessions.get(self.selected_session) else {
            return;
        };

        if session.worktree.is_none() {
            return;
        }

        let session_id = session.id.clone();
        if let Err(error) = manager::cleanup_session_worktree(&self.db, &session_id).await {
            tracing::warn!("Failed to cleanup session {} worktree: {error}", session.id);
            self.set_operator_note(format!(
                "cleanup failed for {}: {error}",
                format_session_id(&session_id)
            ));
            return;
        }

        self.refresh();
        self.set_operator_note(format!(
            "cleaned worktree for {}",
            format_session_id(&session_id)
        ));
    }

    pub async fn merge_selected_worktree(&mut self) {
        let Some(session) = self.sessions.get(self.selected_session) else {
            return;
        };

        if session.worktree.is_none() {
            self.set_operator_note("selected session has no worktree to merge".to_string());
            return;
        }

        let session_id = session.id.clone();
        let outcome = match manager::merge_session_worktree(&self.db, &session_id, true).await {
            Ok(outcome) => outcome,
            Err(error) => {
                tracing::warn!("Failed to merge session {} worktree: {error}", session.id);
                self.set_operator_note(format!(
                    "merge failed for {}: {error}",
                    format_session_id(&session_id)
                ));
                return;
            }
        };

        self.refresh();
        self.set_operator_note(format!(
            "merged {} into {} for {}{}",
            outcome.branch,
            outcome.base_branch,
            format_session_id(&session_id),
            if outcome.already_up_to_date {
                " (already up to date)"
            } else {
                ""
            }
        ));
    }

    pub async fn merge_ready_worktrees(&mut self) {
        match manager::merge_ready_worktrees(&self.db, true).await {
            Ok(outcome) => {
                self.refresh();
                if outcome.merged.is_empty()
                    && outcome.rebased.is_empty()
                    && outcome.active_with_worktree_ids.is_empty()
                    && outcome.conflicted_session_ids.is_empty()
                    && outcome.dirty_worktree_ids.is_empty()
                    && outcome.blocked_by_queue_session_ids.is_empty()
                    && outcome.failures.is_empty()
                {
                    self.set_operator_note("no ready worktrees to merge".to_string());
                    return;
                }

                let mut parts = vec![format!("merged {} ready worktree(s)", outcome.merged.len())];
                if !outcome.rebased.is_empty() {
                    parts.push(format!("rebased {}", outcome.rebased.len()));
                }
                if !outcome.active_with_worktree_ids.is_empty() {
                    parts.push(format!(
                        "skipped {} active",
                        outcome.active_with_worktree_ids.len()
                    ));
                }
                if !outcome.conflicted_session_ids.is_empty() {
                    parts.push(format!(
                        "skipped {} conflicted",
                        outcome.conflicted_session_ids.len()
                    ));
                }
                if !outcome.dirty_worktree_ids.is_empty() {
                    parts.push(format!(
                        "skipped {} dirty",
                        outcome.dirty_worktree_ids.len()
                    ));
                }
                if !outcome.blocked_by_queue_session_ids.is_empty() {
                    parts.push(format!(
                        "blocked {} in queue",
                        outcome.blocked_by_queue_session_ids.len()
                    ));
                }
                if !outcome.failures.is_empty() {
                    parts.push(format!("{} failed", outcome.failures.len()));
                }
                self.set_operator_note(parts.join("; "));
            }
            Err(error) => {
                tracing::warn!("Failed to merge ready worktrees: {error}");
                self.set_operator_note(format!("merge ready worktrees failed: {error}"));
            }
        }
    }

    pub async fn prune_inactive_worktrees(&mut self) {
        match manager::prune_inactive_worktrees(&self.db, &self.cfg).await {
            Ok(outcome) => {
                self.refresh();
                if outcome.cleaned_session_ids.is_empty() && outcome.retained_session_ids.is_empty()
                {
                    self.set_operator_note("no inactive worktrees to prune".to_string());
                } else if outcome.cleaned_session_ids.is_empty() {
                    self.set_operator_note(format!(
                        "deferred {} inactive worktree(s) within retention",
                        outcome.retained_session_ids.len()
                    ));
                } else if outcome.active_with_worktree_ids.is_empty() {
                    if outcome.retained_session_ids.is_empty() {
                        self.set_operator_note(format!(
                            "pruned {} inactive worktree(s)",
                            outcome.cleaned_session_ids.len()
                        ));
                    } else {
                        self.set_operator_note(format!(
                            "pruned {} inactive worktree(s); deferred {} within retention",
                            outcome.cleaned_session_ids.len(),
                            outcome.retained_session_ids.len()
                        ));
                    }
                } else {
                    let mut note = format!(
                        "pruned {} inactive worktree(s); skipped {} active session(s)",
                        outcome.cleaned_session_ids.len(),
                        outcome.active_with_worktree_ids.len()
                    );
                    if !outcome.retained_session_ids.is_empty() {
                        note.push_str(&format!(
                            "; deferred {} within retention",
                            outcome.retained_session_ids.len()
                        ));
                    }
                    self.set_operator_note(note);
                }
            }
            Err(error) => {
                tracing::warn!("Failed to prune inactive worktrees: {error}");
                self.set_operator_note(format!("prune inactive worktrees failed: {error}"));
            }
        }
    }

    pub async fn delete_selected_session(&mut self) {
        let Some(session) = self.sessions.get(self.selected_session) else {
            return;
        };

        let session_id = session.id.clone();
        if let Err(error) = manager::delete_session(&self.db, &session_id).await {
            tracing::warn!("Failed to delete session {}: {error}", session.id);
            self.set_operator_note(format!(
                "delete failed for {}: {error}",
                format_session_id(&session_id)
            ));
            return;
        }

        self.refresh();
        self.set_operator_note(format!(
            "deleted session {}",
            format_session_id(&session_id)
        ));
    }

    pub fn refresh(&mut self) {
        self.sync_from_store();
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn is_input_mode(&self) -> bool {
        self.spawn_input.is_some()
            || self.search_input.is_some()
            || self.commit_input.is_some()
            || self.pr_input.is_some()
    }

    pub fn has_active_search(&self) -> bool {
        self.search_query.is_some()
    }

    pub fn is_context_graph_mode(&self) -> bool {
        self.output_mode == OutputMode::ContextGraph
    }

    pub fn has_active_completion_popup(&self) -> bool {
        self.active_completion_popup.is_some()
    }

    pub fn dismiss_completion_popup(&mut self) {
        if self.active_completion_popup.take().is_some() {
            self.active_completion_popup = self.queued_completion_popups.pop_front();
        }
    }

    pub fn begin_spawn_prompt(&mut self) {
        if self.search_input.is_some() {
            self.set_operator_note(
                "finish output search input before opening spawn prompt".to_string(),
            );
            return;
        }

        self.spawn_input = Some(self.spawn_prompt_seed());
        self.set_operator_note(
            "spawn mode | try: give me 3 agents working on fix flaky tests | or: template feature_development for fix flaky tests".to_string(),
        );
    }

    pub fn toggle_search_scope(&mut self) {
        if self.output_mode == OutputMode::Timeline {
            self.timeline_scope = self.timeline_scope.next();
            self.sync_output_scroll(self.last_output_height.max(1));
            self.set_operator_note(format!(
                "timeline scope set to {}",
                self.timeline_scope.label()
            ));
            return;
        }

        if self.output_mode == OutputMode::ContextGraph {
            self.search_scope = self.search_scope.next();
            self.recompute_search_matches();
            self.sync_output_scroll(self.last_output_height.max(1));

            if self.search_query.is_some() {
                self.set_operator_note(format!(
                    "graph scope set to {} | {} match(es)",
                    self.search_scope.label(),
                    self.search_matches.len()
                ));
            } else {
                self.set_operator_note(format!("graph scope set to {}", self.search_scope.label()));
            }
            return;
        }

        if self.output_mode != OutputMode::SessionOutput {
            self.set_operator_note(
                "scope toggle is only available in session output, graph, or timeline view"
                    .to_string(),
            );
            return;
        }

        self.search_scope = self.search_scope.next();
        self.recompute_search_matches();
        self.sync_output_scroll(self.last_output_height.max(1));

        if self.search_query.is_some() {
            self.set_operator_note(format!(
                "search scope set to {} | {} match(es)",
                self.search_scope.label(),
                self.search_matches.len()
            ));
        } else {
            self.set_operator_note(format!("search scope set to {}", self.search_scope.label()));
        }
    }

    pub fn toggle_search_agent_filter(&mut self) {
        if self.output_mode != OutputMode::SessionOutput {
            self.set_operator_note(
                "search agent filter is only available in session output view".to_string(),
            );
            return;
        }

        let Some(selected_agent_type) = self.selected_agent_type().map(str::to_owned) else {
            self.set_operator_note("search agent filter requires a selected session".to_string());
            return;
        };

        self.search_agent_filter = match self.search_agent_filter {
            SearchAgentFilter::AllAgents => SearchAgentFilter::SelectedAgentType,
            SearchAgentFilter::SelectedAgentType => SearchAgentFilter::AllAgents,
        };
        self.recompute_search_matches();
        self.sync_output_scroll(self.last_output_height.max(1));

        if self.search_query.is_some() {
            self.set_operator_note(format!(
                "search agent filter set to {} | {} match(es)",
                self.search_agent_filter.label(&selected_agent_type),
                self.search_matches.len()
            ));
        } else {
            self.set_operator_note(format!(
                "search agent filter set to {}",
                self.search_agent_filter.label(&selected_agent_type)
            ));
        }
    }

    pub fn begin_search(&mut self) {
        if self.spawn_input.is_some() {
            self.set_operator_note("finish spawn prompt before searching output".to_string());
            return;
        }

        if !matches!(
            self.output_mode,
            OutputMode::SessionOutput | OutputMode::ContextGraph
        ) {
            self.set_operator_note(
                "search is only available in session output or graph view".to_string(),
            );
            return;
        }

        self.search_input = Some(self.search_query.clone().unwrap_or_default());
        let mode = if self.output_mode == OutputMode::ContextGraph {
            "graph search"
        } else {
            "search"
        };
        self.set_operator_note(format!("{mode} mode | type a query and press Enter"));
    }

    pub fn push_input_char(&mut self, ch: char) {
        if let Some(input) = self.spawn_input.as_mut() {
            input.push(ch);
        } else if let Some(input) = self.search_input.as_mut() {
            input.push(ch);
        } else if let Some(input) = self.commit_input.as_mut() {
            input.push(ch);
        } else if let Some(input) = self.pr_input.as_mut() {
            input.push(ch);
        }
    }

    pub fn pop_input_char(&mut self) {
        if let Some(input) = self.spawn_input.as_mut() {
            input.pop();
        } else if let Some(input) = self.search_input.as_mut() {
            input.pop();
        } else if let Some(input) = self.commit_input.as_mut() {
            input.pop();
        } else if let Some(input) = self.pr_input.as_mut() {
            input.pop();
        }
    }

    pub fn cancel_input(&mut self) {
        if self.spawn_input.take().is_some() {
            self.set_operator_note("spawn input cancelled".to_string());
        } else if self.search_input.take().is_some() {
            self.set_operator_note("search input cancelled".to_string());
        } else if self.commit_input.take().is_some() {
            self.set_operator_note("commit input cancelled".to_string());
        } else if self.pr_input.take().is_some() {
            self.set_operator_note("pr input cancelled".to_string());
        }
    }

    pub async fn submit_input(&mut self) {
        if self.spawn_input.is_some() {
            self.submit_spawn_prompt().await;
        } else if self.commit_input.is_some() {
            self.submit_commit_prompt();
        } else if self.pr_input.is_some() {
            self.submit_pr_prompt();
        } else {
            self.submit_search();
        }
    }

    fn submit_pr_prompt(&mut self) {
        let Some(input) = self.pr_input.take() else {
            return;
        };

        let request = match parse_pr_prompt(&input) {
            Ok(request) => request,
            Err(error) => {
                self.pr_input = Some(input);
                self.set_operator_note(format!("invalid PR input: {error}"));
                return;
            }
        };

        if request.title.is_empty() {
            self.pr_input = Some(input);
            self.set_operator_note("pr title cannot be empty".to_string());
            return;
        }

        let Some(session) = self.sessions.get(self.selected_session).cloned() else {
            self.set_operator_note("no session selected".to_string());
            return;
        };
        let Some(worktree) = session.worktree.clone() else {
            self.set_operator_note("selected session has no worktree".to_string());
            return;
        };
        if let Ok(true) = worktree::has_uncommitted_changes(&worktree) {
            self.pr_input = Some(input);
            self.set_operator_note(
                "commit or reset worktree changes before creating a PR".to_string(),
            );
            return;
        }

        let body = self.build_pull_request_body(&session);
        let options = worktree::DraftPrOptions {
            base_branch: request.base_branch.clone(),
            labels: request.labels.clone(),
            reviewers: request.reviewers.clone(),
        };
        match worktree::create_draft_pr_with_options(&worktree, &request.title, &body, &options) {
            Ok(url) => {
                self.set_operator_note(format!(
                    "created draft PR for {} against {}: {}",
                    format_session_id(&session.id),
                    options
                        .base_branch
                        .as_deref()
                        .unwrap_or(&worktree.base_branch),
                    url
                ));
            }
            Err(error) => {
                self.pr_input = Some(input);
                self.set_operator_note(format!("draft PR failed: {error}"));
            }
        }
    }

    fn submit_commit_prompt(&mut self) {
        let Some(input) = self.commit_input.take() else {
            return;
        };

        let message = input.trim().to_string();
        let Some(session_id) = self.selected_session_id().map(ToOwned::to_owned) else {
            self.set_operator_note("no session selected".to_string());
            return;
        };
        let Some(worktree) = self
            .sessions
            .get(self.selected_session)
            .and_then(|session| session.worktree.clone())
        else {
            self.set_operator_note("selected session has no worktree".to_string());
            return;
        };

        match worktree::commit_staged(&worktree, &message) {
            Ok(hash) => {
                self.refresh_after_git_status_action(None);
                self.set_operator_note(format!(
                    "committed {} as {}",
                    format_session_id(&session_id),
                    hash
                ));
            }
            Err(error) => {
                self.commit_input = Some(input);
                self.set_operator_note(format!("commit failed: {error}"));
            }
        }
    }

    fn submit_search(&mut self) {
        let Some(input) = self.search_input.take() else {
            return;
        };

        let query = input.trim().to_string();
        if query.is_empty() {
            self.clear_search();
            return;
        }

        if let Err(error) = compile_search_regex(&query) {
            self.search_input = Some(query.clone());
            self.set_operator_note(format!("invalid regex /{query}: {error}"));
            return;
        }

        self.search_query = Some(query.clone());
        self.recompute_search_matches();
        if self.search_matches.is_empty() {
            let mode = if self.output_mode == OutputMode::ContextGraph {
                "graph search"
            } else {
                "search"
            };
            self.set_operator_note(format!("{mode} /{query} found no matches"));
        } else {
            let mode = if self.output_mode == OutputMode::ContextGraph {
                "graph search"
            } else {
                "search"
            };
            self.set_operator_note(format!(
                "{mode} /{query} matched {} line(s) across {} session(s) | n/N navigate matches",
                self.search_matches.len(),
                self.search_match_session_count()
            ));
        }
    }

    fn build_pull_request_body(&self, session: &Session) -> String {
        let mut lines = vec![
            "## Summary".to_string(),
            format!("- Task: {}", session.task),
            format!("- Agent: {}", session.agent_type),
            format!("- Project: {}", session.project),
            format!("- Task group: {}", session.task_group),
        ];
        if let Some(worktree) = session.worktree.as_ref() {
            lines.push(format!(
                "- Branch: {} -> {}",
                worktree.branch, worktree.base_branch
            ));
        }
        if let Some(summary) = self.selected_diff_summary.as_ref() {
            lines.push(format!("- Diff: {summary}"));
        }
        let changed_files = self
            .selected_diff_preview
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>();
        if !changed_files.is_empty() {
            lines.push(String::new());
            lines.push("## Changed Files".to_string());
            for file in changed_files {
                lines.push(format!("- {file}"));
            }
        }
        lines.push(String::new());
        lines.push("## Session Metrics".to_string());
        lines.push(format!(
            "- Tokens: {} total (in {} / out {})",
            session.metrics.tokens_used,
            session.metrics.input_tokens,
            session.metrics.output_tokens
        ));
        lines.push(format!("- Tool calls: {}", session.metrics.tool_calls));
        lines.push(format!(
            "- Files changed: {}",
            session.metrics.files_changed
        ));
        lines.push(format!(
            "- Duration: {}",
            format_duration(session.metrics.duration_secs)
        ));
        lines.push(String::new());
        lines.push("## Testing".to_string());
        lines.push("- Verified in ECC 2.0 dashboard workflow".to_string());
        lines.join("\n")
    }

    async fn submit_spawn_prompt(&mut self) {
        let Some(input) = self.spawn_input.take() else {
            return;
        };

        let plan = match self.build_spawn_plan(&input) {
            Ok(plan) => plan,
            Err(error) => {
                self.spawn_input = Some(input);
                self.set_operator_note(error);
                return;
            }
        };

        let source_session = self.sessions.get(self.selected_session).cloned();
        let handoff_context = source_session.as_ref().map(|session| {
            format!(
                "Dashboard handoff from {} [{}] | cwd {}{}",
                format_session_id(&session.id),
                session.agent_type,
                session.working_dir.display(),
                session
                    .worktree
                    .as_ref()
                    .map(|worktree| format!(
                        " | worktree {} ({})",
                        worktree.branch,
                        worktree.path.display()
                    ))
                    .unwrap_or_default()
            )
        });
        let source_task = source_session.as_ref().map(|session| session.task.clone());
        let source_session_id = source_session.as_ref().map(|session| session.id.clone());
        let source_grouping = source_session
            .as_ref()
            .map(|session| SessionGrouping {
                project: Some(session.project.clone()),
                task_group: Some(session.task_group.clone()),
            })
            .unwrap_or_default();
        let agent = self.cfg.default_agent.clone();
        let mut created_ids = Vec::new();

        match &plan {
            SpawnPlan::AdHoc {
                requested_count: _,
                spawn_count,
                task,
            } => {
                for task in expand_spawn_tasks(task, *spawn_count) {
                    let session_id = match manager::create_session_with_grouping(
                        &self.db,
                        &self.cfg,
                        &task,
                        &agent,
                        self.cfg.auto_create_worktrees,
                        source_grouping.clone(),
                    )
                    .await
                    {
                        Ok(session_id) => session_id,
                        Err(error) => {
                            let preferred_selection =
                                post_spawn_selection_id(source_session_id.as_deref(), &created_ids);
                            self.refresh_after_spawn(preferred_selection.as_deref());
                            let mut summary = if created_ids.is_empty() {
                                format!("spawn failed: {error}")
                            } else {
                                format!(
                                    "spawn partially completed: {} of {} queued before failure: {error}",
                                    created_ids.len(),
                                    spawn_count
                                )
                            };
                            if let Some(layout_note) =
                                self.auto_split_layout_after_spawn(created_ids.len())
                            {
                                summary.push_str(" | ");
                                summary.push_str(&layout_note);
                            }
                            self.set_operator_note(summary);
                            return;
                        }
                    };

                    if let (Some(source_id), Some(task), Some(context)) = (
                        source_session_id.as_ref(),
                        source_task.as_ref(),
                        handoff_context.as_ref(),
                    ) {
                        if let Err(error) = comms::send(
                            &self.db,
                            source_id,
                            &session_id,
                            &comms::MessageType::TaskHandoff {
                                task: task.clone(),
                                context: context.clone(),
                                priority: comms::TaskPriority::Normal,
                            },
                        ) {
                            tracing::warn!(
                                "Failed to send handoff from session {} to {}: {error}",
                                source_id,
                                session_id
                            );
                        }
                    }

                    created_ids.push(session_id);
                }
            }
            SpawnPlan::Template {
                name,
                task,
                variables,
                ..
            } => match manager::launch_orchestration_template(
                &self.db,
                &self.cfg,
                name,
                source_session_id.as_deref(),
                task.as_deref(),
                variables.clone(),
            )
            .await
            {
                Ok(outcome) => {
                    created_ids.extend(outcome.created.into_iter().map(|step| step.session_id));
                }
                Err(error) => {
                    self.set_operator_note(format!("template launch failed: {error}"));
                    return;
                }
            },
        }

        let preferred_selection =
            post_spawn_selection_id(source_session_id.as_deref(), &created_ids);
        self.refresh_after_spawn(preferred_selection.as_deref());
        let queued_count = created_ids
            .iter()
            .filter(|session_id| {
                self.db
                    .pending_worktree_queue_contains(session_id)
                    .unwrap_or(false)
            })
            .count();
        let mut note = build_spawn_note(&plan, created_ids.len(), queued_count);
        if let Some(layout_note) = self.auto_split_layout_after_spawn(created_ids.len()) {
            note.push_str(" | ");
            note.push_str(&layout_note);
        }
        self.set_operator_note(note);
    }

    pub fn clear_search(&mut self) {
        let had_query = self.search_query.take().is_some();
        let had_input = self.search_input.take().is_some();
        self.search_matches.clear();
        self.selected_search_match = 0;
        if had_query || had_input {
            let mode = if self.output_mode == OutputMode::ContextGraph {
                "graph search"
            } else {
                "output search"
            };
            self.set_operator_note(format!("cleared {mode}"));
        }
    }

    pub fn next_search_match(&mut self) {
        if self.search_matches.is_empty() {
            self.set_operator_note("no output search matches to navigate".to_string());
            return;
        }

        self.selected_search_match = (self.selected_search_match + 1) % self.search_matches.len();
        self.focus_selected_search_match();
        self.set_operator_note(self.search_navigation_note());
    }

    pub fn prev_search_match(&mut self) {
        if self.search_matches.is_empty() {
            self.set_operator_note("no output search matches to navigate".to_string());
            return;
        }

        self.selected_search_match = if self.selected_search_match == 0 {
            self.search_matches.len() - 1
        } else {
            self.selected_search_match - 1
        };
        self.focus_selected_search_match();
        self.set_operator_note(self.search_navigation_note());
    }

    pub fn toggle_output_filter(&mut self) {
        if self.output_mode != OutputMode::SessionOutput {
            self.set_operator_note(
                "output filters are only available in session output view".to_string(),
            );
            return;
        }

        self.output_filter = self.output_filter.next();
        self.recompute_search_matches();
        self.sync_output_scroll(self.last_output_height.max(1));
        self.set_operator_note(format!(
            "output filter set to {}",
            self.output_filter.label()
        ));
    }

    pub fn cycle_output_time_filter(&mut self) {
        if !matches!(
            self.output_mode,
            OutputMode::SessionOutput | OutputMode::Timeline | OutputMode::ContextGraph
        ) {
            self.set_operator_note(
                "time filters are only available in session output, graph, or timeline view"
                    .to_string(),
            );
            return;
        }

        self.output_time_filter = self.output_time_filter.next();
        if matches!(
            self.output_mode,
            OutputMode::SessionOutput | OutputMode::ContextGraph
        ) {
            self.recompute_search_matches();
        }
        self.sync_output_scroll(self.last_output_height.max(1));
        let note_prefix = match self.output_mode {
            OutputMode::Timeline => "timeline range",
            OutputMode::ContextGraph => "graph range",
            _ => "output time filter",
        };
        self.set_operator_note(format!(
            "{note_prefix} set to {}",
            self.output_time_filter.label()
        ));
    }

    pub fn cycle_timeline_event_filter(&mut self) {
        if self.output_mode != OutputMode::Timeline {
            self.set_operator_note(
                "timeline event filters are only available in timeline view".to_string(),
            );
            return;
        }

        self.timeline_event_filter = self.timeline_event_filter.next();
        self.sync_output_scroll(self.last_output_height.max(1));
        self.set_operator_note(format!(
            "timeline filter set to {}",
            self.timeline_event_filter.label()
        ));
    }

    pub fn toggle_context_graph_mode(&mut self) {
        match self.output_mode {
            OutputMode::ContextGraph => {
                self.output_mode = OutputMode::SessionOutput;
                self.reset_output_view();
                self.set_operator_note("showing session output".to_string());
            }
            _ => {
                self.output_mode = OutputMode::ContextGraph;
                self.selected_pane = Pane::Output;
                self.output_follow = false;
                self.output_scroll_offset = 0;
                self.recompute_search_matches();
                self.set_operator_note("showing selected session context graph".to_string());
            }
        }
    }

    pub fn cycle_graph_entity_filter(&mut self) {
        if self.output_mode != OutputMode::ContextGraph {
            self.set_operator_note(
                "graph entity filters are only available in context graph view".to_string(),
            );
            return;
        }

        self.graph_entity_filter = self.graph_entity_filter.next();
        self.recompute_search_matches();
        self.sync_output_scroll(self.last_output_height.max(1));
        self.set_operator_note(format!(
            "graph filter set to {}",
            self.graph_entity_filter.label()
        ));
    }

    pub fn toggle_auto_dispatch_policy(&mut self) {
        self.cfg.auto_dispatch_unread_handoffs = !self.cfg.auto_dispatch_unread_handoffs;
        match self.cfg.save() {
            Ok(()) => {
                let state = if self.cfg.auto_dispatch_unread_handoffs {
                    "enabled"
                } else {
                    "disabled"
                };
                self.set_operator_note(format!(
                    "daemon auto-dispatch {state} | saved to {}",
                    crate::config::Config::config_path().display()
                ));
            }
            Err(error) => {
                self.cfg.auto_dispatch_unread_handoffs = !self.cfg.auto_dispatch_unread_handoffs;
                self.set_operator_note(format!("failed to persist auto-dispatch policy: {error}"));
            }
        }
    }

    pub fn toggle_auto_merge_policy(&mut self) {
        self.cfg.auto_merge_ready_worktrees = !self.cfg.auto_merge_ready_worktrees;
        match self.cfg.save() {
            Ok(()) => {
                let state = if self.cfg.auto_merge_ready_worktrees {
                    "enabled"
                } else {
                    "disabled"
                };
                self.set_operator_note(format!(
                    "daemon auto-merge {state} | saved to {}",
                    crate::config::Config::config_path().display()
                ));
            }
            Err(error) => {
                self.cfg.auto_merge_ready_worktrees = !self.cfg.auto_merge_ready_worktrees;
                self.set_operator_note(format!("failed to persist auto-merge policy: {error}"));
            }
        }
    }

    pub fn toggle_auto_worktree_policy(&mut self) {
        self.cfg.auto_create_worktrees = !self.cfg.auto_create_worktrees;
        match self.cfg.save() {
            Ok(()) => {
                let state = if self.cfg.auto_create_worktrees {
                    "enabled"
                } else {
                    "disabled"
                };
                self.set_operator_note(format!(
                    "default worktree creation {state} | saved to {}",
                    crate::config::Config::config_path().display()
                ));
            }
            Err(error) => {
                self.cfg.auto_create_worktrees = !self.cfg.auto_create_worktrees;
                self.set_operator_note(format!(
                    "failed to persist worktree creation policy: {error}"
                ));
            }
        }
    }

    pub fn adjust_auto_dispatch_limit(&mut self, delta: isize) {
        let next =
            (self.cfg.auto_dispatch_limit_per_session as isize + delta).clamp(1, 50) as usize;
        if next == self.cfg.auto_dispatch_limit_per_session {
            self.set_operator_note(format!(
                "auto-dispatch limit unchanged at {} handoff(s) per lead",
                self.cfg.auto_dispatch_limit_per_session
            ));
            return;
        }

        let previous = self.cfg.auto_dispatch_limit_per_session;
        self.cfg.auto_dispatch_limit_per_session = next;
        match self.cfg.save() {
            Ok(()) => self.set_operator_note(format!(
                "auto-dispatch limit set to {} handoff(s) per lead | saved to {}",
                self.cfg.auto_dispatch_limit_per_session,
                crate::config::Config::config_path().display()
            )),
            Err(error) => {
                self.cfg.auto_dispatch_limit_per_session = previous;
                self.set_operator_note(format!("failed to persist auto-dispatch limit: {error}"));
            }
        }
    }

    pub async fn tick(&mut self) {
        loop {
            match self.output_rx.try_recv() {
                Ok(_event) => {}
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }

        if let Err(error) = manager::activate_pending_worktree_sessions(&self.db, &self.cfg).await {
            tracing::warn!("Failed to activate queued worktree sessions: {error}");
        }

        self.sync_from_store();
    }

    fn sync_runtime_metrics(
        &mut self,
    ) -> (
        Option<manager::HeartbeatEnforcementOutcome>,
        Option<manager::BudgetEnforcementOutcome>,
        Option<manager::ConflictEnforcementOutcome>,
    ) {
        if let Err(error) = self.db.refresh_session_durations() {
            tracing::warn!("Failed to refresh session durations: {error}");
        }

        let metrics_path = self.cfg.cost_metrics_path();
        let signature = metrics_file_signature(&metrics_path);
        if signature != self.last_cost_metrics_signature {
            self.last_cost_metrics_signature = signature;
            if signature.is_some() {
                if let Err(error) = self.db.sync_cost_tracker_metrics(&metrics_path) {
                    tracing::warn!("Failed to sync cost tracker metrics: {error}");
                }
            }
        }

        let activity_path = self.cfg.tool_activity_metrics_path();
        let activity_signature = metrics_file_signature(&activity_path);
        if activity_signature != self.last_tool_activity_signature {
            self.last_tool_activity_signature = activity_signature;
            if activity_signature.is_some() {
                if let Err(error) = self.db.sync_tool_activity_metrics(&activity_path) {
                    tracing::warn!("Failed to sync tool activity metrics: {error}");
                }
            }
        }

        let heartbeat_enforcement = match manager::enforce_session_heartbeats(&self.db, &self.cfg) {
            Ok(outcome) => Some(outcome),
            Err(error) => {
                tracing::warn!("Failed to enforce session heartbeats: {error}");
                None
            }
        };

        let budget_enforcement = match manager::enforce_budget_hard_limits(&self.db, &self.cfg) {
            Ok(outcome) => Some(outcome),
            Err(error) => {
                tracing::warn!("Failed to enforce budget hard limits: {error}");
                None
            }
        };

        let conflict_enforcement = match manager::enforce_conflict_resolution(&self.db, &self.cfg) {
            Ok(outcome) => Some(outcome),
            Err(error) => {
                tracing::warn!("Failed to enforce conflict resolution: {error}");
                None
            }
        };

        (
            heartbeat_enforcement,
            budget_enforcement,
            conflict_enforcement,
        )
    }

    fn sync_from_store(&mut self) {
        let (heartbeat_enforcement, budget_enforcement, conflict_enforcement) =
            self.sync_runtime_metrics();
        let selected_id = self.selected_session_id().map(ToOwned::to_owned);
        self.sessions = match self.db.list_sessions() {
            Ok(mut sessions) => {
                sort_sessions_for_display(&mut sessions);
                sessions
            }
            Err(error) => {
                tracing::warn!("Failed to refresh sessions: {error}");
                Vec::new()
            }
        };
        self.session_harnesses = load_session_harnesses(&self.db, &self.cfg, &self.sessions);
        self.unread_message_counts = match self.db.unread_message_counts() {
            Ok(counts) => counts,
            Err(error) => {
                tracing::warn!("Failed to refresh unread message counts: {error}");
                HashMap::new()
            }
        };
        self.sync_approval_queue();
        self.sync_handoff_backlog_counts();
        self.sync_board_meta();
        self.sync_worktree_health_by_session();
        self.sync_session_state_notifications();
        self.sync_approval_notifications();
        self.sync_global_handoff_backlog();
        self.sync_daemon_activity();
        self.sync_output_cache();
        self.sync_selection_by_id(selected_id.as_deref());
        self.ensure_selected_pane_visible();
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_git_status();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();
        self.sync_budget_alerts();

        if let Some(outcome) =
            budget_enforcement.filter(|outcome| !outcome.paused_sessions.is_empty())
        {
            self.set_operator_note(budget_auto_pause_note(&outcome));
        }
        if let Some(outcome) = conflict_enforcement.filter(|outcome| outcome.created_incidents > 0)
        {
            self.set_operator_note(conflict_enforcement_note(&outcome));
        }
        if let Some(outcome) = heartbeat_enforcement.filter(|outcome| {
            !outcome.stale_sessions.is_empty() || !outcome.auto_terminated_sessions.is_empty()
        }) {
            self.set_operator_note(heartbeat_enforcement_note(&outcome));
        }
    }

    fn sync_budget_alerts(&mut self) {
        let aggregate = self.aggregate_usage();
        let thresholds = self.cfg.effective_budget_alert_thresholds();
        let current_state = aggregate.overall_state;
        if current_state == self.last_budget_alert_state {
            return;
        }

        let previous_state = self.last_budget_alert_state;
        self.last_budget_alert_state = current_state;

        if current_state <= previous_state {
            return;
        }

        let Some(summary_suffix) = current_state.summary_suffix(thresholds) else {
            return;
        };

        let token_budget = if self.cfg.token_budget > 0 {
            format!(
                "{} / {}",
                format_token_count(aggregate.total_tokens),
                format_token_count(self.cfg.token_budget)
            )
        } else {
            format!("{} / no budget", format_token_count(aggregate.total_tokens))
        };
        let cost_budget = if self.cfg.cost_budget_usd > 0.0 {
            format!(
                "{} / {}",
                format_currency(aggregate.total_cost_usd),
                format_currency(self.cfg.cost_budget_usd)
            )
        } else {
            format!("{} / no budget", format_currency(aggregate.total_cost_usd))
        };

        self.set_operator_note(format!(
            "{summary_suffix} | tokens {token_budget} | cost {cost_budget}"
        ));
        self.notify_desktop(
            NotificationEvent::BudgetAlert,
            "ECC 2.0: Budget alert",
            &format!("{summary_suffix} | tokens {token_budget} | cost {cost_budget}"),
        );
        self.notify_webhook(
            NotificationEvent::BudgetAlert,
            &budget_alert_webhook_body(
                &summary_suffix,
                &token_budget,
                &cost_budget,
                self.active_session_count(),
            ),
        );
    }

    fn sync_session_state_notifications(&mut self) {
        let mut next_states = HashMap::new();
        let mut completion_summaries = Vec::new();
        let mut failed_notifications = Vec::new();
        let mut started_webhooks = Vec::new();
        let mut completion_webhooks = Vec::new();
        let mut failed_webhooks = Vec::new();

        for session in &self.sessions {
            let previous_state = self.last_session_states.get(&session.id);
            if let Some(previous_state) = previous_state {
                if previous_state != &session.state {
                    match session.state {
                        SessionState::Running => {
                            started_webhooks.push(session_started_webhook_body(
                                session,
                                session_compare_url(session).as_deref(),
                            ));
                        }
                        SessionState::Completed => {
                            let summary = self.build_completion_summary(session);
                            self.persist_completion_summary_observation(
                                session,
                                &summary,
                                "completion_summary",
                            );
                            if self.cfg.completion_summary_notifications.enabled {
                                completion_summaries.push(summary.clone());
                            } else if self.cfg.desktop_notifications.session_completed {
                                self.notify_desktop(
                                    NotificationEvent::SessionCompleted,
                                    "ECC 2.0: Session completed",
                                    &format!(
                                        "{} | {}",
                                        format_session_id(&session.id),
                                        truncate_for_dashboard(&session.task, 96)
                                    ),
                                );
                            }
                            completion_webhooks.push(completion_summary_webhook_body(
                                &summary,
                                session,
                                session_compare_url(session).as_deref(),
                            ));
                        }
                        SessionState::Failed => {
                            let summary = self.build_completion_summary(session);
                            self.persist_completion_summary_observation(
                                session,
                                &summary,
                                "failure_summary",
                            );
                            failed_notifications.push((
                                "ECC 2.0: Session failed".to_string(),
                                format!(
                                    "{} | {}",
                                    format_session_id(&session.id),
                                    truncate_for_dashboard(&session.task, 96)
                                ),
                            ));
                            failed_webhooks.push(completion_summary_webhook_body(
                                &summary,
                                session,
                                session_compare_url(session).as_deref(),
                            ));
                        }
                        _ => {}
                    }
                }
            } else if session.state == SessionState::Running {
                started_webhooks.push(session_started_webhook_body(
                    session,
                    session_compare_url(session).as_deref(),
                ));
            }

            next_states.insert(session.id.clone(), session.state.clone());
        }

        for summary in completion_summaries {
            self.deliver_completion_summary(summary);
        }

        for body in started_webhooks {
            self.notify_webhook(NotificationEvent::SessionStarted, &body);
        }

        if self.cfg.desktop_notifications.session_failed {
            for (title, body) in failed_notifications {
                self.notify_desktop(NotificationEvent::SessionFailed, &title, &body);
            }
        }

        for body in completion_webhooks {
            self.notify_webhook(NotificationEvent::SessionCompleted, &body);
        }

        for body in failed_webhooks {
            self.notify_webhook(NotificationEvent::SessionFailed, &body);
        }

        self.last_session_states = next_states;
    }

    fn persist_completion_summary_observation(
        &self,
        session: &Session,
        summary: &SessionCompletionSummary,
        observation_type: &str,
    ) {
        let observation_summary = format!(
            "{} | files {} | tests {}/{} | warnings {}",
            truncate_for_dashboard(&summary.task, 72),
            summary.files_changed,
            summary.tests_passed,
            summary.tests_run,
            summary.warnings.len()
        );
        let details = completion_summary_observation_details(summary, session);
        let priority = if observation_type == "failure_summary" {
            ContextObservationPriority::High
        } else {
            ContextObservationPriority::Normal
        };
        if let Err(error) = self.db.add_session_observation(
            &session.id,
            observation_type,
            priority,
            false,
            &observation_summary,
            &details,
        ) {
            tracing::warn!(
                "Failed to persist completion observation for {}: {error}",
                session.id
            );
        }
    }

    fn sync_approval_notifications(&mut self) {
        let latest_message = match self.db.latest_unread_approval_message() {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!("Failed to refresh latest approval request: {error}");
                return;
            }
        };

        let Some(message) = latest_message else {
            return;
        };

        if self
            .last_seen_approval_message_id
            .is_some_and(|last_seen| message.id <= last_seen)
        {
            return;
        }

        self.last_seen_approval_message_id = Some(message.id);
        let preview =
            truncate_for_dashboard(&comms::preview(&message.msg_type, &message.content), 96);
        self.notify_desktop(
            NotificationEvent::ApprovalRequest,
            "ECC 2.0: Approval needed",
            &format!(
                "{} from {} | {}",
                format_session_id(&message.to_session),
                format_session_id(&message.from_session),
                preview
            ),
        );
        self.notify_webhook(
            NotificationEvent::ApprovalRequest,
            &approval_request_webhook_body(&message, &preview),
        );
    }

    fn deliver_completion_summary(&mut self, summary: SessionCompletionSummary) {
        if self.cfg.completion_summary_notifications.desktop_enabled()
            && self.cfg.desktop_notifications.session_completed
        {
            self.notify_desktop(
                NotificationEvent::SessionCompleted,
                &summary.title(),
                &summary.notification_body(),
            );
        }

        if self.cfg.completion_summary_notifications.popup_enabled() {
            if self.active_completion_popup.is_none() {
                self.active_completion_popup = Some(summary);
            } else {
                self.queued_completion_popups.push_back(summary);
            }
        }
    }

    fn build_completion_summary(&self, session: &Session) -> SessionCompletionSummary {
        let file_activity = match self.db.list_file_activity(&session.id, 5) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(
                    "Failed to load file activity for completion summary {}: {error}",
                    session.id
                );
                Vec::new()
            }
        };
        let tool_logs = match self.db.list_tool_logs_for_session(&session.id) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(
                    "Failed to load tool logs for completion summary {}: {error}",
                    session.id
                );
                Vec::new()
            }
        };
        let overlaps = match self.db.list_file_overlaps(&session.id, 3) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(
                    "Failed to load file overlaps for completion summary {}: {error}",
                    session.id
                );
                Vec::new()
            }
        };

        let tests = summarize_test_runs(&tool_logs, session.state == SessionState::Completed);
        let recent_files = recent_completion_files(&file_activity, session.metrics.files_changed);
        let key_decisions =
            summarize_completion_decisions(&tool_logs, &file_activity, &session.task);
        let warnings = summarize_completion_warnings(
            session,
            &tool_logs,
            &tests,
            self.worktree_health_by_session.get(&session.id),
            self.approval_queue_counts
                .get(&session.id)
                .copied()
                .unwrap_or(0),
            overlaps.len(),
        );

        SessionCompletionSummary {
            session_id: session.id.clone(),
            task: session.task.clone(),
            state: session.state.clone(),
            files_changed: session.metrics.files_changed,
            tokens_used: session.metrics.tokens_used,
            duration_secs: session.metrics.duration_secs,
            cost_usd: session.metrics.cost_usd,
            tests_run: tests.total,
            tests_passed: tests.passed,
            recent_files,
            key_decisions,
            warnings,
        }
    }

    fn notify_desktop(&self, event: NotificationEvent, title: &str, body: &str) {
        let _ = self.notifier.notify(event, title, body);
    }

    fn notify_webhook(&self, event: NotificationEvent, body: &str) {
        let _ = self.webhook_notifier.notify(event, body);
    }

    fn sync_selection(&mut self) {
        if self.sessions.is_empty() {
            self.selected_session = 0;
            self.session_table_state.select(None);
        } else {
            self.selected_session = self.selected_session.min(self.sessions.len() - 1);
            self.session_table_state.select(Some(self.selected_session));
        }
    }

    fn sync_selection_by_id(&mut self, selected_id: Option<&str>) {
        if let Some(selected_id) = selected_id {
            if let Some(index) = self
                .sessions
                .iter()
                .position(|session| session.id == selected_id)
            {
                self.selected_session = index;
            }
        }
        self.sync_selection();
    }

    fn sync_output_cache(&mut self) {
        let active_session_ids: HashSet<_> = self
            .sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect();
        self.session_output_cache
            .retain(|session_id, _| active_session_ids.contains(session_id.as_str()));

        for session in &self.sessions {
            match self.db.get_output_lines(&session.id, OUTPUT_BUFFER_LIMIT) {
                Ok(lines) => {
                    self.output_store.replace_lines(&session.id, lines.clone());
                    self.session_output_cache.insert(session.id.clone(), lines);
                }
                Err(error) => {
                    tracing::warn!("Failed to load session output for {}: {error}", session.id);
                }
            }
        }
    }

    fn ensure_selected_pane_visible(&mut self) {
        if !self.is_pane_visible(self.selected_pane) {
            self.selected_pane = Pane::Sessions;
        }
    }

    fn focus_pane(&mut self, pane: Pane) {
        self.selected_pane = pane;
        self.ensure_selected_pane_visible();
        self.set_operator_note(format!("focused {} pane", pane.title().to_lowercase()));
    }

    fn move_pane_focus(&mut self, direction: PaneDirection) {
        let visible_panes = self.visible_panes();
        if visible_panes.len() <= 1 {
            return;
        }

        let pane_areas = self.pane_areas(Rect::new(0, 0, 100, 40));
        let Some(current_rect) = pane_rect(&pane_areas, self.selected_pane) else {
            return;
        };
        let current_center = pane_center(current_rect);

        let candidate = visible_panes
            .into_iter()
            .filter(|pane| *pane != self.selected_pane)
            .filter_map(|pane| {
                let rect = pane_rect(&pane_areas, pane)?;
                let center = pane_center(rect);
                let dx = center.0 - current_center.0;
                let dy = center.1 - current_center.1;

                let (primary, secondary) = match direction {
                    PaneDirection::Left if dx < 0 => ((-dx) as u16, dy.unsigned_abs()),
                    PaneDirection::Right if dx > 0 => (dx as u16, dy.unsigned_abs()),
                    PaneDirection::Up if dy < 0 => ((-dy) as u16, dx.unsigned_abs()),
                    PaneDirection::Down if dy > 0 => (dy as u16, dx.unsigned_abs()),
                    _ => return None,
                };

                Some((pane, primary, secondary))
            })
            .min_by_key(|(pane, primary, secondary)| (*primary, *secondary, pane.sort_key()));

        if let Some((pane, _, _)) = candidate {
            self.focus_pane(pane);
        }
    }

    fn pane_focus_shortcuts_label(&self) -> String {
        self.cfg.pane_navigation.focus_shortcuts_label()
    }

    fn pane_move_shortcuts_label(&self) -> String {
        self.cfg.pane_navigation.movement_shortcuts_label()
    }

    fn sync_global_handoff_backlog(&mut self) {
        let limit = self.sessions.len().max(1);
        match self.db.unread_task_handoff_targets(limit) {
            Ok(targets) => {
                self.global_handoff_backlog_leads = targets.len();
                self.global_handoff_backlog_messages =
                    targets.iter().map(|(_, unread_count)| *unread_count).sum();
            }
            Err(error) => {
                tracing::warn!("Failed to refresh global handoff backlog: {error}");
                self.global_handoff_backlog_leads = 0;
                self.global_handoff_backlog_messages = 0;
            }
        }
    }

    fn sync_approval_queue(&mut self) {
        self.approval_queue_counts = match self.db.unread_approval_counts() {
            Ok(counts) => counts,
            Err(error) => {
                tracing::warn!("Failed to refresh approval queue counts: {error}");
                HashMap::new()
            }
        };
        self.approval_queue_preview = match self.db.unread_approval_queue(3) {
            Ok(messages) => messages,
            Err(error) => {
                tracing::warn!("Failed to refresh approval queue preview: {error}");
                Vec::new()
            }
        };
    }

    fn sync_handoff_backlog_counts(&mut self) {
        let limit = self.sessions.len().max(1);
        self.handoff_backlog_counts.clear();
        match self.db.unread_task_handoff_targets(limit) {
            Ok(targets) => {
                self.handoff_backlog_counts.extend(targets);
            }
            Err(error) => {
                tracing::warn!("Failed to refresh handoff backlog counts: {error}");
            }
        }
    }

    fn sync_board_meta(&mut self) {
        self.board_meta_by_session = match self.db.list_session_board_meta() {
            Ok(meta) => meta,
            Err(error) => {
                tracing::warn!("Failed to refresh board metadata: {error}");
                HashMap::new()
            }
        };
    }

    fn sync_worktree_health_by_session(&mut self) {
        self.worktree_health_by_session.clear();
        for session in &self.sessions {
            let Some(worktree) = session.worktree.as_ref() else {
                continue;
            };

            match worktree::health(worktree) {
                Ok(health) => {
                    self.worktree_health_by_session
                        .insert(session.id.clone(), health);
                }
                Err(error) => {
                    tracing::warn!(
                        "Failed to refresh worktree health for {}: {error}",
                        session.id
                    );
                }
            }
        }
    }

    fn sync_daemon_activity(&mut self) {
        self.daemon_activity = match self.db.daemon_activity() {
            Ok(activity) => activity,
            Err(error) => {
                tracing::warn!("Failed to refresh daemon activity: {error}");
                DaemonActivity::default()
            }
        };
    }

    fn sync_selected_output(&mut self) {
        if self.selected_session_id().is_none() {
            self.output_scroll_offset = 0;
            self.output_follow = true;
            self.search_matches.clear();
            self.selected_search_match = 0;
            return;
        }

        self.recompute_search_matches();
    }

    fn sync_selected_diff(&mut self) {
        let session = self.sessions.get(self.selected_session);
        let worktree = session.and_then(|session| session.worktree.as_ref());

        self.selected_diff_summary =
            worktree.and_then(|worktree| worktree::diff_summary(worktree).ok().flatten());
        self.selected_diff_preview = worktree
            .and_then(|worktree| worktree::diff_file_preview(worktree, MAX_DIFF_PREVIEW_LINES).ok())
            .unwrap_or_default();
        self.selected_diff_patch = worktree.and_then(|worktree| {
            worktree::diff_patch_preview(worktree, MAX_DIFF_PATCH_LINES)
                .ok()
                .flatten()
        });
        self.selected_diff_hunk_offsets_unified = self
            .selected_diff_patch
            .as_deref()
            .map(build_unified_diff_hunk_offsets)
            .unwrap_or_default();
        self.selected_diff_hunk_offsets_split = self
            .selected_diff_patch
            .as_deref()
            .map(|patch| build_worktree_diff_columns(patch, self.theme_palette()).hunk_offsets)
            .unwrap_or_default();
        if self.selected_diff_hunk >= self.current_diff_hunk_offsets().len() {
            self.selected_diff_hunk = 0;
        }
        self.selected_merge_readiness =
            worktree.and_then(|worktree| worktree::merge_readiness(worktree).ok());
        self.selected_conflict_protocol = session.and_then(|selected_session| {
            worktree
                .zip(self.selected_merge_readiness.as_ref())
                .and_then(|(worktree, merge_readiness)| {
                    build_conflict_protocol(&selected_session.id, worktree, merge_readiness)
                })
                .or_else(|| {
                    let incidents = self
                        .db
                        .list_open_conflict_incidents_for_session(&selected_session.id, 5)
                        .unwrap_or_default();
                    build_session_conflict_protocol(&selected_session.id, &incidents)
                })
        });
        if self.output_mode == OutputMode::WorktreeDiff && self.selected_diff_patch.is_none() {
            self.output_mode = OutputMode::SessionOutput;
        }
        if self.output_mode == OutputMode::ConflictProtocol
            && self.selected_conflict_protocol.is_none()
        {
            self.output_mode = OutputMode::SessionOutput;
        }
        self.sync_selected_git_status();
        self.sync_selected_git_patch();
    }

    fn sync_selected_git_status(&mut self) {
        let session = self.sessions.get(self.selected_session);
        let worktree = session.and_then(|session| session.worktree.as_ref());
        self.selected_git_status_entries = worktree
            .and_then(|worktree| worktree::git_status_entries(worktree).ok())
            .unwrap_or_default();
        if self.selected_git_status >= self.selected_git_status_entries.len() {
            self.selected_git_status = self.selected_git_status_entries.len().saturating_sub(1);
        }
        if matches!(
            self.output_mode,
            OutputMode::GitStatus | OutputMode::GitPatch
        ) && worktree.is_none()
        {
            self.output_mode = OutputMode::SessionOutput;
        }
    }

    fn sync_selected_git_patch(&mut self) {
        let Some((entry, worktree)) = self.selected_git_status_context() else {
            self.selected_git_patch = None;
            self.selected_git_patch_hunk_offsets_unified.clear();
            self.selected_git_patch_hunk_offsets_split.clear();
            self.selected_git_patch_hunk = 0;
            if self.output_mode == OutputMode::GitPatch {
                self.output_mode = OutputMode::GitStatus;
            }
            return;
        };

        self.selected_git_patch = worktree::git_status_patch_view(&worktree, &entry)
            .ok()
            .flatten();
        self.selected_git_patch_hunk_offsets_unified = self
            .selected_git_patch
            .as_ref()
            .map(|patch| build_unified_diff_hunk_offsets(&patch.patch))
            .unwrap_or_default();
        self.selected_git_patch_hunk_offsets_split = self
            .selected_git_patch
            .as_ref()
            .map(|patch| {
                build_worktree_diff_columns(&patch.patch, self.theme_palette()).hunk_offsets
            })
            .unwrap_or_default();
        if self.selected_git_patch_hunk >= self.current_diff_hunk_offsets().len() {
            self.selected_git_patch_hunk = 0;
        }
        if self.output_mode == OutputMode::GitPatch && self.selected_git_patch.is_none() {
            self.output_mode = OutputMode::GitStatus;
        }
    }

    fn selected_git_status_context(
        &self,
    ) -> Option<(worktree::GitStatusEntry, crate::session::WorktreeInfo)> {
        let session = self.sessions.get(self.selected_session)?;
        let worktree = session.worktree.clone()?;
        let entry = self
            .selected_git_status_entries
            .get(self.selected_git_status)
            .cloned()?;
        Some((entry, worktree))
    }

    fn selected_git_patch_context(
        &self,
    ) -> Option<(
        worktree::GitStatusEntry,
        crate::session::WorktreeInfo,
        worktree::GitStatusPatchView,
        worktree::GitPatchHunk,
    )> {
        let (entry, worktree) = self.selected_git_status_context()?;
        let patch = self.selected_git_patch.clone()?;
        let hunk = patch.hunks.get(self.selected_git_patch_hunk).cloned()?;
        Some((entry, worktree, patch, hunk))
    }

    fn refresh_after_git_status_action(&mut self, preferred_path: Option<&str>) {
        let keep_patch_view = self.output_mode == OutputMode::GitPatch;
        let preferred_hunk = self.selected_git_patch_hunk;
        self.refresh();
        self.selected_pane = Pane::Output;
        self.output_follow = false;
        if let Some(path) = preferred_path {
            if let Some(index) = self
                .selected_git_status_entries
                .iter()
                .position(|entry| entry.path == path)
            {
                self.selected_git_status = index;
            }
        }
        self.sync_selected_git_patch();
        if keep_patch_view && self.selected_git_patch.is_some() {
            self.output_mode = OutputMode::GitPatch;
            let max_index = self.current_diff_hunk_offsets().len().saturating_sub(1);
            self.selected_git_patch_hunk = preferred_hunk.min(max_index);
            self.output_scroll_offset = self.current_diff_hunk_offset();
        } else {
            self.output_mode = OutputMode::GitStatus;
        }
        self.sync_output_scroll(self.last_output_height.max(1));
    }

    fn active_patch_text(&self) -> Option<&String> {
        match self.output_mode {
            OutputMode::GitPatch => self.selected_git_patch.as_ref().map(|patch| &patch.patch),
            OutputMode::WorktreeDiff => self.selected_diff_patch.as_ref(),
            _ => None,
        }
    }

    fn current_diff_hunk_offsets(&self) -> &[usize] {
        match self.output_mode {
            OutputMode::GitPatch => match self.diff_view_mode {
                DiffViewMode::Split => &self.selected_git_patch_hunk_offsets_split,
                DiffViewMode::Unified => &self.selected_git_patch_hunk_offsets_unified,
            },
            _ => match self.diff_view_mode {
                DiffViewMode::Split => &self.selected_diff_hunk_offsets_split,
                DiffViewMode::Unified => &self.selected_diff_hunk_offsets_unified,
            },
        }
    }

    fn current_diff_hunk_index(&self) -> usize {
        match self.output_mode {
            OutputMode::GitPatch => self.selected_git_patch_hunk,
            _ => self.selected_diff_hunk,
        }
    }

    fn set_current_diff_hunk_index(&mut self, index: usize) {
        match self.output_mode {
            OutputMode::GitPatch => self.selected_git_patch_hunk = index,
            _ => self.selected_diff_hunk = index,
        }
    }

    fn current_diff_hunk_offset(&self) -> usize {
        self.current_diff_hunk_offsets()
            .get(self.current_diff_hunk_index())
            .copied()
            .unwrap_or(0)
    }

    fn diff_hunk_title_suffix(&self) -> String {
        let total = self.current_diff_hunk_offsets().len();
        if total == 0 {
            String::new()
        } else {
            format!(" {}/{}", self.current_diff_hunk_index() + 1, total)
        }
    }

    fn sync_selected_messages(&mut self) {
        let Some(session_id) = self.selected_session_id().map(ToOwned::to_owned) else {
            self.selected_messages.clear();
            self.sync_approval_queue();
            return;
        };

        let unread_count = self
            .unread_message_counts
            .get(&session_id)
            .copied()
            .unwrap_or(0);
        if unread_count > 0 {
            match self.db.mark_messages_read(&session_id) {
                Ok(_) => {
                    self.unread_message_counts.insert(session_id.clone(), 0);
                }
                Err(error) => {
                    tracing::warn!(
                        "Failed to mark session {} messages as read: {error}",
                        session_id
                    );
                }
            }
        }

        self.selected_messages = match self.db.list_messages_for_session(&session_id, 5) {
            Ok(messages) => messages,
            Err(error) => {
                tracing::warn!("Failed to load session messages: {error}");
                Vec::new()
            }
        };

        self.sync_approval_queue();
    }

    fn sync_selected_lineage(&mut self) {
        let Some(session_id) = self.selected_session_id().map(ToOwned::to_owned) else {
            self.selected_parent_session = None;
            self.selected_child_sessions.clear();
            self.focused_delegate_session_id = None;
            self.selected_team_summary = None;
            self.selected_route_preview = None;
            return;
        };

        self.selected_parent_session = match self.db.latest_task_handoff_source(&session_id) {
            Ok(parent) => parent,
            Err(error) => {
                tracing::warn!("Failed to load session parent linkage: {error}");
                None
            }
        };

        self.selected_child_sessions = match self.db.delegated_children(&session_id, 50) {
            Ok(children) => {
                let mut delegated = Vec::new();
                let mut team = TeamSummary::default();
                let mut route_candidates = Vec::new();

                for child_id in children {
                    match self.db.get_session(&child_id) {
                        Ok(Some(session)) => {
                            team.total += 1;
                            let approval_backlog = self
                                .approval_queue_counts
                                .get(&child_id)
                                .copied()
                                .unwrap_or(0);
                            let handoff_backlog = match self.db.unread_task_handoff_count(&child_id)
                            {
                                Ok(count) => count,
                                Err(error) => {
                                    tracing::warn!(
                                        "Failed to load delegated child handoff backlog {}: {error}",
                                        child_id
                                    );
                                    0
                                }
                            };
                            let state = session.state.clone();
                            match state {
                                SessionState::Idle => team.idle += 1,
                                SessionState::Running => team.running += 1,
                                SessionState::Pending => team.pending += 1,
                                SessionState::Failed => team.failed += 1,
                                SessionState::Stopped => team.stopped += 1,
                                SessionState::Stale => team.stale += 1,
                                SessionState::Completed => {}
                            }

                            route_candidates.push(DelegatedChildSummary {
                                worktree_health: self
                                    .worktree_health_by_session
                                    .get(&child_id)
                                    .copied(),
                                approval_backlog,
                                handoff_backlog,
                                state: state.clone(),
                                session_id: child_id.clone(),
                                tokens_used: session.metrics.tokens_used,
                                files_changed: session.metrics.files_changed,
                                duration_secs: session.metrics.duration_secs,
                                task_preview: truncate_for_dashboard(&session.task, 40),
                                branch: session
                                    .worktree
                                    .as_ref()
                                    .map(|worktree| worktree.branch.clone()),
                                last_output_preview: self
                                    .db
                                    .get_output_lines(&child_id, 1)
                                    .ok()
                                    .and_then(|lines| lines.last().cloned())
                                    .map(|line| truncate_for_dashboard(&line.text, 48)),
                            });
                            delegated.push(DelegatedChildSummary {
                                worktree_health: self
                                    .worktree_health_by_session
                                    .get(&session.id)
                                    .copied(),
                                approval_backlog,
                                handoff_backlog,
                                state,
                                session_id: child_id,
                                tokens_used: session.metrics.tokens_used,
                                files_changed: session.metrics.files_changed,
                                duration_secs: session.metrics.duration_secs,
                                task_preview: truncate_for_dashboard(&session.task, 40),
                                branch: session
                                    .worktree
                                    .as_ref()
                                    .map(|worktree| worktree.branch.clone()),
                                last_output_preview: self
                                    .db
                                    .get_output_lines(&session.id, 1)
                                    .ok()
                                    .and_then(|lines| lines.last().cloned())
                                    .map(|line| truncate_for_dashboard(&line.text, 48)),
                            });
                        }
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(
                                "Failed to load delegated child session {}: {error}",
                                child_id
                            );
                        }
                    }
                }

                self.selected_team_summary = if team.total > 0 { Some(team) } else { None };
                let selected_agent_type = self
                    .selected_agent_type()
                    .unwrap_or(self.cfg.default_agent.as_str())
                    .to_string();
                self.selected_route_preview = self.build_route_preview(
                    &session_id,
                    &selected_agent_type,
                    team.total,
                    &route_candidates,
                );
                delegated.sort_by_key(|delegate| {
                    (
                        delegate_attention_priority(delegate),
                        std::cmp::Reverse(delegate.approval_backlog),
                        std::cmp::Reverse(delegate.handoff_backlog),
                        delegate.session_id.clone(),
                    )
                });
                delegated
            }
            Err(error) => {
                tracing::warn!("Failed to load delegated child sessions: {error}");
                self.selected_team_summary = None;
                self.selected_route_preview = None;
                Vec::new()
            }
        };
        self.sync_focused_delegate_selection();
    }

    fn build_route_preview(
        &self,
        lead_id: &str,
        lead_agent_type: &str,
        delegate_count: usize,
        delegates: &[DelegatedChildSummary],
    ) -> Option<String> {
        if let Some(task) = self.latest_route_task(lead_id) {
            if let Ok(preview) = manager::preview_assignment_for_task(
                &self.db,
                &self.cfg,
                lead_id,
                &task,
                lead_agent_type,
            ) {
                return Some(self.format_assignment_preview(&task, &preview));
            }
        }

        if let Some(idle_clear) = delegates
            .iter()
            .filter(|delegate| {
                delegate.state == SessionState::Idle && delegate.handoff_backlog == 0
            })
            .min_by_key(|delegate| delegate.session_id.as_str())
        {
            return Some(format!(
                "reuse idle {}",
                format_session_id(&idle_clear.session_id)
            ));
        }

        if delegate_count < self.cfg.max_parallel_sessions {
            return Some("spawn new delegate".to_string());
        }

        if let Some(idle_backed_up) = delegates
            .iter()
            .filter(|delegate| delegate.state == SessionState::Idle)
            .min_by_key(|delegate| (delegate.handoff_backlog, delegate.session_id.as_str()))
        {
            return Some(format!(
                "defer; idle {} backlog {}",
                format_session_id(&idle_backed_up.session_id),
                idle_backed_up.handoff_backlog
            ));
        }

        if let Some(active_delegate) = delegates
            .iter()
            .filter(|delegate| {
                matches!(
                    delegate.state,
                    SessionState::Running | SessionState::Pending
                )
            })
            .min_by_key(|delegate| (delegate.handoff_backlog, delegate.session_id.as_str()))
        {
            return Some(format!(
                "{} active {}{}",
                if active_delegate.handoff_backlog > 0 {
                    "defer;"
                } else {
                    "reuse"
                },
                format_session_id(&active_delegate.session_id),
                if active_delegate.handoff_backlog > 0 {
                    format!(" backlog {}", active_delegate.handoff_backlog)
                } else {
                    String::new()
                }
            ));
        }

        if delegate_count == 0 {
            Some("spawn new delegate".to_string())
        } else {
            Some("spawn fallback delegate".to_string())
        }
    }

    fn latest_route_task(&self, session_id: &str) -> Option<String> {
        self.db
            .list_messages_for_session(session_id, 16)
            .ok()?
            .into_iter()
            .rev()
            .find_map(|message| {
                if message.to_session != session_id || message.msg_type != "task_handoff" {
                    return None;
                }
                manager::parse_task_handoff_task(&message.content).or(Some(message.content))
            })
    }

    fn format_assignment_preview(
        &self,
        task: &str,
        preview: &manager::AssignmentPreview,
    ) -> String {
        let task_preview = truncate_for_dashboard(task, 40);
        let graph_suffix = if preview.graph_match_terms.is_empty() {
            String::new()
        } else {
            format!(
                " | graph {}",
                truncate_for_dashboard(&preview.graph_match_terms.join(", "), 36)
            )
        };

        match preview.action {
            manager::AssignmentAction::Spawned => {
                format!("for `{task_preview}` spawn new delegate")
            }
            manager::AssignmentAction::ReusedIdle => format!(
                "for `{task_preview}` reuse idle {}{}",
                preview
                    .session_id
                    .as_deref()
                    .map(format_session_id)
                    .unwrap_or_else(|| "unknown".to_string()),
                graph_suffix
            ),
            manager::AssignmentAction::ReusedActive => format!(
                "for `{task_preview}` reuse active {}{}",
                preview
                    .session_id
                    .as_deref()
                    .map(format_session_id)
                    .unwrap_or_else(|| "unknown".to_string()),
                graph_suffix
            ),
            manager::AssignmentAction::DeferredSaturated => {
                let state_label = match preview.delegate_state {
                    Some(SessionState::Idle) => "idle",
                    Some(SessionState::Running) | Some(SessionState::Pending) => "active",
                    _ => "delegate",
                };
                format!(
                    "for `{task_preview}` defer; {state_label} {} backlog {}{}",
                    preview
                        .session_id
                        .as_deref()
                        .map(format_session_id)
                        .unwrap_or_else(|| "unknown".to_string()),
                    preview.handoff_backlog,
                    graph_suffix
                )
            }
        }
    }

    fn selected_session_id(&self) -> Option<&str> {
        self.sessions
            .get(self.selected_session)
            .map(|session| session.id.as_str())
    }

    fn selected_output_lines(&self) -> &[OutputLine] {
        self.selected_session_id()
            .and_then(|session_id| self.session_output_cache.get(session_id))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn selected_agent_type(&self) -> Option<&str> {
        self.sessions
            .get(self.selected_session)
            .map(|session| session.agent_type.as_str())
    }

    fn search_agent_filter_label(&self) -> String {
        self.search_agent_filter
            .label(self.selected_agent_type().unwrap_or("selected agent"))
            .to_string()
    }

    fn search_agent_title_suffix(&self) -> String {
        match self.selected_agent_type() {
            Some(agent_type) => self
                .search_agent_filter
                .title_suffix(agent_type)
                .to_string(),
            None => String::new(),
        }
    }

    fn visible_output_lines_for_session(&self, session_id: &str) -> Vec<&OutputLine> {
        self.session_output_cache
            .get(session_id)
            .map(|lines| {
                lines
                    .iter()
                    .filter(|line| {
                        self.output_filter.matches(line) && self.output_time_filter.matches(line)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn visible_output_lines(&self) -> Vec<&OutputLine> {
        self.selected_session_id()
            .map(|session_id| self.visible_output_lines_for_session(session_id))
            .unwrap_or_default()
    }

    fn visible_graph_lines(&self) -> Vec<GraphDisplayLine> {
        let session_scope = match self.search_scope {
            SearchScope::SelectedSession => self.selected_session_id(),
            SearchScope::AllSessions => None,
        };
        let entity_type = self.graph_entity_filter.entity_type();
        let entities = self
            .db
            .list_context_entities(session_scope, entity_type, 48)
            .unwrap_or_default();
        let show_session_label = self.search_scope == SearchScope::AllSessions;

        entities
            .into_iter()
            .filter(|entity| self.output_time_filter.matches_timestamp(entity.updated_at))
            .flat_map(|entity| self.graph_lines_for_entity(entity, show_session_label))
            .collect()
    }

    fn graph_lines_for_entity(
        &self,
        entity: crate::session::ContextGraphEntity,
        show_session_label: bool,
    ) -> Vec<GraphDisplayLine> {
        let session_id = entity.session_id.clone().unwrap_or_default();
        let session_label = if show_session_label {
            if session_id.is_empty() {
                "global ".to_string()
            } else {
                format!("{} ", format_session_id(&session_id))
            }
        } else {
            String::new()
        };
        let entity_title = format!(
            "[{}] {}{:<8} {}",
            entity.updated_at.format("%H:%M:%S"),
            session_label,
            entity.entity_type,
            entity.name
        );
        let mut lines = vec![GraphDisplayLine {
            session_id: session_id.clone(),
            text: entity_title,
        }];

        if let Some(path) = entity.path.as_ref() {
            lines.push(GraphDisplayLine {
                session_id: session_id.clone(),
                text: format!("               path {}", truncate_for_dashboard(path, 96)),
            });
        }

        if !entity.summary.trim().is_empty() {
            lines.push(GraphDisplayLine {
                session_id: session_id.clone(),
                text: format!(
                    "               summary {}",
                    truncate_for_dashboard(&entity.summary, 96)
                ),
            });
        }

        if let Ok(Some(detail)) = self.db.get_context_entity_detail(entity.id, 2) {
            for relation in detail.outgoing {
                lines.push(GraphDisplayLine {
                    session_id: session_id.clone(),
                    text: format!(
                        "               -> {} {}:{}",
                        relation.relation_type,
                        relation.to_entity_type,
                        truncate_for_dashboard(&relation.to_entity_name, 72)
                    ),
                });
            }
            for relation in detail.incoming {
                lines.push(GraphDisplayLine {
                    session_id: session_id.clone(),
                    text: format!(
                        "               <- {} {}:{}",
                        relation.relation_type,
                        relation.from_entity_type,
                        truncate_for_dashboard(&relation.from_entity_name, 72)
                    ),
                });
            }
        }

        lines
    }

    fn session_graph_metrics_lines(&self, session_id: &str) -> Vec<String> {
        let entity = self
            .db
            .list_context_entities(Some(session_id), Some("session"), 4)
            .unwrap_or_default()
            .into_iter()
            .find(|entity| {
                entity.session_id.as_deref() == Some(session_id) || entity.name == session_id
            });
        let Some(entity) = entity else {
            return Vec::new();
        };

        let Ok(Some(detail)) = self
            .db
            .get_context_entity_detail(entity.id, MAX_METRICS_GRAPH_RELATIONS)
        else {
            return Vec::new();
        };

        if detail.outgoing.is_empty() && detail.incoming.is_empty() {
            return Vec::new();
        }

        let mut lines = vec![
            "Context graph".to_string(),
            format!(
                "- outgoing {} | incoming {}",
                detail.outgoing.len(),
                detail.incoming.len()
            ),
        ];

        for relation in detail.outgoing.iter().take(4) {
            lines.push(format!(
                "- -> {} {}:{}",
                relation.relation_type,
                relation.to_entity_type,
                truncate_for_dashboard(&relation.to_entity_name, 72)
            ));
        }

        for relation in detail.incoming.iter().take(2) {
            lines.push(format!(
                "- <- {} {}:{}",
                relation.relation_type,
                relation.from_entity_type,
                truncate_for_dashboard(&relation.from_entity_name, 72)
            ));
        }

        lines
    }

    fn session_graph_recall_lines(&self, session: &Session) -> Vec<String> {
        let query = session.task.trim();
        if query.is_empty() {
            return Vec::new();
        }

        let Ok(entries) = self.db.recall_context_entities(None, query, 4) else {
            return Vec::new();
        };

        let entries = entries
            .into_iter()
            .filter(|entry| {
                !(entry.entity.entity_type == "session" && entry.entity.name == session.id)
            })
            .take(3)
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return Vec::new();
        }

        let mut lines = vec!["Relevant memory".to_string()];
        for entry in entries {
            let mut line = format!(
                "- #{} [{}] {} | score {} | relations {} | observations {} | priority {}",
                entry.entity.id,
                entry.entity.entity_type,
                truncate_for_dashboard(&entry.entity.name, 60),
                entry.score,
                entry.relation_count,
                entry.observation_count,
                entry.max_observation_priority
            );
            if entry.has_pinned_observation {
                line.push_str(" | pinned");
            }
            if let Some(session_id) = entry.entity.session_id.as_deref() {
                if session_id != session.id {
                    line.push_str(&format!(" | {}", format_session_id(session_id)));
                }
            }
            lines.push(line);
            if !entry.matched_terms.is_empty() {
                lines.push(format!("  matches {}", entry.matched_terms.join(", ")));
            }
            if let Some(path) = entry.entity.path.as_deref() {
                lines.push(format!("  path {}", truncate_for_dashboard(path, 72)));
            }
            if !entry.entity.summary.is_empty() {
                lines.push(format!(
                    "  summary {}",
                    truncate_for_dashboard(&entry.entity.summary, 72)
                ));
            }
            if let Ok(observations) = self.db.list_context_observations(Some(entry.entity.id), 1) {
                if let Some(observation) = observations.first() {
                    lines.push(format!(
                        "  memory [{}{}] {}",
                        observation.priority,
                        if observation.pinned { "/pinned" } else { "" },
                        truncate_for_dashboard(&observation.summary, 72)
                    ));
                }
            }
        }

        lines
    }

    fn visible_git_status_lines(&self) -> Vec<Line<'static>> {
        self.selected_git_status_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let marker = if index == self.selected_git_status {
                    ">>"
                } else {
                    "-"
                };
                let mut flags = Vec::new();
                if entry.conflicted {
                    flags.push("conflict");
                }
                if entry.staged {
                    flags.push("staged");
                }
                if entry.unstaged {
                    flags.push("unstaged");
                }
                if entry.untracked {
                    flags.push("untracked");
                }
                let flag_text = if flags.is_empty() {
                    "clean".to_string()
                } else {
                    flags.join(",")
                };
                Line::from(format!(
                    "{} [{}{}] [{}] {}",
                    marker,
                    entry.index_status,
                    entry.worktree_status,
                    flag_text,
                    entry.display_path
                ))
            })
            .collect()
    }

    fn visible_timeline_lines(&self) -> Vec<Line<'static>> {
        let show_session_label = self.timeline_scope == SearchScope::AllSessions;
        self.timeline_events()
            .into_iter()
            .filter(|event| self.timeline_event_filter.matches(event.event_type))
            .filter(|event| self.output_time_filter.matches_timestamp(event.occurred_at))
            .flat_map(|event| {
                let prefix = if show_session_label {
                    format!("{} ", format_session_id(&event.session_id))
                } else {
                    String::new()
                };
                let mut lines = vec![Line::from(format!(
                    "[{}] {}{:<11} {}",
                    event.occurred_at.format("%H:%M:%S"),
                    prefix,
                    event.event_type.label(),
                    event.summary
                ))];
                lines.extend(
                    event
                        .detail_lines
                        .into_iter()
                        .map(|line| Line::from(format!("               {}", line))),
                );
                lines
            })
            .collect()
    }

    fn timeline_events(&self) -> Vec<TimelineEvent> {
        let mut events = match self.timeline_scope {
            SearchScope::SelectedSession => self
                .sessions
                .get(self.selected_session)
                .map(|session| self.session_timeline_events(session))
                .unwrap_or_default(),
            SearchScope::AllSessions => self
                .sessions
                .iter()
                .flat_map(|session| self.session_timeline_events(session))
                .collect(),
        };
        events.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.session_id.cmp(&right.session_id))
                .then_with(|| left.summary.cmp(&right.summary))
        });
        events
    }

    fn session_timeline_events(&self, session: &Session) -> Vec<TimelineEvent> {
        let mut events = vec![TimelineEvent {
            occurred_at: session.created_at,
            session_id: session.id.clone(),
            event_type: TimelineEventType::Lifecycle,
            summary: format!(
                "created session as {} for {}",
                session.agent_type,
                truncate_for_dashboard(&session.task, 64)
            ),
            detail_lines: Vec::new(),
        }];

        if session.updated_at > session.created_at {
            events.push(TimelineEvent {
                occurred_at: session.updated_at,
                session_id: session.id.clone(),
                event_type: TimelineEventType::Lifecycle,
                summary: format!("state {} | updated session metadata", session.state),
                detail_lines: Vec::new(),
            });
        }

        if let Some(worktree) = session.worktree.as_ref() {
            events.push(TimelineEvent {
                occurred_at: session.updated_at,
                session_id: session.id.clone(),
                event_type: TimelineEventType::Lifecycle,
                summary: format!(
                    "attached worktree {} from {}",
                    worktree.branch, worktree.base_branch
                ),
                detail_lines: Vec::new(),
            });
        }

        let file_activity = self
            .db
            .list_file_activity(&session.id, 64)
            .unwrap_or_default();
        if file_activity.is_empty() && session.metrics.files_changed > 0 {
            events.push(TimelineEvent {
                occurred_at: session.updated_at,
                session_id: session.id.clone(),
                event_type: TimelineEventType::FileChange,
                summary: format!("files touched {}", session.metrics.files_changed),
                detail_lines: Vec::new(),
            });
        } else {
            events.extend(file_activity.into_iter().map(|entry| TimelineEvent {
                occurred_at: entry.timestamp,
                session_id: session.id.clone(),
                event_type: TimelineEventType::FileChange,
                summary: file_activity_summary(&entry),
                detail_lines: file_activity_patch_lines(&entry, MAX_FILE_ACTIVITY_PATCH_LINES),
            }));
        }

        let messages = self
            .db
            .list_messages_for_session(&session.id, 128)
            .unwrap_or_default();
        events.extend(messages.into_iter().map(|message| {
            let (direction, counterpart) = if message.from_session == session.id {
                ("sent", format_session_id(&message.to_session))
            } else {
                ("received", format_session_id(&message.from_session))
            };
            TimelineEvent {
                occurred_at: message.timestamp,
                session_id: session.id.clone(),
                event_type: TimelineEventType::Message,
                summary: format!(
                    "{direction} {} {} | {}",
                    message.msg_type,
                    counterpart,
                    truncate_for_dashboard(
                        &comms::preview(&message.msg_type, &message.content),
                        64
                    )
                ),
                detail_lines: Vec::new(),
            }
        }));

        let decisions = self
            .db
            .list_decisions_for_session(&session.id, 32)
            .unwrap_or_default();
        events.extend(decisions.into_iter().map(|entry| TimelineEvent {
            occurred_at: entry.timestamp,
            session_id: session.id.clone(),
            event_type: TimelineEventType::Decision,
            summary: decision_log_summary(&entry),
            detail_lines: decision_log_detail_lines(&entry),
        }));

        let tool_logs = self
            .db
            .query_tool_logs(&session.id, 1, 128)
            .map(|page| page.entries)
            .unwrap_or_default();
        events.extend(tool_logs.into_iter().filter_map(|entry| {
            parse_rfc3339_to_utc(&entry.timestamp).map(|occurred_at| TimelineEvent {
                occurred_at,
                session_id: session.id.clone(),
                event_type: TimelineEventType::ToolCall,
                summary: format!(
                    "tool {} | {}ms | {}",
                    entry.tool_name,
                    entry.duration_ms,
                    truncate_for_dashboard(&entry.input_summary, 56)
                ),
                detail_lines: tool_log_detail_lines(&entry),
            })
        }));
        events
    }

    fn recompute_search_matches(&mut self) {
        let Some(query) = self.search_query.clone() else {
            self.search_matches.clear();
            self.selected_search_match = 0;
            return;
        };

        let Ok(regex) = compile_search_regex(&query) else {
            self.search_matches.clear();
            self.selected_search_match = 0;
            return;
        };

        self.search_matches = if self.output_mode == OutputMode::ContextGraph {
            self.visible_graph_lines()
                .into_iter()
                .enumerate()
                .filter_map(|(index, line)| {
                    regex.is_match(&line.text).then_some(SearchMatch {
                        session_id: line.session_id,
                        line_index: index,
                    })
                })
                .collect()
        } else {
            self.search_target_session_ids()
                .into_iter()
                .flat_map(|session_id| {
                    self.visible_output_lines_for_session(session_id)
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, line)| {
                            regex.is_match(&line.text).then_some(SearchMatch {
                                session_id: session_id.to_string(),
                                line_index: index,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        };

        if self.search_matches.is_empty() {
            self.selected_search_match = 0;
            return;
        }

        self.selected_search_match = self
            .selected_search_match
            .min(self.search_matches.len().saturating_sub(1));
        self.focus_selected_search_match();
    }

    fn focus_selected_search_match(&mut self) {
        let Some(search_match) = self.search_matches.get(self.selected_search_match).cloned()
        else {
            return;
        };

        if !search_match.session_id.is_empty()
            && self.selected_session_id() != Some(search_match.session_id.as_str())
        {
            self.sync_selection_by_id(Some(&search_match.session_id));
            self.sync_selected_output();
            self.sync_selected_diff();
            self.sync_selected_messages();
            self.sync_selected_lineage();
            self.refresh_logs();
        }

        self.output_follow = false;
        let viewport_height = self.last_output_height.max(1);
        let offset = search_match
            .line_index
            .saturating_sub(viewport_height.saturating_sub(1) / 2);
        self.output_scroll_offset = offset.min(self.max_output_scroll());
    }

    fn search_navigation_note(&self) -> String {
        let query = self.search_query.as_deref().unwrap_or_default();
        let total = self.search_matches.len();
        let current = if total == 0 {
            0
        } else {
            self.selected_search_match.min(total.saturating_sub(1)) + 1
        };

        let mode = if self.output_mode == OutputMode::ContextGraph {
            "graph search"
        } else {
            "search"
        };
        format!(
            "{mode} /{query} match {current}/{total} | {}",
            self.search_scope.label()
        )
    }

    fn search_match_session_count(&self) -> usize {
        self.search_matches
            .iter()
            .filter(|search_match| !search_match.session_id.is_empty())
            .map(|search_match| search_match.session_id.as_str())
            .collect::<HashSet<_>>()
            .len()
    }

    fn search_target_session_ids(&self) -> Vec<&str> {
        let selected_session_id = self.selected_session_id();
        let selected_agent_type = self.selected_agent_type();

        self.sessions
            .iter()
            .filter(|session| {
                self.search_scope
                    .matches(selected_session_id, session.id.as_str())
                    && self
                        .search_agent_filter
                        .matches(selected_agent_type, session.agent_type.as_str())
            })
            .map(|session| session.id.as_str())
            .collect()
    }

    fn next_approval_target_session_id(&self) -> Option<String> {
        let pending_items: usize = self.approval_queue_counts.values().sum();
        if pending_items == 0 {
            return None;
        }

        let active_session_ids: HashSet<_> =
            self.sessions.iter().map(|session| &session.id).collect();
        let queue = self.db.unread_approval_queue(pending_items).ok()?;
        let mut seen = HashSet::new();
        let ordered_targets = queue
            .into_iter()
            .filter_map(|message| {
                if active_session_ids.contains(&message.to_session)
                    && seen.insert(message.to_session.clone())
                {
                    Some(message.to_session)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if ordered_targets.is_empty() {
            return None;
        }

        let current_session_id = self.selected_session_id();
        current_session_id
            .and_then(|session_id| {
                ordered_targets
                    .iter()
                    .position(|target_session_id| target_session_id == session_id)
                    .map(|index| ordered_targets[(index + 1) % ordered_targets.len()].clone())
            })
            .or_else(|| ordered_targets.first().cloned())
    }

    fn sync_output_scroll(&mut self, viewport_height: usize) {
        self.last_output_height = viewport_height.max(1);
        if self.output_mode == OutputMode::GitStatus {
            let max_scroll = self.max_output_scroll();
            let centered = self
                .selected_git_status
                .saturating_sub(self.last_output_height.max(1).saturating_sub(1) / 2);
            self.output_scroll_offset = centered.min(max_scroll);
            return;
        }
        let max_scroll = self.max_output_scroll();

        if self.output_follow {
            self.output_scroll_offset = max_scroll;
        } else {
            self.output_scroll_offset = self.output_scroll_offset.min(max_scroll);
        }
    }

    fn max_output_scroll(&self) -> usize {
        let total_lines = if self.output_mode == OutputMode::GitStatus {
            self.selected_git_status_entries.len()
        } else if matches!(
            self.output_mode,
            OutputMode::WorktreeDiff | OutputMode::GitPatch
        ) {
            self.active_patch_text()
                .map(|patch| patch.lines().count())
                .unwrap_or(0)
        } else if self.output_mode == OutputMode::ContextGraph {
            self.visible_graph_lines().len()
        } else if self.output_mode == OutputMode::Timeline {
            self.visible_timeline_lines().len()
        } else {
            self.visible_output_lines().len()
        };
        total_lines.saturating_sub(self.last_output_height.max(1))
    }

    fn sync_metrics_scroll(&mut self, viewport_height: usize) {
        self.last_metrics_height = viewport_height.max(1);
        let max_scroll = self.max_metrics_scroll();
        self.metrics_scroll_offset = self.metrics_scroll_offset.min(max_scroll);
    }

    fn max_metrics_scroll(&self) -> usize {
        self.selected_session_metrics_text()
            .lines()
            .count()
            .saturating_sub(self.last_metrics_height.max(1))
    }

    fn focused_delegate_index(&self) -> Option<usize> {
        if self.selected_child_sessions.is_empty() {
            return None;
        }

        self.focused_delegate_session_id
            .as_deref()
            .and_then(|session_id| {
                self.selected_child_sessions
                    .iter()
                    .position(|delegate| delegate.session_id == session_id)
            })
            .or(Some(0))
    }

    fn set_focused_delegate_by_index(&mut self, index: usize) {
        let Some(delegate) = self.selected_child_sessions.get(index) else {
            return;
        };
        let delegate_session_id = delegate.session_id.clone();

        self.focused_delegate_session_id = Some(delegate_session_id.clone());
        self.ensure_focused_delegate_visible();
        self.set_operator_note(format!(
            "focused delegate {}",
            format_session_id(&delegate_session_id)
        ));
    }

    fn sync_focused_delegate_selection(&mut self) {
        self.focused_delegate_session_id = self
            .focused_delegate_index()
            .and_then(|index| self.selected_child_sessions.get(index))
            .map(|delegate| delegate.session_id.clone());
        self.ensure_focused_delegate_visible();
    }

    fn ensure_focused_delegate_visible(&mut self) {
        let Some(delegate_index) = self.focused_delegate_index() else {
            return;
        };
        let Some(line_index) = self.delegate_metrics_line_index(delegate_index) else {
            return;
        };

        let viewport_height = self.last_metrics_height.max(1);
        if line_index < self.metrics_scroll_offset {
            self.metrics_scroll_offset = line_index;
        } else if line_index >= self.metrics_scroll_offset + viewport_height {
            self.metrics_scroll_offset =
                line_index.saturating_sub(viewport_height.saturating_sub(1));
        }
        self.metrics_scroll_offset = self.metrics_scroll_offset.min(self.max_metrics_scroll());
    }

    fn delegate_metrics_line_index(&self, target_index: usize) -> Option<usize> {
        if target_index >= self.selected_child_sessions.len() {
            return None;
        }

        let mut line_index = self.metrics_line_count_before_delegates();
        for delegate in self.selected_child_sessions.iter().take(target_index) {
            line_index += 1;
            if delegate.last_output_preview.is_some() {
                line_index += 1;
            }
        }

        Some(line_index)
    }

    fn metrics_line_count_before_delegates(&self) -> usize {
        if self.sessions.get(self.selected_session).is_none() {
            return 0;
        }

        let mut line_count = 2;
        if self.selected_parent_session.is_some() {
            line_count += 1;
        }
        if self.selected_team_summary.is_some() {
            line_count += 1;
        }
        line_count += 1;
        line_count += 1;

        let stabilized = self.daemon_activity.stabilized_after_recovery_at();
        if self.daemon_activity.chronic_saturation_streak > 0 {
            line_count += 1;
        }
        if self.daemon_activity.operator_escalation_required() {
            line_count += 1;
        }
        if self
            .daemon_activity
            .chronic_saturation_cleared_at()
            .is_some()
        {
            line_count += 1;
        }
        if stabilized.is_some() {
            line_count += 1;
        }
        if self.daemon_activity.last_dispatch_at.is_some() {
            line_count += 1;
        }
        if stabilized.is_none() {
            if self.daemon_activity.last_recovery_dispatch_at.is_some() {
                line_count += 1;
            }
            if self.daemon_activity.last_rebalance_at.is_some() {
                line_count += 1;
            }
        }
        if self.daemon_activity.last_auto_merge_at.is_some() {
            line_count += 1;
        }
        if self.daemon_activity.last_auto_prune_at.is_some() {
            line_count += 1;
        }
        if self.selected_route_preview.is_some() {
            line_count += 1;
        }
        if !self.selected_child_sessions.is_empty() {
            line_count += 1;
        }

        line_count
    }

    #[cfg(test)]
    fn visible_output_text(&self) -> String {
        self.visible_output_lines()
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn reset_output_view(&mut self) {
        self.output_follow = true;
        self.output_scroll_offset = 0;
    }

    fn reset_metrics_view(&mut self) {
        self.metrics_scroll_offset = 0;
    }

    fn refresh_logs(&mut self) {
        let Some(session_id) = self.selected_session_id().map(ToOwned::to_owned) else {
            self.logs.clear();
            return;
        };

        match self.db.query_tool_logs(&session_id, 1, MAX_LOG_ENTRIES) {
            Ok(page) => self.logs = page.entries,
            Err(error) => {
                tracing::warn!("Failed to load tool logs: {error}");
                self.logs.clear();
            }
        }
    }

    fn aggregate_usage(&self) -> AggregateUsage {
        let thresholds = self.cfg.effective_budget_alert_thresholds();
        let total_tokens = self
            .sessions
            .iter()
            .map(|session| session.metrics.tokens_used)
            .sum();
        let total_cost_usd = self
            .sessions
            .iter()
            .map(|session| session.metrics.cost_usd)
            .sum::<f64>();
        let token_state = budget_state(
            total_tokens as f64,
            self.cfg.token_budget as f64,
            thresholds,
        );
        let cost_state = budget_state(total_cost_usd, self.cfg.cost_budget_usd, thresholds);

        AggregateUsage {
            total_tokens,
            total_cost_usd,
            token_state,
            cost_state,
            overall_state: token_state.max(cost_state),
        }
    }

    fn selected_session_metrics_text(&self) -> String {
        if let Some(session) = self.sessions.get(self.selected_session) {
            let metrics = &session.metrics;
            let selected_profile = self.db.get_session_profile(&session.id).ok().flatten();
            let group_peers = self
                .sessions
                .iter()
                .filter(|candidate| {
                    candidate.project == session.project
                        && candidate.task_group == session.task_group
                })
                .count();
            let mut lines = vec![
                format!(
                    "Selected {} [{}]",
                    &session.id[..8.min(session.id.len())],
                    session.state
                ),
                format!("Task {}", session.task),
                format!(
                    "Project {} | Group {} | Peer sessions {}",
                    session.project, session.task_group, group_peers
                ),
            ];

            if let Some(profile) = selected_profile.as_ref() {
                let model = profile.model.as_deref().unwrap_or("default");
                let permission_mode = profile.permission_mode.as_deref().unwrap_or("default");
                lines.push(format!(
                    "Profile {} | Model {} | Permissions {}",
                    profile.profile_name, model, permission_mode
                ));
                let mut profile_details = Vec::new();
                if let Some(token_budget) = profile.token_budget {
                    profile_details.push(format!(
                        "Profile tokens {}",
                        format_token_count(token_budget)
                    ));
                }
                if let Some(max_budget_usd) = profile.max_budget_usd {
                    profile_details
                        .push(format!("Profile cost {}", format_currency(max_budget_usd)));
                }
                if !profile.allowed_tools.is_empty() {
                    profile_details.push(format!(
                        "Allow {}",
                        truncate_for_dashboard(&profile.allowed_tools.join(", "), 36)
                    ));
                }
                if !profile.disallowed_tools.is_empty() {
                    profile_details.push(format!(
                        "Deny {}",
                        truncate_for_dashboard(&profile.disallowed_tools.join(", "), 36)
                    ));
                }
                if !profile.add_dirs.is_empty() {
                    profile_details.push(format!(
                        "Dirs {}",
                        truncate_for_dashboard(
                            &profile
                                .add_dirs
                                .iter()
                                .map(|path| path.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                            36
                        )
                    ));
                }
                if !profile_details.is_empty() {
                    lines.push(profile_details.join(" | "));
                }
            }

            if let Some(parent) = self.selected_parent_session.as_ref() {
                lines.push(format!("Delegated from {}", format_session_id(parent)));
            }

            if let Some(team) = self.selected_team_summary {
                lines.push(format!(
                    "Team {}/{} | idle {} | running {} | pending {} | failed {} | stopped {}",
                    team.total,
                    self.cfg.max_parallel_sessions,
                    team.idle,
                    team.running,
                    team.pending,
                    team.failed,
                    team.stopped
                ));
            }

            lines.push(format!(
                "Global handoff backlog {} lead(s) / {} handoff(s) | Auto-dispatch {} @ {}/lead | Auto-worktree {} | Auto-merge {}",
                self.global_handoff_backlog_leads,
                self.global_handoff_backlog_messages,
                if self.cfg.auto_dispatch_unread_handoffs {
                    "on"
                } else {
                    "off"
                },
                self.cfg.auto_dispatch_limit_per_session,
                if self.cfg.auto_create_worktrees {
                    "on"
                } else {
                    "off"
                },
                if self.cfg.auto_merge_ready_worktrees {
                    "on"
                } else {
                    "off"
                }
            ));

            let stabilized = self.daemon_activity.stabilized_after_recovery_at();

            lines.push(format!(
                "Coordination mode {}",
                if self.daemon_activity.dispatch_cooloff_active() {
                    "rebalance-cooloff (chronic saturation)"
                } else if self.daemon_activity.prefers_rebalance_first() {
                    "rebalance-first (chronic saturation)"
                } else if stabilized.is_some() {
                    "dispatch-first (stabilized)"
                } else {
                    "dispatch-first"
                }
            ));

            if self.daemon_activity.chronic_saturation_streak > 0 {
                lines.push(format!(
                    "Chronic saturation streak {} cycle(s)",
                    self.daemon_activity.chronic_saturation_streak
                ));
            }

            if self.daemon_activity.operator_escalation_required() {
                lines.push(
                    "Operator escalation recommended: chronic saturation is not clearing".into(),
                );
            }

            if let Some(cleared_at) = self.daemon_activity.chronic_saturation_cleared_at() {
                lines.push(format!(
                    "Chronic saturation cleared @ {}",
                    self.short_timestamp(&cleared_at.to_rfc3339())
                ));
            }

            if let Some(stabilized_at) = stabilized {
                lines.push(format!(
                    "Recovery stabilized @ {}",
                    self.short_timestamp(&stabilized_at.to_rfc3339())
                ));
            }

            if let Some(last_dispatch_at) = self.daemon_activity.last_dispatch_at.as_ref() {
                lines.push(format!(
                    "Last daemon dispatch {} routed / {} deferred across {} lead(s) @ {}",
                    self.daemon_activity.last_dispatch_routed,
                    self.daemon_activity.last_dispatch_deferred,
                    self.daemon_activity.last_dispatch_leads,
                    self.short_timestamp(&last_dispatch_at.to_rfc3339())
                ));
            }

            if stabilized.is_none() {
                if let Some(last_recovery_dispatch_at) =
                    self.daemon_activity.last_recovery_dispatch_at.as_ref()
                {
                    lines.push(format!(
                        "Last daemon recovery dispatch {} handoff(s) across {} lead(s) @ {}",
                        self.daemon_activity.last_recovery_dispatch_routed,
                        self.daemon_activity.last_recovery_dispatch_leads,
                        self.short_timestamp(&last_recovery_dispatch_at.to_rfc3339())
                    ));
                }

                if let Some(last_rebalance_at) = self.daemon_activity.last_rebalance_at.as_ref() {
                    lines.push(format!(
                        "Last daemon rebalance {} handoff(s) across {} lead(s) @ {}",
                        self.daemon_activity.last_rebalance_rerouted,
                        self.daemon_activity.last_rebalance_leads,
                        self.short_timestamp(&last_rebalance_at.to_rfc3339())
                    ));
                }
            }

            if let Some(last_auto_merge_at) = self.daemon_activity.last_auto_merge_at.as_ref() {
                lines.push(format!(
                    "Last daemon auto-merge {} merged / {} active / {} conflicted / {} dirty / {} failed @ {}",
                    self.daemon_activity.last_auto_merge_merged,
                    self.daemon_activity.last_auto_merge_active_skipped,
                    self.daemon_activity.last_auto_merge_conflicted_skipped,
                    self.daemon_activity.last_auto_merge_dirty_skipped,
                    self.daemon_activity.last_auto_merge_failed,
                    self.short_timestamp(&last_auto_merge_at.to_rfc3339())
                ));
            }

            if let Some(last_auto_prune_at) = self.daemon_activity.last_auto_prune_at.as_ref() {
                lines.push(format!(
                    "Last daemon auto-prune {} pruned / {} active @ {}",
                    self.daemon_activity.last_auto_prune_pruned,
                    self.daemon_activity.last_auto_prune_active_skipped,
                    self.short_timestamp(&last_auto_prune_at.to_rfc3339())
                ));
            }

            if let Some(route_preview) = self.selected_route_preview.as_ref() {
                lines.push(format!("Next route {route_preview}"));
            }

            if !self.selected_child_sessions.is_empty() {
                lines.push("Delegates".to_string());
                for child in &self.selected_child_sessions {
                    let mut child_line = format!(
                        "{} {} [{}] | next {}",
                        if self.focused_delegate_session_id.as_deref()
                            == Some(child.session_id.as_str())
                        {
                            ">>"
                        } else {
                            "-"
                        },
                        format_session_id(&child.session_id),
                        session_state_label(&child.state),
                        delegate_next_action(child)
                    );
                    if let Some(worktree_health) = child.worktree_health {
                        child_line.push_str(&format!(
                            " | worktree {}",
                            delegate_worktree_health_label(worktree_health)
                        ));
                    }
                    child_line.push_str(&format!(
                        " | approvals {} | backlog {} | progress {} tok / {} files / {} | task {}",
                        child.approval_backlog,
                        child.handoff_backlog,
                        format_token_count(child.tokens_used),
                        child.files_changed,
                        format_duration(child.duration_secs),
                        child.task_preview
                    ));
                    if let Some(branch) = child.branch.as_ref() {
                        child_line.push_str(&format!(" | branch {branch}"));
                    }
                    lines.push(child_line);
                    if let Some(last_output_preview) = child.last_output_preview.as_ref() {
                        lines.push(format!("  last output {last_output_preview}"));
                    }
                }
            }

            if let Some(worktree) = session.worktree.as_ref() {
                lines.push(format!(
                    "Branch {} | Base {}",
                    worktree.branch, worktree.base_branch
                ));
                lines.push(format!("Worktree {}", worktree.path.display()));
                if let Some(diff_summary) = self.selected_diff_summary.as_ref() {
                    lines.push(format!("Diff {diff_summary}"));
                }
                if !self.selected_diff_preview.is_empty() {
                    lines.push("Changed files".to_string());
                    for entry in &self.selected_diff_preview {
                        lines.push(format!("- {entry}"));
                    }
                }
                if let Some(merge_readiness) = self.selected_merge_readiness.as_ref() {
                    lines.push(merge_readiness.summary.clone());
                    for conflict in merge_readiness.conflicts.iter().take(3) {
                        lines.push(format!("- conflict {conflict}"));
                    }
                }
                if let Ok(merge_queue) = manager::build_merge_queue(&self.db) {
                    let entry = merge_queue
                        .ready_entries
                        .iter()
                        .chain(merge_queue.blocked_entries.iter())
                        .find(|entry| entry.session_id == session.id);
                    if let Some(entry) = entry {
                        lines.push("Merge queue".to_string());
                        if let Some(position) = entry.queue_position {
                            lines.push(format!(
                                "- ready #{} | {}",
                                position, entry.suggested_action
                            ));
                        } else {
                            lines.push(format!("- blocked | {}", entry.suggested_action));
                        }
                        for blocker in entry.blocked_by.iter().take(2) {
                            lines.push(format!(
                                "  blocker {} [{}] | {}",
                                format_session_id(&blocker.session_id),
                                blocker.branch,
                                blocker.summary
                            ));
                            for conflict in blocker.conflicts.iter().take(3) {
                                lines.push(format!("    conflict {conflict}"));
                            }
                        }
                    }
                }
            }

            if let Some(harness) = self.session_harnesses.get(&session.id) {
                lines.push(format!(
                    "Harness {} | Detected {}",
                    harness.primary_label,
                    harness.detected_summary()
                ));
            }

            lines.push(format!(
                "Tokens {} total | In {} | Out {}",
                format_token_count(metrics.tokens_used),
                format_token_count(metrics.input_tokens),
                format_token_count(metrics.output_tokens),
            ));
            lines.push(format!(
                "Tools {} | Files {}",
                metrics.tool_calls, metrics.files_changed,
            ));
            let recent_file_activity = self
                .db
                .list_file_activity(&session.id, 5)
                .unwrap_or_default();
            if !recent_file_activity.is_empty() {
                lines.push("Recent file activity".to_string());
                for entry in recent_file_activity {
                    lines.push(format!(
                        "- {} {}",
                        self.short_timestamp(&entry.timestamp.to_rfc3339()),
                        file_activity_summary(&entry)
                    ));
                    for detail in file_activity_patch_lines(&entry, 2) {
                        lines.push(format!("  {}", detail));
                    }
                }
            }
            let recent_decisions = self
                .db
                .list_decisions_for_session(&session.id, 5)
                .unwrap_or_default();
            if !recent_decisions.is_empty() {
                lines.push("Recent decisions".to_string());
                for entry in recent_decisions {
                    lines.push(format!(
                        "- {} {}",
                        self.short_timestamp(&entry.timestamp.to_rfc3339()),
                        decision_log_summary(&entry)
                    ));
                    for detail in decision_log_detail_lines(&entry).into_iter().take(3) {
                        lines.push(format!("  {}", detail));
                    }
                }
            }
            lines.extend(self.session_graph_recall_lines(session));
            lines.extend(self.session_graph_metrics_lines(&session.id));
            let file_overlaps = self
                .db
                .list_file_overlaps(&session.id, 3)
                .unwrap_or_default();
            if !file_overlaps.is_empty() {
                lines.push("Potential overlaps".to_string());
                for overlap in file_overlaps {
                    lines.push(format!(
                        "- {}",
                        file_overlap_summary(
                            &overlap,
                            &self.short_timestamp(&overlap.timestamp.to_rfc3339())
                        )
                    ));
                }
            }
            let conflict_incidents = self
                .db
                .list_open_conflict_incidents_for_session(&session.id, 3)
                .unwrap_or_default();
            if !conflict_incidents.is_empty() {
                lines.push("Active conflicts".to_string());
                for incident in conflict_incidents {
                    lines.push(format!(
                        "- {}",
                        conflict_incident_summary(
                            &incident,
                            &self.short_timestamp(&incident.updated_at.to_rfc3339())
                        )
                    ));
                }
            }
            lines.push(format!(
                "Cost ${:.4} | Duration {}s",
                metrics.cost_usd, metrics.duration_secs
            ));

            if let Some(last_output) = self.selected_output_lines().last() {
                lines.push(format!(
                    "Last output {}",
                    truncate_for_dashboard(&last_output.text, 96)
                ));
            }

            lines.push(String::new());
            if self.selected_messages.is_empty() {
                lines.push("Message inbox clear".to_string());
            } else {
                lines.push("Recent messages:".to_string());
                let recent = self
                    .selected_messages
                    .iter()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>();
                for message in recent.into_iter().rev() {
                    lines.push(format!(
                        "- {} {} -> {} | {}",
                        self.short_timestamp(&message.timestamp.to_rfc3339()),
                        format_session_id(&message.from_session),
                        format_session_id(&message.to_session),
                        comms::preview(&message.msg_type, &message.content)
                    ));
                }
            }

            let attention_items = self.attention_queue_items(3);
            if attention_items.is_empty() {
                lines.push(String::new());
                lines.push("Attention queue clear".to_string());
            } else {
                lines.push(String::new());
                lines.push("Needs attention:".to_string());
                lines.extend(attention_items);
            }

            lines.join("\n")
        } else {
            "No metrics available".to_string()
        }
    }

    fn board_text(&self) -> String {
        if self.sessions.is_empty() {
            return "No sessions available.\n\nStart a session to populate the board.".to_string();
        }

        let mut lines = Vec::new();
        lines.push(format!("Board snapshot | {} sessions", self.sessions.len()));

        if let Some(session) = self.sessions.get(self.selected_session) {
            let meta = self.board_meta_by_session.get(&session.id);
            let branch = session_branch(session);
            lines.push(format!(
                "Focus {} {} | {} | {}{}",
                board_presence_marker(session),
                board_codename(session),
                meta.map(|meta| meta.lane.as_str())
                    .unwrap_or_else(|| board_lane_label(&session.state)),
                format_session_id(&session.id),
                if branch == "-" {
                    String::new()
                } else {
                    format!(" | {branch}")
                }
            ));
            lines.push(format!("Task {}", truncate_for_dashboard(&session.task, 48)));
            if let Some(meta) = meta {
                lines.push(format!(
                    "Progress {:>3}% {}",
                    meta.progress_percent,
                    board_progress_bar(meta.progress_percent)
                ));
                if let Some(status_detail) = meta.status_detail.as_ref() {
                    lines.push(format!("Status {status_detail}"));
                }
                if let Some(movement_note) = meta.movement_note.as_ref() {
                    lines.push(format!("Event {movement_note}"));
                }
                if meta.handoff_backlog > 0 {
                    lines.push(format!("Inbox {} handoff(s)", meta.handoff_backlog));
                }
                if let Some(activity_note) = meta.activity_note.as_ref() {
                    lines.push(format!("Route {activity_note}"));
                }
                lines.push(format!(
                    "Coords C{} R{} S{}",
                    meta.column_index + 1,
                    meta.row_index + 1,
                    meta.stack_index + 1
                ));
                if let Some(row_label) = meta.row_label.as_ref() {
                    lines.push(format!("Row {row_label}"));
                }
                if let Some(project) = meta.project.as_ref() {
                    lines.push(format!("Project {project}"));
                }
                if let Some(feature) = meta.feature.as_ref() {
                    lines.push(format!("Feature {feature}"));
                }
                if let Some(issue) = meta.issue.as_ref() {
                    lines.push(format!("Issue {issue}"));
                }
            }
        }

        let overlap_risks = self.board_overlap_risks();
        if overlap_risks.is_empty() {
            lines.push("Overlap risk clear".to_string());
        } else {
            lines.push("Overlap risk".to_string());
            for risk in overlap_risks {
                lines.push(format!("- {risk}"));
            }
        }

        let lanes = ["Inbox", "In Progress", "Review", "Blocked", "Done", "Stopped"];
        for label in lanes {
            let mut lane_sessions = self
                .sessions
                .iter()
                .filter_map(|session| {
                    let lane = self
                        .board_meta_by_session
                        .get(&session.id)
                        .map(|meta| meta.lane.as_str())
                        .unwrap_or_else(|| board_lane_label(&session.state));
                    if lane == label {
                        Some((session, self.board_meta_by_session.get(&session.id)))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if lane_sessions.is_empty() {
                continue;
            }

            let mut row_risks: HashMap<(i64, String), Vec<String>> = HashMap::new();
            let mut row_backlogs: HashMap<(i64, String), i64> = HashMap::new();
            for (_, meta) in &lane_sessions {
                let Some(meta) = meta else {
                    continue;
                };
                let key = (
                    meta.row_index,
                    meta.row_label
                        .clone()
                        .unwrap_or_else(|| "General".to_string()),
                );
                if let Some(conflict_signal) = meta.conflict_signal.as_ref() {
                    let entry = row_risks.entry(key.clone()).or_default();
                    for risk in conflict_signal.split("; ") {
                        if !entry.iter().any(|existing| existing == risk) {
                            entry.push(risk.to_string());
                        }
                    }
                }
                if meta.handoff_backlog > 0 {
                    *row_backlogs.entry(key).or_default() += meta.handoff_backlog;
                }
            }

            lane_sessions.sort_by(|left, right| {
                let left_meta = left.1.cloned().unwrap_or_default();
                let right_meta = right.1.cloned().unwrap_or_default();
                left_meta
                    .row_index
                    .cmp(&right_meta.row_index)
                    .then_with(|| left_meta.stack_index.cmp(&right_meta.stack_index))
                    .then_with(|| left.0.id.cmp(&right.0.id))
            });

            lines.push(String::new());
            lines.push(format!("{label} ({})", lane_sessions.len()));
            let mut current_row: Option<String> = None;
            for (session, meta) in lane_sessions.into_iter().take(6) {
                let meta = meta.cloned().unwrap_or_default();
                let row_label = meta
                    .row_label
                    .clone()
                    .unwrap_or_else(|| "General".to_string());
                if current_row.as_ref() != Some(&row_label) {
                    current_row = Some(row_label.clone());
                    let row_key = (meta.row_index, row_label.clone());
                    let row_conflict_summary = row_risks
                        .get(&row_key)
                        .filter(|risks| !risks.is_empty())
                        .map(|risks| truncate_for_dashboard(&risks.join(" + "), 42));
                    let row_backlog = row_backlogs.get(&row_key).copied().unwrap_or(0);
                    let row_pressure_summary = if row_backlog > 0 {
                        Some(format!("{} handoff(s)", row_backlog))
                    } else {
                        None
                    };
                    let row_marker = if row_conflict_summary.is_some() {
                        "!"
                    } else if row_pressure_summary.is_some() {
                        "+"
                    } else {
                        "-"
                    };
                    lines.push(format!(
                        "  {} Row {} | {}{}{}",
                        row_marker,
                        meta.row_index + 1,
                        row_label,
                        row_conflict_summary
                            .map(|summary| format!(" | {summary}"))
                            .unwrap_or_default(),
                        row_pressure_summary
                            .map(|summary| format!(" | {summary}"))
                            .unwrap_or_default()
                    ));
                }
                let branch = session_branch(session);
                let branch_suffix = if branch == "-" {
                    String::new()
                } else {
                    format!(" | {branch}")
                };
                let activity_suffix = meta
                    .activity_note
                    .as_ref()
                    .map(|note| format!(" | {}", truncate_for_dashboard(note, 26)))
                    .unwrap_or_default();
                let backlog_suffix = if meta.handoff_backlog > 0 {
                    format!(" | inbox {}", meta.handoff_backlog)
                } else {
                    String::new()
                };
                let kind_marker = board_activity_marker(&meta);
                lines.push(format!(
                    "    {}{} {} {} {} [{}] {:>3}% {} | {}{}{}{}",
                    board_motion_marker(&meta),
                    kind_marker,
                    board_presence_marker(session),
                    board_codename(session),
                    format_session_id(&session.id),
                    session.agent_type,
                    meta.progress_percent,
                    board_progress_bar(meta.progress_percent),
                    truncate_for_dashboard(meta.status_detail.as_deref().unwrap_or(&session.task), 18),
                    activity_suffix,
                    backlog_suffix,
                    branch_suffix
                ));
            }
        }

        lines.join("\n")
    }

    fn board_overlap_risks(&self) -> Vec<String> {
        let mut risks = self
            .board_meta_by_session
            .values()
            .filter_map(|meta| meta.conflict_signal.clone())
            .collect::<Vec<_>>();
        if risks.is_empty() {
            let mut duplicate_branches: HashMap<String, Vec<String>> = HashMap::new();
            let mut duplicate_tasks: HashMap<String, Vec<String>> = HashMap::new();

            for session in self.sessions.iter().filter(|session| {
                matches!(
                    session.state,
                    SessionState::Pending
                        | SessionState::Running
                        | SessionState::Idle
                        | SessionState::Stale
                )
            }) {
                if let Some(worktree) = session.worktree.as_ref() {
                    duplicate_branches
                        .entry(worktree.branch.clone())
                        .or_default()
                        .push(format_session_id(&session.id));
                }
                duplicate_tasks
                    .entry(session.task.trim().to_ascii_lowercase())
                    .or_default()
                    .push(format_session_id(&session.id));
            }

            for (branch, sessions) in duplicate_branches {
                if sessions.len() >= 2 {
                    risks.push(format!("Shared branch {branch}: {}", sessions.join(", ")));
                }
            }
            for (task, sessions) in duplicate_tasks {
                if sessions.len() >= 2 {
                    risks.push(format!(
                        "Shared task {}: {}",
                        truncate_for_dashboard(&task, 32),
                        sessions.join(", ")
                    ));
                }
            }
        }
        risks.sort();
        risks.dedup();
        risks
    }

    fn aggregate_cost_summary(&self) -> (String, Style) {
        let aggregate = self.aggregate_usage();
        let thresholds = self.cfg.effective_budget_alert_thresholds();
        let mut text = if self.cfg.cost_budget_usd > 0.0 {
            format!(
                "Aggregate cost {} / {}",
                format_currency(aggregate.total_cost_usd),
                format_currency(self.cfg.cost_budget_usd),
            )
        } else {
            format!(
                "Aggregate cost {} (no budget)",
                format_currency(aggregate.total_cost_usd)
            )
        };

        if let Some(summary_suffix) = aggregate.overall_state.summary_suffix(thresholds) {
            text.push_str(" | ");
            text.push_str(&summary_suffix);
        }

        (text, aggregate.overall_state.style())
    }

    fn attention_queue_items(&self, limit: usize) -> Vec<String> {
        let mut items = Vec::new();
        let suppress_inbox_attention = self
            .daemon_activity
            .stabilized_after_recovery_at()
            .is_some();

        for session in &self.sessions {
            if self.worktree_health_by_session.get(&session.id).copied()
                == Some(worktree::WorktreeHealth::Conflicted)
            {
                items.push(format!(
                    "- Conflicted worktree {} | {}",
                    format_session_id(&session.id),
                    truncate_for_dashboard(&session.task, 48)
                ));
            }

            let handoff_backlog = self
                .handoff_backlog_counts
                .get(&session.id)
                .copied()
                .unwrap_or(0);
            if handoff_backlog > 0 && !suppress_inbox_attention {
                items.push(format!(
                    "- Backlog {} | {} handoff(s) | {}",
                    format_session_id(&session.id),
                    handoff_backlog,
                    truncate_for_dashboard(&session.task, 40)
                ));
            }

            if matches!(
                session.state,
                SessionState::Failed | SessionState::Stopped | SessionState::Pending
            ) {
                items.push(format!(
                    "- {} {} | {}",
                    session_state_label(&session.state),
                    format_session_id(&session.id),
                    truncate_for_dashboard(&session.task, 48)
                ));
            }

            if items.len() >= limit {
                break;
            }
        }

        items.truncate(limit);
        items
    }

    fn set_operator_note(&mut self, note: String) {
        self.operator_note = Some(note);
    }

    fn active_session_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|session| {
                matches!(
                    session.state,
                    SessionState::Pending
                        | SessionState::Running
                        | SessionState::Idle
                        | SessionState::Stale
                )
            })
            .count()
    }

    fn refresh_after_spawn(&mut self, select_session_id: Option<&str>) {
        self.refresh();
        self.sync_selection_by_id(select_session_id);
        self.reset_output_view();
        self.reset_metrics_view();
        self.sync_selected_output();
        self.sync_selected_diff();
        self.sync_selected_messages();
        self.sync_selected_lineage();
        self.refresh_logs();
    }

    fn new_session_task(&self) -> String {
        self.sessions
            .get(self.selected_session)
            .map(|session| {
                format!(
                    "Follow up on {}: {}",
                    format_session_id(&session.id),
                    truncate_for_dashboard(&session.task, 96)
                )
            })
            .unwrap_or_else(|| "New ECC 2.0 session".to_string())
    }

    fn spawn_prompt_seed(&self) -> String {
        format!("give me 2 agents working on {}", self.new_session_task())
    }

    fn build_spawn_plan(&self, input: &str) -> Result<SpawnPlan, String> {
        let request = parse_spawn_request(input)?;
        let available_slots = self
            .cfg
            .max_parallel_sessions
            .saturating_sub(self.active_session_count());

        match request {
            SpawnRequest::AdHoc {
                requested_count,
                task,
            } => {
                if available_slots == 0 {
                    return Err(format!(
                        "cannot queue sessions: active session limit reached ({})",
                        self.cfg.max_parallel_sessions
                    ));
                }

                Ok(SpawnPlan::AdHoc {
                    requested_count,
                    spawn_count: requested_count.min(available_slots),
                    task,
                })
            }
            SpawnRequest::Template {
                name,
                task,
                variables,
            } => {
                let repo_root = std::env::current_dir().map_err(|error| {
                    format!("failed to resolve cwd for template preview: {error}")
                })?;
                let source_session = self.sessions.get(self.selected_session);
                let preview_vars = manager::build_template_variables(
                    &repo_root,
                    source_session,
                    task.as_deref(),
                    variables.clone(),
                );
                let template = self
                    .cfg
                    .resolve_orchestration_template(&name, &preview_vars)
                    .map_err(|error| error.to_string())?;
                if available_slots < template.steps.len() {
                    return Err(format!(
                        "template {name} requires {} session slots but only {available_slots} available",
                        template.steps.len()
                    ));
                }

                Ok(SpawnPlan::Template {
                    name,
                    task,
                    variables,
                    step_count: template.steps.len(),
                })
            }
        }
    }

    fn pane_areas(&self, area: Rect) -> PaneAreas {
        let detail_panes = self.visible_detail_panes();
        match self.cfg.pane_layout {
            PaneLayout::Horizontal => {
                let columns = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(self.primary_constraints())
                    .split(area);
                let mut pane_areas = PaneAreas {
                    sessions: columns[0],
                    output: None,
                    metrics: None,
                    log: None,
                };
                for (pane, rect) in horizontal_detail_layout(columns[1], &detail_panes) {
                    pane_areas.assign(pane, rect);
                }
                pane_areas
            }
            PaneLayout::Vertical => {
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(self.primary_constraints())
                    .split(area);
                let mut pane_areas = PaneAreas {
                    sessions: rows[0],
                    output: None,
                    metrics: None,
                    log: None,
                };
                for (pane, rect) in vertical_detail_layout(rows[1], &detail_panes) {
                    pane_areas.assign(pane, rect);
                }
                pane_areas
            }
            PaneLayout::Grid => {
                if detail_panes.len() < 3 {
                    let columns = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(self.primary_constraints())
                        .split(area);
                    let mut pane_areas = PaneAreas {
                        sessions: columns[0],
                        output: None,
                        metrics: None,
                        log: None,
                    };
                    for (pane, rect) in horizontal_detail_layout(columns[1], &detail_panes) {
                        pane_areas.assign(pane, rect);
                    }
                    pane_areas
                } else {
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(self.primary_constraints())
                        .split(area);
                    let top_columns = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(self.primary_constraints())
                        .split(rows[0]);
                    let bottom_columns = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(self.primary_constraints())
                        .split(rows[1]);

                    PaneAreas {
                        sessions: top_columns[0],
                        output: Some(top_columns[1]),
                        metrics: Some(bottom_columns[0]),
                        log: Some(bottom_columns[1]),
                    }
                }
            }
        }
    }

    fn primary_constraints(&self) -> [Constraint; 2] {
        [
            Constraint::Percentage(self.pane_size_percent),
            Constraint::Percentage(100 - self.pane_size_percent),
        ]
    }

    fn visible_panes(&self) -> Vec<Pane> {
        self.layout_panes()
            .into_iter()
            .filter(|pane| !self.collapsed_panes.contains(pane))
            .collect()
    }

    fn visible_detail_panes(&self) -> Vec<Pane> {
        self.layout_panes()
            .into_iter()
            .filter(|pane| !self.collapsed_panes.contains(pane))
            .filter(|pane| *pane != Pane::Sessions)
            .collect()
    }

    fn layout_panes(&self) -> Vec<Pane> {
        match self.cfg.pane_layout {
            PaneLayout::Grid => vec![Pane::Sessions, Pane::Output, Pane::Metrics, Pane::Log],
            PaneLayout::Horizontal | PaneLayout::Vertical => {
                vec![Pane::Sessions, Pane::Output, Pane::Metrics]
            }
        }
    }

    fn selected_pane_index(&self) -> usize {
        self.visible_panes()
            .iter()
            .position(|pane| *pane == self.selected_pane)
            .unwrap_or(0)
    }

    fn pane_border_style(&self, pane: Pane) -> Style {
        if self.selected_pane == pane {
            Style::default().fg(self.theme_palette().accent)
        } else {
            Style::default()
        }
    }

    fn layout_label(&self) -> &'static str {
        match self.cfg.pane_layout {
            PaneLayout::Horizontal => "horizontal",
            PaneLayout::Vertical => "vertical",
            PaneLayout::Grid => "grid",
        }
    }

    fn theme_label(&self) -> &'static str {
        match self.cfg.theme {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    fn board_pane_visible(&self) -> bool {
        self.cfg.pane_layout == PaneLayout::Grid
            && !self.collapsed_panes.contains(&Pane::Metrics)
            && self.layout_panes().contains(&Pane::Metrics)
    }

    fn is_pane_visible(&self, pane: Pane) -> bool {
        match pane {
            Pane::Board => self.board_pane_visible(),
            _ => self.visible_panes().contains(&pane),
        }
    }

    fn theme_palette(&self) -> ThemePalette {
        match self.cfg.theme {
            Theme::Dark => ThemePalette {
                accent: Color::Cyan,
                row_highlight_bg: Color::DarkGray,
                muted: Color::DarkGray,
                help_border: Color::Yellow,
            },
            Theme::Light => ThemePalette {
                accent: Color::Blue,
                row_highlight_bg: Color::Gray,
                muted: Color::Black,
                help_border: Color::Blue,
            },
        }
    }

    fn log_field<'a>(&self, value: &'a str) -> &'a str {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            "n/a"
        } else {
            trimmed
        }
    }

    fn short_timestamp(&self, timestamp: &str) -> String {
        chrono::DateTime::parse_from_rfc3339(timestamp)
            .map(|value| value.format("%H:%M:%S").to_string())
            .unwrap_or_else(|_| timestamp.to_string())
    }

    #[cfg(test)]
    fn aggregate_cost_summary_text(&self) -> String {
        self.aggregate_cost_summary().0
    }

    #[cfg(test)]
    fn selected_output_text(&self) -> String {
        self.selected_output_lines()
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[cfg(test)]
    fn rendered_output_text(&mut self, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| self.render(frame)).expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }
}

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Pane::Sessions => "Sessions",
            Pane::Output => "Output",
            Pane::Metrics => "Metrics",
            Pane::Board => "Board",
            Pane::Log => "Log",
        }
    }

    fn from_shortcut(slot: usize) -> Option<Self> {
        match slot {
            1 => Some(Self::Sessions),
            2 => Some(Self::Output),
            3 => Some(Self::Metrics),
            4 => Some(Self::Log),
            5 => Some(Self::Board),
            _ => None,
        }
    }

    fn sort_key(self) -> u8 {
        match self {
            Self::Sessions => 1,
            Self::Output => 2,
            Self::Metrics => 3,
            Self::Board => 4,
            Self::Log => 5,
        }
    }
}

fn pane_rect(pane_areas: &PaneAreas, pane: Pane) -> Option<Rect> {
    match pane {
        Pane::Sessions => Some(pane_areas.sessions),
        Pane::Output => pane_areas.output,
        Pane::Metrics => pane_areas.metrics,
        Pane::Board => pane_areas.metrics,
        Pane::Log => pane_areas.log,
    }
}

fn pane_center(rect: Rect) -> (i16, i16) {
    (
        rect.x as i16 + rect.width as i16 / 2,
        rect.y as i16 + rect.height as i16 / 2,
    )
}

impl OutputFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::ErrorsOnly,
            Self::ErrorsOnly => Self::ToolCallsOnly,
            Self::ToolCallsOnly => Self::FileChangesOnly,
            Self::FileChangesOnly => Self::All,
        }
    }

    fn matches(self, line: &OutputLine) -> bool {
        match self {
            OutputFilter::All => true,
            OutputFilter::ErrorsOnly => line.stream == OutputStream::Stderr,
            OutputFilter::ToolCallsOnly => looks_like_tool_call(&line.text),
            OutputFilter::FileChangesOnly => looks_like_file_change(&line.text),
        }
    }

    fn label(self) -> &'static str {
        match self {
            OutputFilter::All => "all",
            OutputFilter::ErrorsOnly => "errors",
            OutputFilter::ToolCallsOnly => "tool calls",
            OutputFilter::FileChangesOnly => "file changes",
        }
    }

    fn title_suffix(self) -> &'static str {
        match self {
            OutputFilter::All => "",
            OutputFilter::ErrorsOnly => " errors",
            OutputFilter::ToolCallsOnly => " tool calls",
            OutputFilter::FileChangesOnly => " file changes",
        }
    }
}

fn looks_like_tool_call(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }

    const TOOL_PREFIXES: &[&str] = &[
        "tool ",
        "tool:",
        "[tool",
        "tool call",
        "calling tool",
        "running tool",
        "invoking tool",
        "using tool",
        "read(",
        "write(",
        "edit(",
        "multi_edit(",
        "bash(",
        "grep(",
        "glob(",
        "search(",
        "ls(",
        "apply_patch(",
    ];

    TOOL_PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
}

fn parse_spawn_request(input: &str) -> Result<SpawnRequest, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("spawn request cannot be empty".to_string());
    }

    if let Some(template_request) = parse_template_spawn_request(trimmed)? {
        return Ok(template_request);
    }

    let count = Regex::new(r"\b([1-9]\d*)\b")
        .expect("spawn count regex")
        .captures(trimmed)
        .and_then(|captures| captures.get(1))
        .and_then(|count| count.as_str().parse::<usize>().ok())
        .unwrap_or(1);

    let task = extract_spawn_task(trimmed);
    if task.is_empty() {
        return Err("spawn request must include a task description".to_string());
    }

    Ok(SpawnRequest::AdHoc {
        requested_count: count,
        task,
    })
}

fn parse_template_spawn_request(input: &str) -> Result<Option<SpawnRequest>, String> {
    let captures = Regex::new(
        r"(?is)^\s*template\s+(?P<name>[A-Za-z0-9_-]+)(?:\s+for\s+(?P<task>.*?))?(?:\s+with\s+(?P<vars>.+))?\s*$",
    )
    .expect("template spawn regex")
    .captures(input);

    let Some(captures) = captures else {
        return Ok(None);
    };

    let name = captures
        .name("name")
        .map(|value| value.as_str().trim().to_string())
        .ok_or_else(|| "template request must include a template name".to_string())?;
    let task = captures
        .name("task")
        .map(|value| value.as_str().trim().to_string())
        .filter(|value| !value.is_empty());
    let variables = captures
        .name("vars")
        .map(|value| parse_template_request_variables(value.as_str()))
        .transpose()?
        .unwrap_or_default();

    Ok(Some(SpawnRequest::Template {
        name,
        task,
        variables,
    }))
}

fn parse_template_request_variables(input: &str) -> Result<BTreeMap<String, String>, String> {
    let mut variables = BTreeMap::new();
    for entry in input
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| format!("template vars must use key=value form: {entry}"))?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            return Err(format!(
                "template vars must use non-empty key=value form: {entry}"
            ));
        }
        variables.insert(key.to_string(), value.to_string());
    }
    Ok(variables)
}

fn extract_spawn_task(input: &str) -> String {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();

    for marker in ["working on ", "work on ", "for ", ":"] {
        if let Some(start) = lower.find(marker) {
            let task = trimmed[start + marker.len()..]
                .trim_matches(|ch: char| ch.is_whitespace() || ch == ':' || ch == '-');
            if !task.is_empty() {
                return task.to_string();
            }
        }
    }

    let stripped =
        Regex::new(r"(?i)^\s*(give me|spawn|queue|start|launch)\s+\d+\s+(agents?|sessions?)\s*")
            .expect("spawn command regex")
            .replace(trimmed, "");
    let stripped = stripped.trim_matches(|ch: char| ch.is_whitespace() || ch == ':' || ch == '-');
    if !stripped.is_empty() && stripped != trimmed {
        return stripped.to_string();
    }

    trimmed.to_string()
}

fn expand_spawn_tasks(task: &str, count: usize) -> Vec<String> {
    if count <= 1 {
        return vec![task.to_string()];
    }

    (0..count)
        .map(|index| format!("{task} [{}/{}]", index + 1, count))
        .collect()
}

fn build_spawn_note(plan: &SpawnPlan, created_count: usize, queued_count: usize) -> String {
    let mut note = match plan {
        SpawnPlan::AdHoc {
            requested_count,
            spawn_count,
            task,
        } => {
            let task = truncate_for_dashboard(task, 72);
            if spawn_count < requested_count {
                format!(
                    "spawned {created_count} session(s) for {task} (requested {requested_count}, capped at {spawn_count})"
                )
            } else {
                format!("spawned {created_count} session(s) for {task}")
            }
        }
        SpawnPlan::Template {
            name,
            task,
            step_count,
            ..
        } => {
            let scope = task
                .as_ref()
                .map(|task| format!(" for {}", truncate_for_dashboard(task, 72)))
                .unwrap_or_default();
            format!("launched template {name} ({created_count}/{step_count} step(s)){scope}")
        }
    };

    if queued_count > 0 {
        note.push_str(&format!(" | {queued_count} pending worktree slot"));
    }

    note
}

fn post_spawn_selection_id(
    source_session_id: Option<&str>,
    created_ids: &[String],
) -> Option<String> {
    if created_ids.len() > 1 {
        source_session_id
            .map(ToOwned::to_owned)
            .or_else(|| created_ids.first().cloned())
    } else {
        created_ids.first().cloned()
    }
}

fn looks_like_file_change(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }

    if lower.contains("applied patch")
        || lower.contains("patch applied")
        || lower.starts_with("diff --git ")
    {
        return true;
    }

    const FILE_CHANGE_VERBS: &[&str] = &[
        "updated ",
        "created ",
        "deleted ",
        "renamed ",
        "modified ",
        "wrote ",
        "editing ",
        "edited ",
        "writing ",
    ];

    FILE_CHANGE_VERBS
        .iter()
        .any(|prefix| lower.starts_with(prefix) && contains_path_like_token(text))
}

fn contains_path_like_token(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let trimmed = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '[' | ']' | '(' | ')' | '{' | '}' | ',' | ':' | ';' | '"' | '\''
            )
        });

        trimmed.contains('/')
            || trimmed.contains('\\')
            || trimmed.starts_with("./")
            || trimmed.starts_with("../")
            || trimmed
                .rsplit_once('.')
                .map(|(stem, ext)| {
                    !stem.is_empty()
                        && !ext.is_empty()
                        && ext.len() <= 10
                        && ext.chars().all(|ch| ch.is_ascii_alphanumeric())
                })
                .unwrap_or(false)
    })
}

impl OutputTimeFilter {
    fn next(self) -> Self {
        match self {
            Self::AllTime => Self::Last15Minutes,
            Self::Last15Minutes => Self::LastHour,
            Self::LastHour => Self::Last24Hours,
            Self::Last24Hours => Self::AllTime,
        }
    }

    fn matches(self, line: &OutputLine) -> bool {
        match self {
            Self::AllTime => true,
            Self::Last15Minutes => line
                .occurred_at()
                .map(|timestamp| self.matches_timestamp(timestamp))
                .unwrap_or(false),
            Self::LastHour => line
                .occurred_at()
                .map(|timestamp| self.matches_timestamp(timestamp))
                .unwrap_or(false),
            Self::Last24Hours => line
                .occurred_at()
                .map(|timestamp| self.matches_timestamp(timestamp))
                .unwrap_or(false),
        }
    }

    fn matches_timestamp(self, timestamp: chrono::DateTime<Utc>) -> bool {
        match self {
            Self::AllTime => true,
            Self::Last15Minutes => timestamp >= Utc::now() - Duration::minutes(15),
            Self::LastHour => timestamp >= Utc::now() - Duration::hours(1),
            Self::Last24Hours => timestamp >= Utc::now() - Duration::hours(24),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::AllTime => "all time",
            Self::Last15Minutes => "last 15m",
            Self::LastHour => "last 1h",
            Self::Last24Hours => "last 24h",
        }
    }

    fn title_suffix(self) -> &'static str {
        match self {
            Self::AllTime => "",
            Self::Last15Minutes => " last 15m",
            Self::LastHour => " last 1h",
            Self::Last24Hours => " last 24h",
        }
    }
}

impl DiffViewMode {
    fn label(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Unified => "unified",
        }
    }

    fn title_suffix(self) -> &'static str {
        match self {
            Self::Split => " split",
            Self::Unified => " unified",
        }
    }
}

impl TimelineEventFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Lifecycle,
            Self::Lifecycle => Self::Messages,
            Self::Messages => Self::ToolCalls,
            Self::ToolCalls => Self::FileChanges,
            Self::FileChanges => Self::Decisions,
            Self::Decisions => Self::All,
        }
    }

    fn matches(self, event_type: TimelineEventType) -> bool {
        match self {
            Self::All => true,
            Self::Lifecycle => event_type == TimelineEventType::Lifecycle,
            Self::Messages => event_type == TimelineEventType::Message,
            Self::ToolCalls => event_type == TimelineEventType::ToolCall,
            Self::FileChanges => event_type == TimelineEventType::FileChange,
            Self::Decisions => event_type == TimelineEventType::Decision,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "all events",
            Self::Lifecycle => "lifecycle",
            Self::Messages => "messages",
            Self::ToolCalls => "tool calls",
            Self::FileChanges => "file changes",
            Self::Decisions => "decisions",
        }
    }

    fn title_suffix(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Lifecycle => " lifecycle",
            Self::Messages => " messages",
            Self::ToolCalls => " tool calls",
            Self::FileChanges => " file changes",
            Self::Decisions => " decisions",
        }
    }
}

impl GraphEntityFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Decisions,
            Self::Decisions => Self::Files,
            Self::Files => Self::Functions,
            Self::Functions => Self::Sessions,
            Self::Sessions => Self::All,
        }
    }

    fn entity_type(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Decisions => Some("decision"),
            Self::Files => Some("file"),
            Self::Functions => Some("function"),
            Self::Sessions => Some("session"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "all entities",
            Self::Decisions => "decisions",
            Self::Files => "files",
            Self::Functions => "functions",
            Self::Sessions => "sessions",
        }
    }

    fn title_suffix(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Decisions => " decisions",
            Self::Files => " files",
            Self::Functions => " functions",
            Self::Sessions => " sessions",
        }
    }
}

impl TimelineEventType {
    fn label(self) -> &'static str {
        match self {
            Self::Lifecycle => "lifecycle",
            Self::Message => "message",
            Self::ToolCall => "tool",
            Self::FileChange => "file-change",
            Self::Decision => "decision",
        }
    }
}

fn parse_rfc3339_to_utc(value: &str) -> Option<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

impl SearchScope {
    fn next(self) -> Self {
        match self {
            Self::SelectedSession => Self::AllSessions,
            Self::AllSessions => Self::SelectedSession,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SelectedSession => "selected session",
            Self::AllSessions => "all sessions",
        }
    }

    fn title_suffix(self) -> &'static str {
        match self {
            Self::SelectedSession => "",
            Self::AllSessions => " all sessions",
        }
    }

    fn matches(self, selected_session_id: Option<&str>, session_id: &str) -> bool {
        match self {
            Self::SelectedSession => selected_session_id == Some(session_id),
            Self::AllSessions => true,
        }
    }
}

impl SearchAgentFilter {
    fn matches(self, selected_agent_type: Option<&str>, session_agent_type: &str) -> bool {
        match self {
            Self::AllAgents => true,
            Self::SelectedAgentType => selected_agent_type == Some(session_agent_type),
        }
    }

    fn label(self, selected_agent_type: &str) -> String {
        match self {
            Self::AllAgents => "all agents".to_string(),
            Self::SelectedAgentType => format!("agent {}", selected_agent_type),
        }
    }

    fn title_suffix(self, selected_agent_type: &str) -> String {
        match self {
            Self::AllAgents => String::new(),
            Self::SelectedAgentType => format!(" {}", self.label(selected_agent_type)),
        }
    }
}

impl SessionSummary {
    fn from_sessions(
        sessions: &[Session],
        unread_message_counts: &HashMap<String, usize>,
        worktree_health_by_session: &HashMap<String, worktree::WorktreeHealth>,
        suppress_inbox_attention: bool,
    ) -> Self {
        let projects = sessions
            .iter()
            .map(|session| session.project.as_str())
            .collect::<HashSet<_>>()
            .len();
        let task_groups = sessions
            .iter()
            .map(|session| (session.project.as_str(), session.task_group.as_str()))
            .collect::<HashSet<_>>()
            .len();
        sessions.iter().fold(
            Self {
                total: sessions.len(),
                projects,
                task_groups,
                unread_messages: if suppress_inbox_attention {
                    0
                } else {
                    unread_message_counts.values().sum()
                },
                inbox_sessions: if suppress_inbox_attention {
                    0
                } else {
                    unread_message_counts
                        .values()
                        .filter(|count| **count > 0)
                        .count()
                },
                ..Self::default()
            },
            |mut summary, session| {
                match session.state {
                    SessionState::Pending => summary.pending += 1,
                    SessionState::Running => summary.running += 1,
                    SessionState::Idle => summary.idle += 1,
                    SessionState::Stale => summary.stale += 1,
                    SessionState::Completed => summary.completed += 1,
                    SessionState::Failed => summary.failed += 1,
                    SessionState::Stopped => summary.stopped += 1,
                }
                match worktree_health_by_session.get(&session.id).copied() {
                    Some(worktree::WorktreeHealth::Conflicted) => {
                        summary.conflicted_worktrees += 1;
                    }
                    Some(worktree::WorktreeHealth::InProgress) => {
                        summary.in_progress_worktrees += 1;
                    }
                    Some(worktree::WorktreeHealth::Clear) | None => {}
                }
                summary
            },
        )
    }
}

fn session_row(
    session: &Session,
    project_label: Option<String>,
    task_group_label: Option<String>,
    approval_requests: usize,
    unread_messages: usize,
) -> Row<'static> {
    let state_label = session_state_label(&session.state);
    let state_color = session_state_color(&session.state);
    Row::new(vec![
        Cell::from(format_session_id(&session.id)),
        Cell::from(project_label.unwrap_or_default()),
        Cell::from(task_group_label.unwrap_or_default()),
        Cell::from(session.agent_type.clone()),
        Cell::from(state_label).style(
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(session_branch(session)),
        Cell::from(if approval_requests == 0 {
            "-".to_string()
        } else {
            approval_requests.to_string()
        })
        .style(if approval_requests == 0 {
            Style::default()
        } else {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        }),
        Cell::from(if unread_messages == 0 {
            "-".to_string()
        } else {
            unread_messages.to_string()
        })
        .style(if unread_messages == 0 {
            Style::default()
        } else {
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD)
        }),
        Cell::from(session.metrics.tokens_used.to_string()),
        Cell::from(session.metrics.tool_calls.to_string()),
        Cell::from(session.metrics.files_changed.to_string()),
        Cell::from(format_duration(session.metrics.duration_secs)),
    ])
}

fn sort_sessions_for_display(sessions: &mut [Session]) {
    sessions.sort_by(|left, right| {
        left.project
            .cmp(&right.project)
            .then_with(|| left.task_group.cmp(&right.task_group))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn summary_line(summary: &SessionSummary) -> Line<'static> {
    let mut spans = vec![
        Span::styled(
            format!("Total {}  ", summary.total),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        summary_span("Projects", summary.projects, Color::Cyan),
        summary_span("Groups", summary.task_groups, Color::Magenta),
        summary_span("Running", summary.running, Color::Green),
        summary_span("Idle", summary.idle, Color::Yellow),
        summary_span("Stale", summary.stale, Color::LightRed),
        summary_span("Completed", summary.completed, Color::Blue),
        summary_span("Failed", summary.failed, Color::Red),
        summary_span("Stopped", summary.stopped, Color::DarkGray),
        summary_span("Pending", summary.pending, Color::Reset),
    ];

    if summary.conflicted_worktrees > 0 {
        spans.push(summary_span(
            "Conflicts",
            summary.conflicted_worktrees,
            Color::Red,
        ));
    }

    if summary.in_progress_worktrees > 0 {
        spans.push(summary_span(
            "Worktrees",
            summary.in_progress_worktrees,
            Color::Cyan,
        ));
    }

    Line::from(spans)
}

fn summary_span(label: &str, value: usize, color: Color) -> Span<'static> {
    Span::styled(
        format!("{label} {value}  "),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn attention_queue_line(summary: &SessionSummary, stabilized: bool) -> Line<'static> {
    if summary.failed == 0
        && summary.stopped == 0
        && summary.pending == 0
        && summary.stale == 0
        && summary.unread_messages == 0
        && summary.conflicted_worktrees == 0
    {
        return Line::from(vec![
            Span::styled(
                "Attention queue clear",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(if stabilized {
                "  stabilized backlog absorbed"
            } else {
                "  no failed, stopped, or pending sessions"
            }),
        ]);
    }

    let mut spans = vec![Span::styled(
        "Attention queue  ",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )];

    if summary.conflicted_worktrees > 0 {
        spans.push(summary_span(
            "Conflicts",
            summary.conflicted_worktrees,
            Color::Red,
        ));
    }

    spans.extend([
        summary_span("Stale", summary.stale, Color::LightRed),
        summary_span("Backlog", summary.unread_messages, Color::Magenta),
        summary_span("Failed", summary.failed, Color::Red),
        summary_span("Stopped", summary.stopped, Color::DarkGray),
        summary_span("Pending", summary.pending, Color::Yellow),
    ]);

    Line::from(spans)
}

fn approval_queue_line(approval_queue_counts: &HashMap<String, usize>) -> Line<'static> {
    let pending_sessions = approval_queue_counts.len();
    let pending_items: usize = approval_queue_counts.values().sum();

    if pending_items == 0 {
        return Line::from(vec![
            Span::styled(
                "Approval queue clear",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  no unanswered queries or conflicts"),
        ]);
    }

    Line::from(vec![
        Span::styled(
            "Approval queue  ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        summary_span("Pending", pending_items, Color::Yellow),
        summary_span("Sessions", pending_sessions, Color::Yellow),
    ])
}

fn approval_queue_preview_line(messages: &[SessionMessage]) -> Option<Line<'static>> {
    let message = messages.first()?;
    let preview = truncate_for_dashboard(&comms::preview(&message.msg_type, &message.content), 72);

    Some(Line::from(vec![
        Span::raw("- "),
        Span::styled(
            format_session_id(&message.to_session),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::raw(preview),
    ]))
}

fn truncate_for_dashboard(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let truncated: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}

fn configured_pane_size(cfg: &Config, layout: PaneLayout) -> u16 {
    let configured = match layout {
        PaneLayout::Horizontal | PaneLayout::Vertical => cfg.linear_pane_size_percent,
        PaneLayout::Grid => cfg.grid_pane_size_percent,
    };

    configured.clamp(MIN_PANE_SIZE_PERCENT, MAX_PANE_SIZE_PERCENT)
}

fn recommended_spawn_layout(live_session_count: usize) -> PaneLayout {
    if live_session_count >= 3 {
        PaneLayout::Grid
    } else {
        PaneLayout::Vertical
    }
}

fn pane_layout_name(layout: PaneLayout) -> &'static str {
    match layout {
        PaneLayout::Horizontal => "horizontal",
        PaneLayout::Vertical => "vertical",
        PaneLayout::Grid => "grid",
    }
}

fn horizontal_detail_layout(area: Rect, panes: &[Pane]) -> Vec<(Pane, Rect)> {
    match panes {
        [] => Vec::new(),
        [pane] => vec![(*pane, area)],
        [first, second] => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(OUTPUT_PANE_PERCENT),
                    Constraint::Percentage(100 - OUTPUT_PANE_PERCENT),
                ])
                .split(area);
            vec![(*first, rows[0]), (*second, rows[1])]
        }
        _ => unreachable!("horizontal layouts support at most two detail panes"),
    }
}

fn vertical_detail_layout(area: Rect, panes: &[Pane]) -> Vec<(Pane, Rect)> {
    match panes {
        [] => Vec::new(),
        [pane] => vec![(*pane, area)],
        [first, second] => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(OUTPUT_PANE_PERCENT),
                    Constraint::Percentage(100 - OUTPUT_PANE_PERCENT),
                ])
                .split(area);
            vec![(*first, columns[0]), (*second, columns[1])]
        }
        _ => unreachable!("vertical layouts support at most two detail panes"),
    }
}

fn compile_search_regex(query: &str) -> Result<Regex, regex::Error> {
    Regex::new(query)
}

fn highlight_output_line(
    text: &str,
    query: &str,
    is_current_match: bool,
    palette: ThemePalette,
) -> Line<'static> {
    if query.is_empty() {
        return Line::from(text.to_string());
    }

    let Ok(regex) = compile_search_regex(query) else {
        return Line::from(text.to_string());
    };

    let mut spans = Vec::new();
    let mut cursor = 0;
    for matched in regex.find_iter(text) {
        let start = matched.start();
        let end = matched.end();

        if start > cursor {
            spans.push(Span::raw(text[cursor..start].to_string()));
        }

        let match_style = if is_current_match {
            Style::default()
                .bg(palette.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(Color::Yellow).fg(Color::Black)
        };
        spans.push(Span::styled(text[start..end].to_string(), match_style));
        cursor = end;
    }

    if cursor < text.len() {
        spans.push(Span::raw(text[cursor..].to_string()));
    }

    if spans.is_empty() {
        Line::from(text.to_string())
    } else {
        Line::from(spans)
    }
}

fn build_worktree_diff_columns(patch: &str, palette: ThemePalette) -> WorktreeDiffColumns {
    let mut removals = Vec::new();
    let mut additions = Vec::new();
    let mut hunk_offsets = Vec::new();
    let mut pending_removals = Vec::new();
    let mut pending_additions = Vec::new();

    for line in patch.lines() {
        if is_diff_removal_line(line) {
            pending_removals.push(line[1..].to_string());
            continue;
        }

        if is_diff_addition_line(line) {
            pending_additions.push(line[1..].to_string());
            continue;
        }

        flush_split_diff_change_block(
            &mut removals,
            &mut additions,
            &mut pending_removals,
            &mut pending_additions,
            palette,
        );

        if line.is_empty() {
            continue;
        }

        if line.starts_with("@@") {
            hunk_offsets.push(removals.len().max(additions.len()));
        }

        let styled_line = if line.starts_with(' ') {
            styled_diff_context_line(line, palette)
        } else {
            styled_diff_meta_line(split_diff_display_line(line), palette)
        };
        removals.push(styled_line.clone());
        additions.push(styled_line);
    }

    flush_split_diff_change_block(
        &mut removals,
        &mut additions,
        &mut pending_removals,
        &mut pending_additions,
        palette,
    );

    WorktreeDiffColumns {
        removals: if removals.is_empty() {
            Text::from("No removals in this bounded preview.")
        } else {
            Text::from(removals)
        },
        additions: if additions.is_empty() {
            Text::from("No additions in this bounded preview.")
        } else {
            Text::from(additions)
        },
        hunk_offsets,
    }
}

fn build_unified_diff_text(patch: &str, palette: ThemePalette) -> Text<'static> {
    let mut lines = Vec::new();
    let mut pending_removals = Vec::new();
    let mut pending_additions = Vec::new();

    for line in patch.lines() {
        if is_diff_removal_line(line) {
            pending_removals.push(line[1..].to_string());
            continue;
        }

        if is_diff_addition_line(line) {
            pending_additions.push(line[1..].to_string());
            continue;
        }

        flush_unified_diff_change_block(
            &mut lines,
            &mut pending_removals,
            &mut pending_additions,
            palette,
        );

        if line.is_empty() {
            continue;
        }

        lines.push(if line.starts_with(' ') {
            styled_diff_context_line(line, palette)
        } else {
            styled_diff_meta_line(line, palette)
        });
    }

    flush_unified_diff_change_block(
        &mut lines,
        &mut pending_removals,
        &mut pending_additions,
        palette,
    );

    Text::from(lines)
}

fn build_unified_diff_hunk_offsets(patch: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut rendered_index = 0usize;
    let mut pending_removals = 0usize;
    let mut pending_additions = 0usize;

    for line in patch.lines() {
        if is_diff_removal_line(line) {
            pending_removals += 1;
            continue;
        }

        if is_diff_addition_line(line) {
            pending_additions += 1;
            continue;
        }

        if pending_removals > 0 || pending_additions > 0 {
            rendered_index += pending_removals + pending_additions;
            pending_removals = 0;
            pending_additions = 0;
        }

        if line.is_empty() {
            continue;
        }

        if line.starts_with("@@") {
            offsets.push(rendered_index);
        }
        rendered_index += 1;
    }

    offsets
}

fn flush_split_diff_change_block(
    removals: &mut Vec<Line<'static>>,
    additions: &mut Vec<Line<'static>>,
    pending_removals: &mut Vec<String>,
    pending_additions: &mut Vec<String>,
    palette: ThemePalette,
) {
    let pair_count = pending_removals.len().max(pending_additions.len());
    for index in 0..pair_count {
        match (pending_removals.get(index), pending_additions.get(index)) {
            (Some(removal), Some(addition)) => {
                let (removal_mask, addition_mask) =
                    diff_word_change_masks(removal.as_str(), addition.as_str());
                removals.push(styled_diff_change_line(
                    '-',
                    removal,
                    &removal_mask,
                    diff_removal_style(palette),
                    diff_removal_word_style(),
                ));
                additions.push(styled_diff_change_line(
                    '+',
                    addition,
                    &addition_mask,
                    diff_addition_style(palette),
                    diff_addition_word_style(),
                ));
            }
            (Some(removal), None) => {
                removals.push(styled_diff_change_line(
                    '-',
                    removal,
                    &vec![false; tokenize_diff_words(removal).len()],
                    diff_removal_style(palette),
                    diff_removal_word_style(),
                ));
                additions.push(Line::from(""));
            }
            (None, Some(addition)) => {
                removals.push(Line::from(""));
                additions.push(styled_diff_change_line(
                    '+',
                    addition,
                    &vec![false; tokenize_diff_words(addition).len()],
                    diff_addition_style(palette),
                    diff_addition_word_style(),
                ));
            }
            (None, None) => {}
        }
    }

    pending_removals.clear();
    pending_additions.clear();
}

fn flush_unified_diff_change_block(
    lines: &mut Vec<Line<'static>>,
    pending_removals: &mut Vec<String>,
    pending_additions: &mut Vec<String>,
    palette: ThemePalette,
) {
    let pair_count = pending_removals.len().max(pending_additions.len());
    for index in 0..pair_count {
        match (pending_removals.get(index), pending_additions.get(index)) {
            (Some(removal), Some(addition)) => {
                let (removal_mask, addition_mask) =
                    diff_word_change_masks(removal.as_str(), addition.as_str());
                lines.push(styled_diff_change_line(
                    '-',
                    removal,
                    &removal_mask,
                    diff_removal_style(palette),
                    diff_removal_word_style(),
                ));
                lines.push(styled_diff_change_line(
                    '+',
                    addition,
                    &addition_mask,
                    diff_addition_style(palette),
                    diff_addition_word_style(),
                ));
            }
            (Some(removal), None) => lines.push(styled_diff_change_line(
                '-',
                removal,
                &vec![false; tokenize_diff_words(removal).len()],
                diff_removal_style(palette),
                diff_removal_word_style(),
            )),
            (None, Some(addition)) => lines.push(styled_diff_change_line(
                '+',
                addition,
                &vec![false; tokenize_diff_words(addition).len()],
                diff_addition_style(palette),
                diff_addition_word_style(),
            )),
            (None, None) => {}
        }
    }

    pending_removals.clear();
    pending_additions.clear();
}

fn split_diff_display_line(line: &str) -> String {
    if line.starts_with("--- ") && !line.starts_with("--- a/") {
        return line.to_string();
    }

    if let Some(path) = line.strip_prefix("--- a/") {
        return format!("File {path}");
    }

    if let Some(path) = line.strip_prefix("+++ b/") {
        return format!("File {path}");
    }

    line.to_string()
}

fn is_diff_removal_line(line: &str) -> bool {
    line.starts_with('-') && !line.starts_with("--- ")
}

fn is_diff_addition_line(line: &str) -> bool {
    line.starts_with('+') && !line.starts_with("+++ ")
}

fn styled_diff_meta_line(text: impl Into<String>, palette: ThemePalette) -> Line<'static> {
    Line::from(vec![Span::styled(text.into(), diff_meta_style(palette))])
}

fn styled_diff_context_line(text: &str, palette: ThemePalette) -> Line<'static> {
    Line::from(vec![Span::styled(
        text.to_string(),
        diff_context_style(palette),
    )])
}

fn styled_diff_change_line(
    prefix: char,
    body: &str,
    change_mask: &[bool],
    base_style: Style,
    changed_style: Style,
) -> Line<'static> {
    let tokens = tokenize_diff_words(body);
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        base_style.add_modifier(Modifier::BOLD),
    )];

    for (index, token) in tokens.into_iter().enumerate() {
        let style = if change_mask.get(index).copied().unwrap_or(false) {
            changed_style
        } else {
            base_style
        };
        spans.push(Span::styled(token, style));
    }

    Line::from(spans)
}

fn tokenize_diff_words(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_is_whitespace: Option<bool> = None;

    for ch in text.chars() {
        let is_whitespace = ch.is_whitespace();
        match current_is_whitespace {
            Some(state) if state == is_whitespace => current.push(ch),
            Some(_) => {
                tokens.push(std::mem::take(&mut current));
                current.push(ch);
                current_is_whitespace = Some(is_whitespace);
            }
            None => {
                current.push(ch);
                current_is_whitespace = Some(is_whitespace);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn diff_word_change_masks(left: &str, right: &str) -> (Vec<bool>, Vec<bool>) {
    let left_tokens = tokenize_diff_words(left);
    let right_tokens = tokenize_diff_words(right);
    let left_len = left_tokens.len();
    let right_len = right_tokens.len();
    let mut lcs = vec![vec![0usize; right_len + 1]; left_len + 1];

    for left_index in (0..left_len).rev() {
        for right_index in (0..right_len).rev() {
            lcs[left_index][right_index] = if left_tokens[left_index] == right_tokens[right_index] {
                lcs[left_index + 1][right_index + 1] + 1
            } else {
                lcs[left_index + 1][right_index].max(lcs[left_index][right_index + 1])
            };
        }
    }

    let mut left_changed = vec![true; left_len];
    let mut right_changed = vec![true; right_len];
    let (mut left_index, mut right_index) = (0usize, 0usize);
    while left_index < left_len && right_index < right_len {
        if left_tokens[left_index] == right_tokens[right_index] {
            left_changed[left_index] = false;
            right_changed[right_index] = false;
            left_index += 1;
            right_index += 1;
        } else if lcs[left_index + 1][right_index] >= lcs[left_index][right_index + 1] {
            left_index += 1;
        } else {
            right_index += 1;
        }
    }

    (left_changed, right_changed)
}

fn diff_meta_style(palette: ThemePalette) -> Style {
    Style::default()
        .fg(palette.accent)
        .add_modifier(Modifier::BOLD)
}

fn diff_context_style(palette: ThemePalette) -> Style {
    Style::default().fg(palette.muted)
}

fn diff_removal_style(palette: ThemePalette) -> Style {
    let color = match palette.accent {
        Color::Blue => Color::Red,
        _ => Color::LightRed,
    };
    Style::default().fg(color)
}

fn diff_addition_style(palette: ThemePalette) -> Style {
    let color = match palette.accent {
        Color::Blue => Color::Green,
        _ => Color::LightGreen,
    };
    Style::default().fg(color)
}

fn diff_removal_word_style() -> Style {
    Style::default()
        .bg(Color::Red)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD)
}

fn diff_addition_word_style() -> Style {
    Style::default()
        .bg(Color::Green)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD)
}

fn board_lane_label(state: &SessionState) -> &'static str {
    match state {
        SessionState::Pending => "Inbox",
        SessionState::Running => "In Progress",
        SessionState::Idle => "Review",
        SessionState::Stale | SessionState::Failed => "Blocked",
        SessionState::Completed => "Done",
        SessionState::Stopped => "Stopped",
    }
}

fn session_state_label(state: &SessionState) -> &'static str {
    match state {
        SessionState::Pending => "Pending",
        SessionState::Running => "Running",
        SessionState::Idle => "Idle",
        SessionState::Stale => "Stale",
        SessionState::Completed => "Completed",
        SessionState::Failed => "Failed",
        SessionState::Stopped => "Stopped",
    }
}

fn session_state_color(state: &SessionState) -> Color {
    match state {
        SessionState::Running => Color::Green,
        SessionState::Idle => Color::Yellow,
        SessionState::Stale => Color::LightRed,
        SessionState::Failed => Color::Red,
        SessionState::Stopped => Color::DarkGray,
        SessionState::Completed => Color::Blue,
        SessionState::Pending => Color::Reset,
    }
}

fn board_codename(session: &Session) -> String {
    const ADJECTIVES: &[&str] = &[
        "Amber", "Cinder", "Moss", "Nova", "Sable", "Slate", "Swift", "Talon",
    ];
    const NOUNS: &[&str] = &[
        "Fox", "Kite", "Lynx", "Otter", "Rook", "Sprite", "Wisp", "Wolf",
    ];

    let seed = session
        .id
        .bytes()
        .fold(0usize, |acc, byte| acc.wrapping_mul(33).wrapping_add(byte as usize));
    format!(
        "{} {}",
        ADJECTIVES[seed % ADJECTIVES.len()],
        NOUNS[(seed / ADJECTIVES.len()) % NOUNS.len()]
    )
}

fn file_activity_summary(entry: &FileActivityEntry) -> String {
    let mut summary = format!(
        "{} {}",
        file_activity_verb(entry.action.clone()),
        truncate_for_dashboard(&entry.path, 72)
    );

    if let Some(diff_preview) = entry.diff_preview.as_ref() {
        summary.push_str(" | ");
        summary.push_str(&truncate_for_dashboard(diff_preview, 56));
    }

    summary
}

fn file_activity_patch_lines(entry: &FileActivityEntry, max_lines: usize) -> Vec<String> {
    entry
        .patch_preview
        .as_deref()
        .map(|patch| {
            patch
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && *line != "@@" && *line != "+" && *line != "-")
                .take(max_lines)
                .map(|line| truncate_for_dashboard(line, 72))
                .collect()
        })
        .unwrap_or_default()
}

fn file_overlap_summary(entry: &FileActivityOverlap, timestamp: &str) -> String {
    format!(
        "{} {} | {} {} as {} | {}",
        file_activity_verb(entry.current_action.clone()),
        truncate_for_dashboard(&entry.path, 48),
        entry.other_session_state,
        format_session_id(&entry.other_session_id),
        file_activity_verb(entry.other_action.clone()),
        timestamp
    )
}

fn conflict_incident_summary(
    incident: &crate::session::store::ConflictIncident,
    timestamp: &str,
) -> String {
    format!(
        "{} {} | active {} | paused {} | {}",
        timestamp,
        truncate_for_dashboard(&incident.path, 48),
        format_session_id(&incident.active_session_id),
        format_session_id(&incident.paused_session_id),
        incident.strategy.replace('_', "-")
    )
}

fn decision_log_summary(entry: &DecisionLogEntry) -> String {
    format!("decided {}", truncate_for_dashboard(&entry.decision, 72))
}

fn decision_log_detail_lines(entry: &DecisionLogEntry) -> Vec<String> {
    let mut lines = vec![format!(
        "why {}",
        truncate_for_dashboard(&entry.reasoning, 72)
    )];
    if entry.alternatives.is_empty() {
        lines.push("alternatives none recorded".to_string());
    } else {
        for alternative in entry.alternatives.iter().take(3) {
            lines.push(format!(
                "alternative {}",
                truncate_for_dashboard(alternative, 72)
            ));
        }
    }
    lines
}

fn tool_log_detail_lines(entry: &ToolLogEntry) -> Vec<String> {
    let mut lines = Vec::new();
    if !entry.trigger_summary.trim().is_empty() {
        lines.push(format!(
            "why {}",
            truncate_for_dashboard(&entry.trigger_summary, 72)
        ));
    }
    if entry.input_params_json.trim() != "{}" {
        lines.push(format!(
            "params {}",
            truncate_for_dashboard(&entry.input_params_json, 72)
        ));
    }
    lines
}

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

fn summarize_test_runs(
    tool_logs: &[ToolLogEntry],
    assume_success_on_completion: bool,
) -> TestRunSummary {
    let mut summary = TestRunSummary::default();

    for entry in tool_logs {
        if !tool_log_looks_like_test(entry) {
            continue;
        }

        summary.total += 1;
        let failed = tool_log_looks_failed(entry);
        let passed = tool_log_looks_passed(entry);
        if !failed && (passed || assume_success_on_completion) {
            summary.passed += 1;
        }
    }

    summary
}

fn tool_log_looks_like_test(entry: &ToolLogEntry) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        entry.tool_name,
        entry.input_summary,
        extract_tool_command(entry),
        entry.output_summary
    )
    .to_ascii_lowercase();
    const TEST_MARKERS: &[&str] = &[
        "cargo test",
        "npm test",
        "pnpm test",
        "pnpm exec vitest",
        "pnpm exec playwright",
        "yarn test",
        "bun test",
        "vitest",
        "jest",
        "pytest",
        "go test",
        "playwright test",
        "cypress",
        "rspec",
        "phpunit",
        "e2e",
    ];

    TEST_MARKERS.iter().any(|marker| haystack.contains(marker))
}

fn tool_log_looks_failed(entry: &ToolLogEntry) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        entry.tool_name,
        entry.input_summary,
        extract_tool_command(entry),
        entry.output_summary
    )
    .to_ascii_lowercase();
    const FAILURE_MARKERS: &[&str] = &[
        " fail",
        "failed",
        " error",
        "panic",
        "timed out",
        "non-zero",
        "exit code 1",
        "exited with",
    ];

    FAILURE_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
}

fn tool_log_looks_passed(entry: &ToolLogEntry) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        entry.tool_name,
        entry.input_summary,
        extract_tool_command(entry),
        entry.output_summary
    )
    .to_ascii_lowercase();
    const SUCCESS_MARKERS: &[&str] = &[" pass", "passed", " ok", "success", "green", "completed"];

    SUCCESS_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
}

fn extract_tool_command(entry: &ToolLogEntry) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&entry.input_params_json) else {
        return String::new();
    };

    value
        .get("command")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_default()
}

fn recent_completion_files(file_activity: &[FileActivityEntry], files_changed: u32) -> Vec<String> {
    if !file_activity.is_empty() {
        return file_activity
            .iter()
            .take(3)
            .map(file_activity_summary)
            .collect();
    }

    if files_changed > 0 {
        return vec![format!("files touched {}", files_changed)];
    }

    Vec::new()
}

fn summarize_completion_decisions(
    tool_logs: &[ToolLogEntry],
    file_activity: &[FileActivityEntry],
    session_task: &str,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut decisions = Vec::new();

    for entry in tool_logs.iter().rev() {
        let mut candidates = Vec::new();
        if !entry.trigger_summary.trim().is_empty()
            && entry.trigger_summary.trim() != session_task.trim()
        {
            candidates.push(format!(
                "why {}",
                truncate_for_dashboard(&entry.trigger_summary, 72)
            ));
        }

        let action = if entry.tool_name.eq_ignore_ascii_case("Bash") {
            truncate_for_dashboard(&extract_tool_command(entry), 72)
        } else if !entry.output_summary.trim().is_empty() && entry.output_summary.trim() != "ok" {
            truncate_for_dashboard(&entry.output_summary, 72)
        } else {
            truncate_for_dashboard(&entry.input_summary, 72)
        };

        if !action.trim().is_empty() {
            candidates.push(action);
        }

        for candidate in candidates {
            let normalized = candidate.to_ascii_lowercase();
            if seen.insert(normalized) {
                decisions.push(candidate);
            }
            if decisions.len() >= 3 {
                return decisions;
            }
        }
    }

    for entry in file_activity.iter().take(3) {
        let candidate = file_activity_summary(entry);
        let normalized = candidate.to_ascii_lowercase();
        if seen.insert(normalized) {
            decisions.push(candidate);
        }
        if decisions.len() >= 3 {
            break;
        }
    }

    decisions
}

fn summarize_completion_warnings(
    session: &Session,
    tool_logs: &[ToolLogEntry],
    tests: &TestRunSummary,
    worktree_health: Option<&worktree::WorktreeHealth>,
    approval_backlog: usize,
    overlap_count: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let high_risk_tool_calls = tool_logs
        .iter()
        .filter(|entry| entry.risk_score >= Config::RISK_THRESHOLDS.review)
        .count();

    if session.metrics.files_changed > 0 && tests.total == 0 {
        warnings.push("no test runs detected".to_string());
    }
    if tests.total > tests.passed {
        warnings.push(format!(
            "{} detected test run(s) were not confirmed passed",
            tests.total - tests.passed
        ));
    }
    if high_risk_tool_calls > 0 {
        warnings.push(format!(
            "{high_risk_tool_calls} high-risk tool call(s) recorded"
        ));
    }
    if approval_backlog > 0 {
        warnings.push(format!(
            "{approval_backlog} approval/conflict request(s) remained unread"
        ));
    }
    if overlap_count > 0 {
        warnings.push(format!(
            "{overlap_count} potential file overlap(s) remained"
        ));
    }
    match worktree_health {
        Some(worktree::WorktreeHealth::Conflicted) => {
            warnings.push("worktree still has unresolved conflicts".to_string());
        }
        Some(worktree::WorktreeHealth::InProgress) => {
            warnings.push("worktree still has unmerged changes".to_string());
        }
        Some(worktree::WorktreeHealth::Clear) | None => {}
    }

    warnings
}

fn completion_summary_observation_details(
    summary: &SessionCompletionSummary,
    session: &Session,
) -> BTreeMap<String, String> {
    let mut details = BTreeMap::new();
    details.insert("state".to_string(), session.state.to_string());
    details.insert(
        "files_changed".to_string(),
        summary.files_changed.to_string(),
    );
    details.insert("tokens_used".to_string(), summary.tokens_used.to_string());
    details.insert(
        "duration_secs".to_string(),
        summary.duration_secs.to_string(),
    );
    details.insert("cost_usd".to_string(), format!("{:.4}", summary.cost_usd));
    details.insert("tests_run".to_string(), summary.tests_run.to_string());
    details.insert("tests_passed".to_string(), summary.tests_passed.to_string());
    if !summary.recent_files.is_empty() {
        details.insert("recent_files".to_string(), summary.recent_files.join(" | "));
    }
    if !summary.key_decisions.is_empty() {
        details.insert(
            "key_decisions".to_string(),
            summary.key_decisions.join(" | "),
        );
    }
    if !summary.warnings.is_empty() {
        details.insert("warnings".to_string(), summary.warnings.join(" | "));
    }
    details
}

fn session_started_webhook_body(session: &Session, compare_url: Option<&str>) -> String {
    let mut lines = vec![
        "*ECC 2.0: Session started*".to_string(),
        format!(
            "`{}` {}",
            format_session_id(&session.id),
            truncate_for_dashboard(&session.task, 96)
        ),
        format!(
            "Project `{}` | Group `{}` | Agent `{}`",
            session.project, session.task_group, session.agent_type
        ),
    ];

    if let Some(worktree) = session.worktree.as_ref() {
        lines.push(format!(
            "```text\nbranch: {}\nbase: {}\nworktree: {}\n```",
            worktree.branch,
            worktree.base_branch,
            worktree.path.display()
        ));
    }

    if let Some(compare_url) = compare_url {
        lines.push(format!("PR / compare: {compare_url}"));
    }

    lines.join("\n")
}

fn completion_summary_webhook_body(
    summary: &SessionCompletionSummary,
    session: &Session,
    compare_url: Option<&str>,
) -> String {
    let mut lines = vec![
        format!("*{}*", summary.title()),
        format!(
            "`{}` {}",
            format_session_id(&summary.session_id),
            truncate_for_dashboard(&summary.task, 96)
        ),
        format!(
            "Project `{}` | Group `{}` | State `{}`",
            session.project, session.task_group, session.state
        ),
        format!(
            "Duration `{}` | Files `{}` | Tokens `{}` | Cost `{}`",
            format_duration(summary.duration_secs),
            summary.files_changed,
            format_token_count(summary.tokens_used),
            format_currency(summary.cost_usd)
        ),
        if summary.tests_run > 0 {
            format!(
                "Tests `{}` run / `{}` passed",
                summary.tests_run, summary.tests_passed
            )
        } else {
            "Tests `not detected`".to_string()
        },
    ];

    if !summary.recent_files.is_empty() {
        lines.push(markdown_code_block("Recent files", &summary.recent_files));
    }

    if !summary.key_decisions.is_empty() {
        lines.push(markdown_code_block("Key decisions", &summary.key_decisions));
    }

    if !summary.warnings.is_empty() {
        lines.push(markdown_code_block("Warnings", &summary.warnings));
    }

    if let Some(compare_url) = compare_url {
        lines.push(format!("PR / compare: {compare_url}"));
    }

    lines.join("\n")
}

fn budget_alert_webhook_body(
    summary_suffix: &str,
    token_budget: &str,
    cost_budget: &str,
    active_sessions: usize,
) -> String {
    [
        "*ECC 2.0: Budget alert*".to_string(),
        summary_suffix.to_string(),
        format!("Tokens `{token_budget}`"),
        format!("Cost `{cost_budget}`"),
        format!("Active sessions `{active_sessions}`"),
    ]
    .join("\n")
}

fn approval_request_webhook_body(message: &SessionMessage, preview: &str) -> String {
    [
        "*ECC 2.0: Approval needed*".to_string(),
        format!(
            "To `{}` from `{}`",
            format_session_id(&message.to_session),
            format_session_id(&message.from_session)
        ),
        format!("Type `{}`", message.msg_type),
        markdown_code_block("Request", &[preview.to_string()]),
    ]
    .join("\n")
}

fn markdown_code_block(label: &str, lines: &[String]) -> String {
    format!("{label}\n```text\n{}\n```", lines.join("\n"))
}

fn session_compare_url(session: &Session) -> Option<String> {
    session
        .worktree
        .as_ref()
        .and_then(|worktree| worktree::github_compare_url(worktree).ok().flatten())
}

fn file_activity_verb(action: crate::session::FileActivityAction) -> &'static str {
    match action {
        crate::session::FileActivityAction::Read => "read",
        crate::session::FileActivityAction::Create => "create",
        crate::session::FileActivityAction::Modify => "modify",
        crate::session::FileActivityAction::Move => "move",
        crate::session::FileActivityAction::Delete => "delete",
        crate::session::FileActivityAction::Touch => "touch",
    }
}

fn heartbeat_enforcement_note(outcome: &manager::HeartbeatEnforcementOutcome) -> String {
    if !outcome.auto_terminated_sessions.is_empty() {
        return format!(
            "stale heartbeat detected | auto-terminated {} session(s)",
            outcome.auto_terminated_sessions.len()
        );
    }

    format!(
        "stale heartbeat detected | flagged {} session(s) for attention",
        outcome.stale_sessions.len()
    )
}

fn budget_auto_pause_note(outcome: &manager::BudgetEnforcementOutcome) -> String {
    let cause = match (
        outcome.token_budget_exceeded,
        outcome.cost_budget_exceeded,
        outcome.profile_token_budget_exceeded,
    ) {
        (true, true, _) => "token and cost budgets exceeded",
        (true, false, _) => "token budget exceeded",
        (false, true, _) => "cost budget exceeded",
        (false, false, true) => "profile token budget exceeded",
        (false, false, false) => "budget exceeded",
    };

    format!(
        "{cause} | auto-paused {} active session(s)",
        outcome.paused_sessions.len()
    )
}

fn conflict_enforcement_note(outcome: &manager::ConflictEnforcementOutcome) -> String {
    let strategy = match outcome.strategy {
        crate::config::ConflictResolutionStrategy::Escalate => "escalation",
        crate::config::ConflictResolutionStrategy::LastWriteWins => "last-write-wins",
        crate::config::ConflictResolutionStrategy::Merge => "merge review",
    };

    format!(
        "file conflict detected | opened {} incident(s), auto-paused {} session(s) via {}",
        outcome.created_incidents,
        outcome.paused_sessions.len(),
        strategy
    )
}

fn format_session_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn build_conflict_protocol(
    session_id: &str,
    worktree: &crate::session::WorktreeInfo,
    merge_readiness: &worktree::MergeReadiness,
) -> Option<String> {
    if merge_readiness.status != worktree::MergeReadinessStatus::Conflicted {
        return None;
    }

    let mut lines = vec![
        format!("Conflict protocol for {}", format_session_id(session_id)),
        format!("Worktree {}", worktree.path.display()),
        format!("Branch {} (base {})", worktree.branch, worktree.base_branch),
        merge_readiness.summary.clone(),
    ];

    if !merge_readiness.conflicts.is_empty() {
        lines.push("Conflicts".to_string());
        for conflict in &merge_readiness.conflicts {
            lines.push(format!("- {conflict}"));
        }
    }

    lines.push("Resolution steps".to_string());
    lines.push(format!(
        "1. Inspect current patch: ecc worktree-status {session_id} --patch"
    ));
    lines.push(format!("2. Open worktree: cd {}", worktree.path.display()));
    lines.push("3. Resolve conflicts and stage files: git add <paths>".to_string());
    lines.push(format!(
        "4. Commit the resolution on {}: git commit",
        worktree.branch
    ));
    lines.push(format!(
        "5. Re-check readiness: ecc worktree-status {session_id} --check"
    ));
    lines.push(format!(
        "6. Merge when clear: ecc merge-worktree {session_id}"
    ));

    Some(lines.join("\n"))
}

fn build_session_conflict_protocol(
    session_id: &str,
    incidents: &[crate::session::store::ConflictIncident],
) -> Option<String> {
    if incidents.is_empty() {
        return None;
    }

    let mut lines = vec![
        format!("Conflict protocol for {}", format_session_id(session_id)),
        "Session overlap incidents".to_string(),
    ];

    for incident in incidents {
        lines.push(format!(
            "- {}",
            conflict_incident_summary(
                incident,
                &incident.updated_at.format("%H:%M:%S").to_string()
            )
        ));
        lines.push(format!("  {}", incident.summary));
    }

    lines.push("Resolution steps".to_string());
    lines.push("1. Inspect the affected session output and recent file activity".to_string());
    lines.push(
        "2. Decide whether to keep the active session, reassign, or merge changes manually"
            .to_string(),
    );
    lines.push(format!(
        "3. Resume the paused session only after reviewing the overlap: ecc resume {}",
        session_id
    ));

    Some(lines.join("\n"))
}

fn assignment_action_label(action: manager::AssignmentAction) -> &'static str {
    match action {
        manager::AssignmentAction::Spawned => "spawned",
        manager::AssignmentAction::ReusedIdle => "reused idle",
        manager::AssignmentAction::ReusedActive => "reused active",
        manager::AssignmentAction::DeferredSaturated => "deferred saturated",
    }
}

fn parse_pr_prompt(input: &str) -> std::result::Result<PrPromptSpec, String> {
    let mut segments = input.split('|').map(str::trim);
    let title = segments.next().unwrap_or_default().trim().to_string();
    if title.is_empty() {
        return Err("missing PR title".to_string());
    }

    let mut request = PrPromptSpec {
        title,
        base_branch: None,
        labels: Vec::new(),
        reviewers: Vec::new(),
    };

    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let (key, value) = segment
            .split_once('=')
            .ok_or_else(|| format!("expected key=value segment, got `{segment}`"))?;
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "base" => {
                if value.is_empty() {
                    return Err("base branch cannot be empty".to_string());
                }
                request.base_branch = Some(value.to_string());
            }
            "labels" | "label" => {
                request.labels = value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            "reviewers" | "reviewer" => {
                request.reviewers = value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            _ => return Err(format!("unsupported PR field `{key}`")),
        }
    }

    Ok(request)
}

fn delegate_worktree_health_label(health: worktree::WorktreeHealth) -> &'static str {
    match health {
        worktree::WorktreeHealth::Clear => "clear",
        worktree::WorktreeHealth::InProgress => "in progress",
        worktree::WorktreeHealth::Conflicted => "conflicted",
    }
}

fn delegate_next_action(delegate: &DelegatedChildSummary) -> &'static str {
    if delegate.worktree_health == Some(worktree::WorktreeHealth::Conflicted) {
        return "resolve conflict";
    }
    if delegate.approval_backlog > 0 {
        return "review approvals";
    }
    if delegate.handoff_backlog > 0 && delegate.state == SessionState::Idle {
        return "process handoff";
    }
    if delegate.handoff_backlog > 0 {
        return "drain backlog";
    }
    if delegate.worktree_health == Some(worktree::WorktreeHealth::InProgress) {
        return "finish worktree changes";
    }
    match delegate.state {
        SessionState::Pending => "wait for startup",
        SessionState::Running => "let it run",
        SessionState::Idle => "assign next task",
        SessionState::Stale => "inspect stale heartbeat",
        SessionState::Failed => "inspect failure",
        SessionState::Stopped => "resume or reassign",
        SessionState::Completed => "merge or cleanup",
    }
}

fn delegate_attention_priority(delegate: &DelegatedChildSummary) -> u8 {
    if delegate.worktree_health == Some(worktree::WorktreeHealth::Conflicted) {
        return 0;
    }
    if delegate.approval_backlog > 0 {
        return 1;
    }
    if matches!(
        delegate.state,
        SessionState::Stale | SessionState::Failed | SessionState::Stopped
    ) {
        return 2;
    }
    if delegate.handoff_backlog > 0 {
        return 3;
    }
    if delegate.worktree_health == Some(worktree::WorktreeHealth::InProgress) {
        return 4;
    }
    match delegate.state {
        SessionState::Pending => 5,
        SessionState::Running => 6,
        SessionState::Idle => 7,
        SessionState::Completed => 8,
        SessionState::Stale | SessionState::Failed | SessionState::Stopped => unreachable!(),
    }
}

fn session_branch(session: &Session) -> String {
    session
        .worktree
        .as_ref()
        .map(|worktree| worktree.branch.clone())
        .unwrap_or_else(|| "-".to_string())
}

fn board_progress_bar(progress_percent: i64) -> String {
    let clamped = progress_percent.clamp(0, 100);
    let filled = ((clamped + 9) / 10) as usize;
    let empty = 10usize.saturating_sub(filled);
    format!("[{}{}]", "#".repeat(filled), ".".repeat(empty))
}

fn board_presence_marker(session: &Session) -> String {
    let codename = board_codename(session);
    let initials = codename
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .collect::<String>()
        .to_ascii_uppercase();
    format!("@{initials}")
}

fn board_motion_marker(meta: &SessionBoardMeta) -> &'static str {
    match meta.movement_note.as_deref() {
        Some("Blocked") => "x",
        Some("Completed") => "*",
        Some(note) if note.starts_with("Moved ") => ">",
        Some(note) if note.starts_with("Retargeted ") => "~",
        _ => ".",
    }
}

fn board_activity_marker(meta: &SessionBoardMeta) -> &'static str {
    match meta.activity_kind.as_deref() {
        Some("received") => "<",
        Some("delegated") => ">",
        Some("spawned") => "+",
        Some("spawned_fallback") => "#",
        _ => "",
    }
}

fn format_duration(duration_secs: u64) -> String {
    let hours = duration_secs / 3600;
    let minutes = (duration_secs % 3600) / 60;
    let seconds = duration_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn metrics_file_signature(path: &std::path::Path) -> Option<(u64, u128)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((metadata.len(), modified))
}

#[cfg(test)]
#[path = "dashboard_tests.rs"]
mod tests;

