use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::config;
use crate::comms;
use crate::session;

#[derive(Parser, Debug)]
#[command(name = "ecc", version, about = "ECC 2.0 — Agentic IDE control plane")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(clap::Args, Debug, Clone, Default)]
pub struct WorktreePolicyArgs {
    /// Create a dedicated worktree
    #[arg(short = 'w', long = "worktree", action = clap::ArgAction::SetTrue, overrides_with = "no_worktree")]
    pub worktree: bool,
    /// Skip dedicated worktree creation
    #[arg(long = "no-worktree", action = clap::ArgAction::SetTrue, overrides_with = "worktree")]
    pub no_worktree: bool,
}

impl WorktreePolicyArgs {
    pub fn resolve(&self, cfg: &config::Config) -> bool {
        if self.worktree {
            true
        } else if self.no_worktree {
            false
        } else {
            cfg.auto_create_worktrees
        }
    }
}

#[derive(clap::Args, Debug, Clone, Default)]
pub struct OptionalWorktreePolicyArgs {
    /// Create a dedicated worktree
    #[arg(short = 'w', long = "worktree", action = clap::ArgAction::SetTrue, overrides_with = "no_worktree")]
    pub worktree: bool,
    /// Skip dedicated worktree creation
    #[arg(long = "no-worktree", action = clap::ArgAction::SetTrue, overrides_with = "worktree")]
    pub no_worktree: bool,
}

impl OptionalWorktreePolicyArgs {
    pub fn resolve(&self, default_value: bool) -> bool {
        if self.worktree {
            true
        } else if self.no_worktree {
            false
        } else {
            default_value
        }
    }
}

#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Launch the TUI dashboard
    Dashboard,
    /// Start a new agent session
    Start {
        /// Task description for the agent
        #[arg(short, long)]
        task: String,
        /// Agent type (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        /// Agent profile defined in ecc2.toml
        #[arg(long)]
        profile: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Source session to delegate from
        #[arg(long)]
        from_session: Option<String>,
    },
    /// Delegate a new session from an existing one
    Delegate {
        /// Source session ID or alias
        from_session: String,
        /// Task description for the delegated session
        #[arg(short, long)]
        task: Option<String>,
        /// Agent type (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        /// Agent profile defined in ecc2.toml
        #[arg(long)]
        profile: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
    },
    /// Launch a named orchestration template
    Template {
        /// Template name defined in ecc2.toml
        name: String,
        /// Optional task injected into the template context
        #[arg(short, long)]
        task: Option<String>,
        /// Source session to delegate the template from
        #[arg(long)]
        from_session: Option<String>,
        /// Template variables in key=value form
        #[arg(long = "var")]
        vars: Vec<String>,
    },
    /// Route work to an existing delegate when possible, otherwise spawn a new one
    Assign {
        /// Lead session ID or alias
        from_session: String,
        /// Task description for the assignment
        #[arg(short, long)]
        task: String,
        /// Agent type (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        /// Agent profile defined in ecc2.toml
        #[arg(long)]
        profile: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
    },
    /// Route unread task handoffs from a lead session inbox through the assignment policy
    DrainInbox {
        /// Lead session ID or alias
        session_id: String,
        /// Agent type for routed delegates (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Maximum unread task handoffs to route
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Sweep unread task handoffs across lead sessions and route them through the assignment policy
    AutoDispatch {
        /// Agent type for routed delegates (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Maximum lead sessions to sweep in one pass
        #[arg(long, default_value_t = 10)]
        lead_limit: usize,
    },
    /// Dispatch unread handoffs, then rebalance delegate backlog across lead teams
    CoordinateBacklog {
        /// Agent type for routed delegates (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Maximum lead sessions to sweep in one pass
        #[arg(long, default_value_t = 10)]
        lead_limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
        /// Return a non-zero exit code from the final coordination health
        #[arg(long)]
        check: bool,
        /// Keep coordinating until the backlog is healthy, saturated, or max passes is reached
        #[arg(long)]
        until_healthy: bool,
        /// Maximum coordination passes when using --until-healthy
        #[arg(long, default_value_t = 5)]
        max_passes: usize,
    },
    /// Show global coordination, backlog, and daemon policy status
    CoordinationStatus {
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
        /// Return a non-zero exit code when backlog or saturation needs attention
        #[arg(long)]
        check: bool,
    },
    /// Coordinate only when backlog pressure actually needs work
    MaintainCoordination {
        /// Agent type for routed delegates (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Maximum lead sessions to sweep in one pass
        #[arg(long, default_value_t = 10)]
        lead_limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
        /// Return a non-zero exit code from the final coordination health
        #[arg(long)]
        check: bool,
        /// Maximum coordination passes when maintenance is needed
        #[arg(long, default_value_t = 5)]
        max_passes: usize,
    },
    /// Rebalance unread handoffs across lead teams with backed-up delegates
    RebalanceAll {
        /// Agent type for routed delegates (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Maximum lead sessions to sweep in one pass
        #[arg(long, default_value_t = 10)]
        lead_limit: usize,
    },
    /// Rebalance unread handoffs off backed-up delegates onto clearer team capacity
    RebalanceTeam {
        /// Lead session ID or alias
        session_id: String,
        /// Agent type for routed delegates (defaults to `default_agent` from ecc2.toml)
        #[arg(short, long)]
        agent: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Maximum handoffs to reroute in one pass
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// List active sessions
    Sessions,
    /// Show session details
    Status {
        /// Session ID or alias
        session_id: Option<String>,
    },
    /// Show delegated team board for a session
    Team {
        /// Lead session ID or alias
        session_id: Option<String>,
        /// Delegation depth to traverse
        #[arg(long, default_value_t = 2)]
        depth: usize,
    },
    /// Show worktree diff and merge-readiness details for a session
    WorktreeStatus {
        /// Session ID or alias
        session_id: Option<String>,
        /// Show worktree status for all sessions
        #[arg(long)]
        all: bool,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
        /// Include a bounded patch preview when a worktree is attached
        #[arg(long)]
        patch: bool,
        /// Return a non-zero exit code when the worktree needs attention
        #[arg(long)]
        check: bool,
    },
    /// Show conflict-resolution protocol for a worktree
    WorktreeResolution {
        /// Session ID or alias
        session_id: Option<String>,
        /// Show conflict protocol for all conflicted worktrees
        #[arg(long)]
        all: bool,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
        /// Return a non-zero exit code when conflicted worktrees are present
        #[arg(long)]
        check: bool,
    },
    /// Merge a session worktree branch into its base branch
    MergeWorktree {
        /// Session ID or alias
        session_id: Option<String>,
        /// Merge all ready inactive worktrees
        #[arg(long)]
        all: bool,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
        /// Keep the worktree attached after a successful merge
        #[arg(long)]
        keep_worktree: bool,
    },
    /// Show the merge queue for inactive worktrees and any branch-to-branch blockers
    MergeQueue {
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
        /// Process the queue, auto-rebasing clean blocked worktrees and merging what becomes ready
        #[arg(long)]
        apply: bool,
    },
    /// Prune worktrees for inactive sessions and report any active sessions still holding one
    PruneWorktrees {
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Log a significant agent decision for auditability
    LogDecision {
        /// Session ID or alias. Omit to log against the latest session.
        session_id: Option<String>,
        /// The chosen decision or direction
        #[arg(long)]
        decision: String,
        /// Why the agent made this choice
        #[arg(long)]
        reasoning: String,
        /// Alternative considered and rejected; repeat for multiple entries
        #[arg(long = "alternative")]
        alternatives: Vec<String>,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Show recent decision-log entries
    Decisions {
        /// Session ID or alias. Omit to read the latest session.
        session_id: Option<String>,
        /// Show decision log entries across all sessions
        #[arg(long)]
        all: bool,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
        /// Maximum decision-log entries to return
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Read and write the shared context graph
    Graph {
        #[command(subcommand)]
        command: GraphCommands,
    },
    /// Audit Hermes/OpenClaw-style workspaces and map them onto ECC2
    Migrate {
        #[command(subcommand)]
        command: MigrationCommands,
    },
    /// Manage persistent scheduled task dispatch
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommands,
    },
    /// Manage remote task intake and dispatch
    Remote {
        #[command(subcommand)]
        command: RemoteCommands,
    },
    /// Export sessions, tool spans, and metrics in OTLP-compatible JSON
    ExportOtel {
        /// Session ID or alias. Omit to export all sessions.
        session_id: Option<String>,
        /// Write the export to a file instead of stdout
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Stop a running session
    Stop {
        /// Session ID or alias
        session_id: String,
    },
    /// Resume a failed or stopped session
    Resume {
        /// Session ID or alias
        session_id: String,
    },
    /// Send or inspect inter-session messages
    Messages {
        #[command(subcommand)]
        command: MessageCommands,
    },
    /// Run as background daemon
    Daemon,
    #[command(hide = true)]
    RunSession {
        #[arg(long)]
        session_id: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        cwd: PathBuf,
    },
}

#[derive(clap::Subcommand, Debug)]
pub enum MessageCommands {
    /// Send a structured message between sessions
    Send {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, value_enum)]
        kind: MessageKindArg,
        #[arg(long)]
        text: String,
        #[arg(long)]
        context: Option<String>,
        #[arg(long, value_enum, default_value_t = TaskPriorityArg::Normal)]
        priority: TaskPriorityArg,
        #[arg(long)]
        file: Vec<String>,
    },
    /// Show recent messages for a session
    Inbox {
        session_id: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

#[derive(clap::Subcommand, Debug)]
pub enum ScheduleCommands {
    /// Add a persistent scheduled task
    Add {
        /// Cron expression in 5, 6, or 7-field form
        #[arg(long)]
        cron: String,
        /// Task description to run on each schedule
        #[arg(short, long)]
        task: String,
        /// Agent type (claude, codex, gemini, opencode)
        #[arg(short, long)]
        agent: Option<String>,
        /// Agent profile defined in ecc2.toml
        #[arg(long)]
        profile: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Optional project grouping override
        #[arg(long)]
        project: Option<String>,
        /// Optional task-group grouping override
        #[arg(long)]
        task_group: Option<String>,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// List scheduled tasks
    List {
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Remove a scheduled task
    Remove {
        /// Schedule ID
        schedule_id: i64,
    },
    /// Dispatch currently due scheduled tasks
    RunDue {
        /// Maximum due schedules to dispatch in one pass
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Subcommand, Debug)]
pub enum RemoteCommands {
    /// Queue a remote task request
    Add {
        /// Task description to dispatch
        #[arg(short, long)]
        task: String,
        /// Optional lead session ID or alias to route through
        #[arg(long)]
        to_session: Option<String>,
        /// Task priority
        #[arg(long, value_enum, default_value_t = TaskPriorityArg::Normal)]
        priority: TaskPriorityArg,
        /// Agent type (defaults to ECC default agent)
        #[arg(short, long)]
        agent: Option<String>,
        /// Agent profile defined in ecc2.toml
        #[arg(long)]
        profile: Option<String>,
        #[command(flatten)]
        worktree: WorktreePolicyArgs,
        /// Optional project grouping override
        #[arg(long)]
        project: Option<String>,
        /// Optional task-group grouping override
        #[arg(long)]
        task_group: Option<String>,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Queue a remote computer-use task request
    ComputerUse {
        /// Goal to complete with computer-use/browser tools
        #[arg(long)]
        goal: String,
        /// Optional target URL to open first
        #[arg(long)]
        target_url: Option<String>,
        /// Extra context for the operator
        #[arg(long)]
        context: Option<String>,
        /// Optional lead session ID or alias to route through
        #[arg(long)]
        to_session: Option<String>,
        /// Task priority
        #[arg(long, value_enum, default_value_t = TaskPriorityArg::Normal)]
        priority: TaskPriorityArg,
        /// Agent type override (defaults to [computer_use_dispatch] or ECC default agent)
        #[arg(short, long)]
        agent: Option<String>,
        /// Agent profile override (defaults to [computer_use_dispatch] or ECC default profile)
        #[arg(long)]
        profile: Option<String>,
        #[command(flatten)]
        worktree: OptionalWorktreePolicyArgs,
        /// Optional project grouping override
        #[arg(long)]
        project: Option<String>,
        /// Optional task-group grouping override
        #[arg(long)]
        task_group: Option<String>,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// List queued remote task requests
    List {
        /// Include already dispatched or failed requests
        #[arg(long)]
        all: bool,
        /// Maximum requests to return
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Dispatch queued remote task requests now
    Run {
        /// Maximum queued requests to process
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Serve a token-authenticated remote dispatch intake endpoint
    Serve {
        /// Address to bind, for example 127.0.0.1:8787
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,
        /// Bearer token required for POST /dispatch
        #[arg(long)]
        token: String,
    },
}

#[derive(clap::Subcommand, Debug)]
pub enum MigrationCommands {
    /// Audit a Hermes/OpenClaw-style workspace and map it onto ECC2 features
    Audit {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Generate an actionable ECC2 migration plan from a legacy workspace audit
    Plan {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Write the plan to a file instead of stdout
        #[arg(long)]
        output: Option<PathBuf>,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Scaffold migration artifacts on disk from a legacy workspace audit
    Scaffold {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Directory where scaffolded migration artifacts should be written
        #[arg(long)]
        output_dir: PathBuf,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Import recurring jobs from a legacy cron/jobs.json into ECC2 schedules
    ImportSchedules {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Preview detected jobs without creating ECC2 schedules
        #[arg(long)]
        dry_run: bool,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Import legacy workspace memory into the ECC2 context graph
    ImportMemory {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Maximum imported records across all synthesized connectors
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Import safe legacy env/service config context into the ECC2 context graph
    ImportEnv {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Preview detected importable sources without writing to the ECC2 graph
        #[arg(long)]
        dry_run: bool,
        /// Maximum imported records across all synthesized connectors
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Scaffold ECC-native orchestration templates from legacy skill markdown
    ImportSkills {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Directory where imported ECC2 skill artifacts should be written
        #[arg(long)]
        output_dir: PathBuf,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Scaffold ECC-native templates from legacy tool scripts
    ImportTools {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Directory where imported ECC2 tool artifacts should be written
        #[arg(long)]
        output_dir: PathBuf,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Scaffold ECC-native templates from legacy bridge plugins
    ImportPlugins {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Directory where imported ECC2 plugin artifacts should be written
        #[arg(long)]
        output_dir: PathBuf,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Import legacy gateway/dispatch tasks into the ECC2 remote queue
    ImportRemote {
        /// Path to the legacy Hermes/OpenClaw workspace root
        #[arg(long)]
        source: PathBuf,
        /// Preview detected requests without creating ECC2 remote queue entries
        #[arg(long)]
        dry_run: bool,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Subcommand, Debug)]
pub enum GraphCommands {
    /// Create or update a graph entity
    AddEntity {
        /// Optional source session ID or alias for provenance
        #[arg(long)]
        session_id: Option<String>,
        /// Entity type such as file, function, type, or decision
        #[arg(long = "type")]
        entity_type: String,
        /// Stable entity name
        #[arg(long)]
        name: String,
        /// Optional path associated with the entity
        #[arg(long)]
        path: Option<String>,
        /// Short human summary
        #[arg(long, default_value = "")]
        summary: String,
        /// Metadata in key=value form
        #[arg(long = "meta")]
        metadata: Vec<String>,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Create or update a relation between two entities
    Link {
        /// Optional source session ID or alias for provenance
        #[arg(long)]
        session_id: Option<String>,
        /// Source entity ID
        #[arg(long)]
        from: i64,
        /// Target entity ID
        #[arg(long)]
        to: i64,
        /// Relation type such as references, defines, or depends_on
        #[arg(long)]
        relation: String,
        /// Short human summary
        #[arg(long, default_value = "")]
        summary: String,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// List entities in the shared context graph
    Entities {
        /// Filter by source session ID or alias
        #[arg(long)]
        session_id: Option<String>,
        /// Filter by entity type
        #[arg(long = "type")]
        entity_type: Option<String>,
        /// Maximum entities to return
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// List relations in the shared context graph
    Relations {
        /// Filter to relations touching a specific entity ID
        #[arg(long)]
        entity_id: Option<i64>,
        /// Maximum relations to return
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Record an observation against a context graph entity
    AddObservation {
        /// Optional source session ID or alias for provenance
        #[arg(long)]
        session_id: Option<String>,
        /// Entity ID
        #[arg(long)]
        entity_id: i64,
        /// Observation type such as completion_summary, incident_note, or reminder
        #[arg(long = "type")]
        observation_type: String,
        /// Observation priority
        #[arg(long, value_enum, default_value_t = ObservationPriorityArg::Normal)]
        priority: ObservationPriorityArg,
        /// Keep this observation across aggressive compaction
        #[arg(long)]
        pinned: bool,
        /// Observation summary
        #[arg(long)]
        summary: String,
        /// Details in key=value form
        #[arg(long = "detail")]
        details: Vec<String>,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Pin an existing observation so compaction preserves it
    PinObservation {
        /// Observation ID
        #[arg(long)]
        observation_id: i64,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Remove the pin from an existing observation
    UnpinObservation {
        /// Observation ID
        #[arg(long)]
        observation_id: i64,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// List observations in the shared context graph
    Observations {
        /// Filter to observations for a specific entity ID
        #[arg(long)]
        entity_id: Option<i64>,
        /// Maximum observations to return
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Compact stored observations in the shared context graph
    Compact {
        /// Filter by source session ID or alias
        #[arg(long)]
        session_id: Option<String>,
        /// Maximum observations to retain per entity after compaction
        #[arg(long, default_value_t = 12)]
        keep_observations_per_entity: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Import external memory from a configured connector
    ConnectorSync {
        /// Connector name from ecc2.toml
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        name: Option<String>,
        /// Sync every configured memory connector
        #[arg(long, required_unless_present = "name")]
        all: bool,
        /// Maximum non-empty records to process
        #[arg(long, default_value_t = 256)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Show configured memory connectors plus checkpoint status
    Connectors {
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Recall relevant context graph entities for a query
    Recall {
        /// Filter by source session ID or alias
        #[arg(long)]
        session_id: Option<String>,
        /// Natural-language query used for recall scoring
        query: String,
        /// Maximum entities to return
        #[arg(long, default_value_t = 8)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Show one entity plus its incoming and outgoing relations
    Show {
        /// Entity ID
        entity_id: i64,
        /// Maximum incoming/outgoing relations to return
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
    /// Backfill the context graph from existing decisions and file activity
    Sync {
        /// Source session ID or alias. Omit to backfill the latest session.
        session_id: Option<String>,
        /// Backfill across all sessions
        #[arg(long)]
        all: bool,
        /// Maximum decisions and file events to scan per session
        #[arg(long, default_value_t = 64)]
        limit: usize,
        /// Emit machine-readable JSON instead of the human summary
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum MessageKindArg {
    Handoff,
    Query,
    Response,
    Completed,
    Conflict,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriorityArg {
    Low,
    Normal,
    High,
    Critical,
}

impl From<TaskPriorityArg> for comms::TaskPriority {
    fn from(value: TaskPriorityArg) -> Self {
        match value {
            TaskPriorityArg::Low => Self::Low,
            TaskPriorityArg::Normal => Self::Normal,
            TaskPriorityArg::High => Self::High,
            TaskPriorityArg::Critical => Self::Critical,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ObservationPriorityArg {
    Low,
    Normal,
    High,
    Critical,
}

impl From<ObservationPriorityArg> for session::ContextObservationPriority {
    fn from(value: ObservationPriorityArg) -> Self {
        match value {
            ObservationPriorityArg::Low => Self::Low,
            ObservationPriorityArg::Normal => Self::Normal,
            ObservationPriorityArg::High => Self::High,
            ObservationPriorityArg::Critical => Self::Critical,
        }
    }
}

