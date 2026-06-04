    use super::*;
    use crate::cli::*;
    use anyhow::Result;
    use clap::Parser;
    use crate::config::Config;
    use crate::session::store::StateStore;
    use crate::session::{Session, SessionMetrics, SessionState};
    use chrono::{Duration, Utc};
    use std::fs;
    use std::path::{Path, PathBuf};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Result<Self> {
            let path =
                std::env::temp_dir().join(format!("ecc2-main-{label}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn build_session(id: &str, task: &str, state: SessionState) -> Session {
        let now = Utc::now();
        Session {
            id: id.to_string(),
            task: task.to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp/ecc"),
            state,
            pid: None,
            worktree: None,
            created_at: now - Duration::seconds(5),
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics {
                input_tokens: 120,
                output_tokens: 30,
                tokens_used: 150,
                tool_calls: 2,
                files_changed: 1,
                duration_secs: 5,
                cost_usd: 0.42,
            },
        }
    }

    fn attr_value<'a>(attrs: &'a [OtlpKeyValue], key: &str) -> Option<&'a OtlpAnyValue> {
        attrs
            .iter()
            .find(|attr| attr.key == key)
            .map(|attr| &attr.value)
    }

    #[test]
    fn worktree_policy_defaults_to_config_setting() {
        let mut cfg = Config::default();
        let policy = WorktreePolicyArgs::default();

        assert!(policy.resolve(&cfg));

        cfg.auto_create_worktrees = false;
        assert!(!policy.resolve(&cfg));
    }

    #[test]
    fn worktree_policy_explicit_flags_override_config_setting() {
        let mut cfg = Config::default();
        cfg.auto_create_worktrees = false;

        assert!(WorktreePolicyArgs {
            worktree: true,
            no_worktree: false,
        }
        .resolve(&cfg));

        cfg.auto_create_worktrees = true;
        assert!(!WorktreePolicyArgs {
            worktree: false,
            no_worktree: true,
        }
        .resolve(&cfg));
    }

    #[test]
    fn cli_parses_resume_command() {
        let cli = Cli::try_parse_from(["ecc", "resume", "deadbeef"])
            .expect("resume subcommand should parse");

        match cli.command {
            Some(Commands::Resume { session_id }) => assert_eq!(session_id, "deadbeef"),
            _ => panic!("expected resume subcommand"),
        }
    }

    #[test]
    fn cli_parses_export_otel_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "export-otel",
            "worker-1234",
            "--output",
            "/tmp/ecc-otel.json",
        ])
        .expect("export-otel should parse");

        match cli.command {
            Some(Commands::ExportOtel { session_id, output }) => {
                assert_eq!(session_id.as_deref(), Some("worker-1234"));
                assert_eq!(output.as_deref(), Some(Path::new("/tmp/ecc-otel.json")));
            }
            _ => panic!("expected export-otel subcommand"),
        }
    }

    #[test]
    fn cli_parses_messages_send_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "messages",
            "send",
            "--from",
            "planner",
            "--to",
            "worker",
            "--kind",
            "query",
            "--text",
            "Need context",
        ])
        .expect("messages send should parse");

        match cli.command {
            Some(Commands::Messages {
                command:
                    MessageCommands::Send {
                        from,
                        to,
                        kind,
                        text,
                        priority,
                        ..
                    },
            }) => {
                assert_eq!(from, "planner");
                assert_eq!(to, "worker");
                assert!(matches!(kind, MessageKindArg::Query));
                assert_eq!(text, "Need context");
                assert_eq!(priority, TaskPriorityArg::Normal);
            }
            _ => panic!("expected messages send subcommand"),
        }
    }

    #[test]
    fn cli_parses_schedule_add_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "schedule",
            "add",
            "--cron",
            "*/15 * * * *",
            "--task",
            "Check backlog health",
            "--agent",
            "codex",
            "--profile",
            "planner",
            "--project",
            "ecc-core",
            "--task-group",
            "scheduled maintenance",
        ])
        .expect("schedule add should parse");

        match cli.command {
            Some(Commands::Schedule {
                command:
                    ScheduleCommands::Add {
                        cron,
                        task,
                        agent,
                        profile,
                        project,
                        task_group,
                        ..
                    },
            }) => {
                assert_eq!(cron, "*/15 * * * *");
                assert_eq!(task, "Check backlog health");
                assert_eq!(agent.as_deref(), Some("codex"));
                assert_eq!(profile.as_deref(), Some("planner"));
                assert_eq!(project.as_deref(), Some("ecc-core"));
                assert_eq!(task_group.as_deref(), Some("scheduled maintenance"));
            }
            _ => panic!("expected schedule add subcommand"),
        }
    }

    #[test]
    fn cli_parses_remote_computer_use_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "remote",
            "computer-use",
            "--goal",
            "Confirm the recovery banner",
            "--target-url",
            "https://ecc.tools/account",
            "--context",
            "Use the production flow",
            "--priority",
            "critical",
            "--agent",
            "codex",
            "--profile",
            "browser",
            "--no-worktree",
        ])
        .expect("remote computer-use should parse");

        match cli.command {
            Some(Commands::Remote {
                command:
                    RemoteCommands::ComputerUse {
                        goal,
                        target_url,
                        context,
                        priority,
                        agent,
                        profile,
                        worktree,
                        ..
                    },
            }) => {
                assert_eq!(goal, "Confirm the recovery banner");
                assert_eq!(target_url.as_deref(), Some("https://ecc.tools/account"));
                assert_eq!(context.as_deref(), Some("Use the production flow"));
                assert_eq!(priority, TaskPriorityArg::Critical);
                assert_eq!(agent.as_deref(), Some("codex"));
                assert_eq!(profile.as_deref(), Some("browser"));
                assert!(worktree.no_worktree);
                assert!(!worktree.worktree);
            }
            _ => panic!("expected remote computer-use subcommand"),
        }
    }

    #[test]
    fn cli_parses_start_with_handoff_source() {
        let cli = Cli::try_parse_from([
            "ecc",
            "start",
            "--task",
            "Follow up",
            "--agent",
            "claude",
            "--from-session",
            "planner",
        ])
        .expect("start with handoff source should parse");

        match cli.command {
            Some(Commands::Start {
                from_session,
                task,
                agent,
                ..
            }) => {
                assert_eq!(task, "Follow up");
                assert_eq!(agent.as_deref(), Some("claude"));
                assert_eq!(from_session.as_deref(), Some("planner"));
            }
            _ => panic!("expected start subcommand"),
        }
    }

    #[test]
    fn cli_parses_start_without_agent_override() {
        let cli = Cli::try_parse_from(["ecc", "start", "--task", "Follow up"])
            .expect("start without --agent should parse");

        match cli.command {
            Some(Commands::Start { task, agent, .. }) => {
                assert_eq!(task, "Follow up");
                assert!(agent.is_none());
            }
            _ => panic!("expected start subcommand"),
        }
    }

    #[test]
    fn cli_parses_start_no_worktree_override() {
        let cli = Cli::try_parse_from(["ecc", "start", "--task", "Follow up", "--no-worktree"])
            .expect("start --no-worktree should parse");

        match cli.command {
            Some(Commands::Start { worktree, .. }) => {
                assert!(!worktree.worktree);
                assert!(worktree.no_worktree);
            }
            _ => panic!("expected start subcommand"),
        }
    }

    #[test]
    fn cli_parses_delegate_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "delegate",
            "planner",
            "--task",
            "Review auth changes",
            "--agent",
            "codex",
        ])
        .expect("delegate should parse");

        match cli.command {
            Some(Commands::Delegate {
                from_session,
                task,
                agent,
                ..
            }) => {
                assert_eq!(from_session, "planner");
                assert_eq!(task.as_deref(), Some("Review auth changes"));
                assert_eq!(agent.as_deref(), Some("codex"));
            }
            _ => panic!("expected delegate subcommand"),
        }
    }

    #[test]
    fn cli_parses_delegate_worktree_override() {
        let cli = Cli::try_parse_from(["ecc", "delegate", "planner", "--worktree"])
            .expect("delegate --worktree should parse");

        match cli.command {
            Some(Commands::Delegate { worktree, .. }) => {
                assert!(worktree.worktree);
                assert!(!worktree.no_worktree);
            }
            _ => panic!("expected delegate subcommand"),
        }
    }

    #[test]
    fn cli_parses_template_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "template",
            "feature_development",
            "--task",
            "stabilize auth callback",
            "--from-session",
            "lead",
            "--var",
            "component=billing",
            "--var",
            "area=oauth",
        ])
        .expect("template should parse");

        match cli.command {
            Some(Commands::Template {
                name,
                task,
                from_session,
                vars,
            }) => {
                assert_eq!(name, "feature_development");
                assert_eq!(task.as_deref(), Some("stabilize auth callback"));
                assert_eq!(from_session.as_deref(), Some("lead"));
                assert_eq!(
                    vars,
                    vec!["component=billing".to_string(), "area=oauth".to_string(),]
                );
            }
            _ => panic!("expected template subcommand"),
        }
    }

    #[test]
    fn parse_template_vars_builds_map() {
        let vars =
            parse_template_vars(&["component=billing".to_string(), "area=oauth".to_string()])
                .expect("template vars");

        assert_eq!(
            vars,
            BTreeMap::from([
                ("area".to_string(), "oauth".to_string()),
                ("component".to_string(), "billing".to_string()),
            ])
        );
    }

    #[test]
    fn parse_template_vars_rejects_invalid_entries() {
        let error = parse_template_vars(&["missing-delimiter".to_string()])
            .expect_err("invalid template var should fail");

        assert!(
            error
                .to_string()
                .contains("template vars must use key=value form"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn parse_key_value_pairs_rejects_empty_values() {
        let error = parse_key_value_pairs(&["language=".to_string()], "graph metadata")
            .expect_err("invalid metadata should fail");

        assert!(
            error
                .to_string()
                .contains("graph metadata must use non-empty key=value form"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn cli_parses_team_command() {
        let cli = Cli::try_parse_from(["ecc", "team", "planner", "--depth", "3"])
            .expect("team should parse");

        match cli.command {
            Some(Commands::Team { session_id, depth }) => {
                assert_eq!(session_id.as_deref(), Some("planner"));
                assert_eq!(depth, 3);
            }
            _ => panic!("expected team subcommand"),
        }
    }

    #[test]
    fn cli_parses_worktree_status_command() {
        let cli = Cli::try_parse_from(["ecc", "worktree-status", "planner"])
            .expect("worktree-status should parse");

        match cli.command {
            Some(Commands::WorktreeStatus {
                session_id,
                all,
                json,
                patch,
                check,
            }) => {
                assert_eq!(session_id.as_deref(), Some("planner"));
                assert!(!all);
                assert!(!json);
                assert!(!patch);
                assert!(!check);
            }
            _ => panic!("expected worktree-status subcommand"),
        }
    }

    #[test]
    fn cli_parses_worktree_status_json_flag() {
        let cli = Cli::try_parse_from(["ecc", "worktree-status", "--json"])
            .expect("worktree-status --json should parse");

        match cli.command {
            Some(Commands::WorktreeStatus {
                session_id,
                all,
                json,
                patch,
                check,
            }) => {
                assert_eq!(session_id, None);
                assert!(!all);
                assert!(json);
                assert!(!patch);
                assert!(!check);
            }
            _ => panic!("expected worktree-status subcommand"),
        }
    }

    #[test]
    fn cli_parses_worktree_status_all_flag() {
        let cli = Cli::try_parse_from(["ecc", "worktree-status", "--all"])
            .expect("worktree-status --all should parse");

        match cli.command {
            Some(Commands::WorktreeStatus {
                session_id,
                all,
                json,
                patch,
                check,
            }) => {
                assert_eq!(session_id, None);
                assert!(all);
                assert!(!json);
                assert!(!patch);
                assert!(!check);
            }
            _ => panic!("expected worktree-status subcommand"),
        }
    }

    #[test]
    fn cli_parses_worktree_status_session_id_with_all_flag() {
        let err = Cli::try_parse_from(["ecc", "worktree-status", "planner", "--all"])
            .expect("worktree-status planner --all should parse");

        let command = err.command.expect("expected command");
        let Commands::WorktreeStatus {
            session_id, all, ..
        } = command
        else {
            panic!("expected worktree-status subcommand");
        };

        assert_eq!(session_id.as_deref(), Some("planner"));
        assert!(all);
    }

    #[test]
    fn format_worktree_status_reports_human_joins_multiple_reports() {
        let reports = vec![
            WorktreeStatusReport {
                session_id: "sess-a".to_string(),
                task: "first".to_string(),
                session_state: "running".to_string(),
                health: "in_progress".to_string(),
                check_exit_code: 1,
                patch_included: false,
                attached: false,
                path: None,
                branch: None,
                base_branch: None,
                diff_summary: None,
                file_preview: Vec::new(),
                patch_preview: None,
                merge_readiness: None,
            },
            WorktreeStatusReport {
                session_id: "sess-b".to_string(),
                task: "second".to_string(),
                session_state: "stopped".to_string(),
                health: "clear".to_string(),
                check_exit_code: 0,
                patch_included: false,
                attached: false,
                path: None,
                branch: None,
                base_branch: None,
                diff_summary: None,
                file_preview: Vec::new(),
                patch_preview: None,
                merge_readiness: None,
            },
        ];

        let text = format_worktree_status_reports_human(&reports);
        assert!(text.contains("Worktree status for sess-a [running]"));
        assert!(text.contains("Worktree status for sess-b [stopped]"));
        assert!(text.contains("\n\nWorktree status for sess-b [stopped]"));
    }

    #[test]
    fn cli_parses_worktree_status_patch_flag() {
        let cli = Cli::try_parse_from(["ecc", "worktree-status", "--patch"])
            .expect("worktree-status --patch should parse");

        match cli.command {
            Some(Commands::WorktreeStatus {
                session_id,
                all,
                json,
                patch,
                check,
            }) => {
                assert_eq!(session_id, None);
                assert!(!all);
                assert!(!json);
                assert!(patch);
                assert!(!check);
            }
            _ => panic!("expected worktree-status subcommand"),
        }
    }

    #[test]
    fn build_otel_export_includes_session_and_tool_spans() -> Result<()> {
        let tempdir = TestDir::new("otel-export-session")?;
        let db = StateStore::open(&tempdir.path().join("state.db"))?;
        let session = build_session("session-1", "Investigate export", SessionState::Completed);
        db.insert_session(&session)?;
        db.insert_tool_log(
            &session.id,
            "Write",
            "Write src/lib.rs",
            "{\"file\":\"src/lib.rs\"}",
            "Updated file",
            "manual test",
            120,
            0.75,
            &Utc::now().to_rfc3339(),
        )?;

        let export = build_otel_export(&db, Some("session-1"))?;
        let spans = &export.resource_spans[0].scope_spans[0].spans;
        assert_eq!(spans.len(), 2);

        let session_span = spans
            .iter()
            .find(|span| span.parent_span_id.is_none())
            .expect("session root span");
        let tool_span = spans
            .iter()
            .find(|span| span.parent_span_id.is_some())
            .expect("tool child span");

        assert_eq!(session_span.trace_id, tool_span.trace_id);
        assert_eq!(
            tool_span.parent_span_id.as_deref(),
            Some(session_span.span_id.as_str())
        );
        assert_eq!(session_span.status.code, "STATUS_CODE_OK");
        assert_eq!(
            attr_value(&session_span.attributes, "ecc.session.id")
                .and_then(|value| value.string_value.as_deref()),
            Some("session-1")
        );
        assert_eq!(
            attr_value(&tool_span.attributes, "tool.name")
                .and_then(|value| value.string_value.as_deref()),
            Some("Write")
        );
        assert_eq!(
            attr_value(&tool_span.attributes, "tool.duration_ms")
                .and_then(|value| value.int_value.as_deref()),
            Some("120")
        );

        Ok(())
    }

    #[test]
    fn build_otel_export_links_delegated_session_to_parent_trace() -> Result<()> {
        let tempdir = TestDir::new("otel-export-parent-link")?;
        let db = StateStore::open(&tempdir.path().join("state.db"))?;
        let parent = build_session("lead-1", "Lead task", SessionState::Running);
        let child = build_session("worker-1", "Delegated task", SessionState::Running);
        db.insert_session(&parent)?;
        db.insert_session(&child)?;
        db.send_message(
            &parent.id,
            &child.id,
            "{\"task\":\"Delegated task\",\"context\":\"Delegated from lead\"}",
            "task_handoff",
        )?;

        let export = build_otel_export(&db, Some("worker-1"))?;
        let session_span = export.resource_spans[0].scope_spans[0]
            .spans
            .iter()
            .find(|span| span.parent_span_id.is_none())
            .expect("session root span");

        assert_eq!(session_span.links.len(), 1);
        assert_eq!(session_span.links[0].trace_id, otlp_trace_id("lead-1"));
        assert_eq!(
            session_span.links[0].span_id,
            otlp_span_id("session:lead-1")
        );
        assert_eq!(
            attr_value(&session_span.links[0].attributes, "ecc.parent_session.id")
                .and_then(|value| value.string_value.as_deref()),
            Some("lead-1")
        );

        Ok(())
    }

    #[test]
    fn cli_parses_worktree_status_check_flag() {
        let cli = Cli::try_parse_from(["ecc", "worktree-status", "--check"])
            .expect("worktree-status --check should parse");

        match cli.command {
            Some(Commands::WorktreeStatus {
                session_id,
                all,
                json,
                patch,
                check,
            }) => {
                assert_eq!(session_id, None);
                assert!(!all);
                assert!(!json);
                assert!(!patch);
                assert!(check);
            }
            _ => panic!("expected worktree-status subcommand"),
        }
    }

    #[test]
    fn cli_parses_worktree_resolution_flags() {
        let cli =
            Cli::try_parse_from(["ecc", "worktree-resolution", "planner", "--json", "--check"])
                .expect("worktree-resolution flags should parse");

        match cli.command {
            Some(Commands::WorktreeResolution {
                session_id,
                all,
                json,
                check,
            }) => {
                assert_eq!(session_id.as_deref(), Some("planner"));
                assert!(!all);
                assert!(json);
                assert!(check);
            }
            _ => panic!("expected worktree-resolution subcommand"),
        }
    }

    #[test]
    fn cli_parses_worktree_resolution_all_flag() {
        let cli = Cli::try_parse_from(["ecc", "worktree-resolution", "--all"])
            .expect("worktree-resolution --all should parse");

        match cli.command {
            Some(Commands::WorktreeResolution {
                session_id,
                all,
                json,
                check,
            }) => {
                assert!(session_id.is_none());
                assert!(all);
                assert!(!json);
                assert!(!check);
            }
            _ => panic!("expected worktree-resolution subcommand"),
        }
    }

    #[test]
    fn cli_parses_prune_worktrees_json_flag() {
        let cli = Cli::try_parse_from(["ecc", "prune-worktrees", "--json"])
            .expect("prune-worktrees --json should parse");

        match cli.command {
            Some(Commands::PruneWorktrees { json }) => {
                assert!(json);
            }
            _ => panic!("expected prune-worktrees subcommand"),
        }
    }

    #[test]
    fn cli_parses_merge_worktree_flags() {
        let cli = Cli::try_parse_from([
            "ecc",
            "merge-worktree",
            "deadbeef",
            "--json",
            "--keep-worktree",
        ])
        .expect("merge-worktree flags should parse");

        match cli.command {
            Some(Commands::MergeWorktree {
                session_id,
                all,
                json,
                keep_worktree,
            }) => {
                assert_eq!(session_id.as_deref(), Some("deadbeef"));
                assert!(!all);
                assert!(json);
                assert!(keep_worktree);
            }
            _ => panic!("expected merge-worktree subcommand"),
        }
    }

    #[test]
    fn cli_parses_merge_worktree_all_flags() {
        let cli = Cli::try_parse_from(["ecc", "merge-worktree", "--all", "--json"])
            .expect("merge-worktree --all --json should parse");

        match cli.command {
            Some(Commands::MergeWorktree {
                session_id,
                all,
                json,
                keep_worktree,
            }) => {
                assert!(session_id.is_none());
                assert!(all);
                assert!(json);
                assert!(!keep_worktree);
            }
            _ => panic!("expected merge-worktree subcommand"),
        }
    }

    #[test]
    fn cli_parses_merge_queue_json_flag() {
        let cli = Cli::try_parse_from(["ecc", "merge-queue", "--json"])
            .expect("merge-queue --json should parse");

        match cli.command {
            Some(Commands::MergeQueue { json, apply }) => {
                assert!(json);
                assert!(!apply);
            }
            _ => panic!("expected merge-queue subcommand"),
        }
    }

    #[test]
    fn cli_parses_merge_queue_apply_flag() {
        let cli = Cli::try_parse_from(["ecc", "merge-queue", "--apply", "--json"])
            .expect("merge-queue --apply --json should parse");

        match cli.command {
            Some(Commands::MergeQueue { json, apply }) => {
                assert!(json);
                assert!(apply);
            }
            _ => panic!("expected merge-queue subcommand"),
        }
    }

    #[test]
    fn format_worktree_status_human_includes_readiness_and_conflicts() {
        let report = WorktreeStatusReport {
            session_id: "deadbeefcafefeed".to_string(),
            task: "Review merge readiness".to_string(),
            session_state: "running".to_string(),
            health: "conflicted".to_string(),
            check_exit_code: 2,
            patch_included: true,
            attached: true,
            path: Some("/tmp/ecc/wt-1".to_string()),
            branch: Some("ecc/deadbeefcafefeed".to_string()),
            base_branch: Some("main".to_string()),
            diff_summary: Some("Branch 1 file changed, 2 insertions(+)".to_string()),
            file_preview: vec!["Branch M README.md".to_string()],
            patch_preview: Some("--- Branch diff vs main ---\n+hello".to_string()),
            merge_readiness: Some(WorktreeMergeReadinessReport {
                status: "conflicted".to_string(),
                summary: "Merge blocked by 1 conflict(s): README.md".to_string(),
                conflicts: vec!["README.md".to_string()],
            }),
        };

        let text = format_worktree_status_human(&report);
        assert!(text.contains("Worktree status for deadbeef [running]"));
        assert!(text.contains("Branch ecc/deadbeefcafefeed (base main)"));
        assert!(text.contains("Health conflicted"));
        assert!(text.contains("Branch M README.md"));
        assert!(text.contains("Merge blocked by 1 conflict(s): README.md"));
        assert!(text.contains("- conflict README.md"));
        assert!(text.contains("Patch preview"));
        assert!(text.contains("--- Branch diff vs main ---"));
    }

    #[test]
    fn format_worktree_resolution_human_includes_protocol_steps() {
        let report = WorktreeResolutionReport {
            session_id: "deadbeefcafefeed".to_string(),
            task: "Resolve merge conflict".to_string(),
            session_state: "stopped".to_string(),
            attached: true,
            conflicted: true,
            check_exit_code: 2,
            path: Some("/tmp/ecc/wt-1".to_string()),
            branch: Some("ecc/deadbeefcafefeed".to_string()),
            base_branch: Some("main".to_string()),
            summary: "Merge blocked by 1 conflict(s): README.md".to_string(),
            conflicts: vec!["README.md".to_string()],
            resolution_steps: vec![
                "Inspect current patch: ecc worktree-status deadbeefcafefeed --patch".to_string(),
                "Open worktree: cd /tmp/ecc/wt-1".to_string(),
                "Resolve conflicts and stage files: git add <paths>".to_string(),
            ],
        };

        let text = format_worktree_resolution_human(&report);
        assert!(text.contains("Worktree resolution for deadbeef [stopped]"));
        assert!(text.contains("Merge blocked by 1 conflict(s): README.md"));
        assert!(text.contains("Conflicts"));
        assert!(text.contains("- README.md"));
        assert!(text.contains("Resolution steps"));
        assert!(text.contains("1. Inspect current patch"));
    }

    #[test]
    fn worktree_resolution_reports_exit_code_tracks_conflicts() {
        let clear = WorktreeResolutionReport {
            session_id: "clear".to_string(),
            task: "ok".to_string(),
            session_state: "stopped".to_string(),
            attached: false,
            conflicted: false,
            check_exit_code: 0,
            path: None,
            branch: None,
            base_branch: None,
            summary: "No worktree attached".to_string(),
            conflicts: Vec::new(),
            resolution_steps: Vec::new(),
        };
        let conflicted = WorktreeResolutionReport {
            session_id: "conflicted".to_string(),
            task: "resolve".to_string(),
            session_state: "failed".to_string(),
            attached: true,
            conflicted: true,
            check_exit_code: 2,
            path: Some("/tmp/ecc/wt-2".to_string()),
            branch: Some("ecc/conflicted".to_string()),
            base_branch: Some("main".to_string()),
            summary: "Merge blocked by 1 conflict(s): src/lib.rs".to_string(),
            conflicts: vec!["src/lib.rs".to_string()],
            resolution_steps: vec!["Inspect current patch".to_string()],
        };

        assert_eq!(worktree_resolution_reports_exit_code(&[clear]), 0);
        assert_eq!(worktree_resolution_reports_exit_code(&[conflicted]), 2);
    }

    #[test]
    fn format_prune_worktrees_human_reports_cleaned_and_active_sessions() {
        let text = format_prune_worktrees_human(&session::manager::WorktreePruneOutcome {
            cleaned_session_ids: vec!["deadbeefcafefeed".to_string()],
            active_with_worktree_ids: vec!["facefeed12345678".to_string()],
            retained_session_ids: vec!["retain1234567890".to_string()],
        });

        assert!(text.contains("Pruned 1 inactive worktree(s)"));
        assert!(text.contains("- cleaned deadbeef"));
        assert!(text.contains("Skipped 1 active session(s) still holding worktrees"));
        assert!(text.contains("- active facefeed"));
        assert!(text.contains("Deferred 1 inactive worktree(s) still within retention"));
        assert!(text.contains("- retained retain12"));
    }

    #[test]
    fn format_worktree_merge_human_reports_merge_and_cleanup() {
        let text = format_worktree_merge_human(&session::manager::WorktreeMergeOutcome {
            session_id: "deadbeefcafefeed".to_string(),
            branch: "ecc/deadbeef".to_string(),
            base_branch: "main".to_string(),
            already_up_to_date: false,
            cleaned_worktree: true,
        });

        assert!(text.contains("Merged worktree for deadbeef"));
        assert!(text.contains("Branch ecc/deadbeef -> main"));
        assert!(text.contains("Result merged into base"));
        assert!(text.contains("Cleanup removed worktree and branch"));
    }

    #[test]
    fn format_merge_queue_human_reports_ready_and_blocked_entries() {
        let text = format_merge_queue_human(&session::manager::MergeQueueReport {
            ready_entries: vec![session::manager::MergeQueueEntry {
                session_id: "alpha1234".to_string(),
                task: "merge alpha".to_string(),
                project: "ecc".to_string(),
                task_group: "checkout".to_string(),
                branch: "ecc/alpha1234".to_string(),
                base_branch: "main".to_string(),
                state: session::SessionState::Stopped,
                worktree_health: worktree::WorktreeHealth::InProgress,
                dirty: false,
                queue_position: Some(1),
                ready_to_merge: true,
                blocked_by: Vec::new(),
                suggested_action: "merge in queue order #1".to_string(),
            }],
            blocked_entries: vec![session::manager::MergeQueueEntry {
                session_id: "beta5678".to_string(),
                task: "merge beta".to_string(),
                project: "ecc".to_string(),
                task_group: "checkout".to_string(),
                branch: "ecc/beta5678".to_string(),
                base_branch: "main".to_string(),
                state: session::SessionState::Stopped,
                worktree_health: worktree::WorktreeHealth::InProgress,
                dirty: false,
                queue_position: None,
                ready_to_merge: false,
                blocked_by: vec![session::manager::MergeQueueBlocker {
                    session_id: "alpha1234".to_string(),
                    branch: "ecc/alpha1234".to_string(),
                    state: session::SessionState::Stopped,
                    conflicts: vec!["README.md".to_string()],
                    summary: "merge after alpha1234 to avoid branch conflicts".to_string(),
                    conflicting_patch_preview: Some(
                        "--- Branch diff vs main ---\nREADME.md".to_string(),
                    ),
                    blocker_patch_preview: None,
                }],
                suggested_action: "merge after alpha1234".to_string(),
            }],
        });

        assert!(text.contains("Merge queue: 1 ready / 1 blocked"));
        assert!(text.contains("Ready"));
        assert!(text.contains("#1 alpha1234"));
        assert!(text.contains("Blocked"));
        assert!(text.contains("beta5678"));
        assert!(text.contains("blocker alpha1234"));
        assert!(text.contains("conflict README.md"));
    }

    #[test]
    fn format_bulk_worktree_merge_human_reports_summary_and_skips() {
        let text = format_bulk_worktree_merge_human(&session::manager::WorktreeBulkMergeOutcome {
            merged: vec![session::manager::WorktreeMergeOutcome {
                session_id: "deadbeefcafefeed".to_string(),
                branch: "ecc/deadbeefcafefeed".to_string(),
                base_branch: "main".to_string(),
                already_up_to_date: false,
                cleaned_worktree: true,
            }],
            rebased: vec![session::manager::WorktreeRebaseOutcome {
                session_id: "rebased12345678".to_string(),
                branch: "ecc/rebased12345678".to_string(),
                base_branch: "main".to_string(),
                already_up_to_date: false,
            }],
            active_with_worktree_ids: vec!["running12345678".to_string()],
            conflicted_session_ids: vec!["conflict123456".to_string()],
            dirty_worktree_ids: vec!["dirty123456789".to_string()],
            blocked_by_queue_session_ids: vec!["queue123456789".to_string()],
            failures: vec![session::manager::WorktreeMergeFailure {
                session_id: "fail1234567890".to_string(),
                reason: "base branch not checked out".to_string(),
            }],
        });

        assert!(text.contains("Merged 1 ready worktree(s)"));
        assert!(text.contains("- merged ecc/deadbeefcafefeed -> main for deadbeef"));
        assert!(text.contains("Rebased 1 blocked worktree(s) onto their base branch"));
        assert!(text.contains("- rebased ecc/rebased12345678 onto main for rebased1"));
        assert!(text.contains("Skipped 1 active worktree session(s)"));
        assert!(text.contains("Skipped 1 conflicted worktree(s)"));
        assert!(text.contains("Skipped 1 dirty worktree(s)"));
        assert!(text.contains("Blocked 1 worktree(s) on remaining queue conflicts"));
        assert!(text.contains("Encountered 1 merge failure(s)"));
        assert!(text.contains("- failed fail1234: base branch not checked out"));
    }

    #[test]
    fn format_worktree_status_human_handles_missing_worktree() {
        let report = WorktreeStatusReport {
            session_id: "deadbeefcafefeed".to_string(),
            task: "No worktree here".to_string(),
            session_state: "stopped".to_string(),
            health: "clear".to_string(),
            check_exit_code: 0,
            patch_included: true,
            attached: false,
            path: None,
            branch: None,
            base_branch: None,
            diff_summary: None,
            file_preview: Vec::new(),
            patch_preview: None,
            merge_readiness: None,
        };

        let text = format_worktree_status_human(&report);
        assert!(text.contains("Worktree status for deadbeef [stopped]"));
        assert!(text.contains("Task No worktree here"));
        assert!(text.contains("Health clear"));
        assert!(text.contains("No worktree attached"));
    }

    #[test]
    fn worktree_status_exit_code_tracks_health() {
        let clear = WorktreeStatusReport {
            session_id: "a".to_string(),
            task: "clear".to_string(),
            session_state: "idle".to_string(),
            health: "clear".to_string(),
            check_exit_code: 0,
            patch_included: false,
            attached: false,
            path: None,
            branch: None,
            base_branch: None,
            diff_summary: None,
            file_preview: Vec::new(),
            patch_preview: None,
            merge_readiness: None,
        };
        let in_progress = WorktreeStatusReport {
            session_id: "b".to_string(),
            task: "progress".to_string(),
            session_state: "running".to_string(),
            health: "in_progress".to_string(),
            check_exit_code: 1,
            patch_included: false,
            attached: true,
            path: Some("/tmp/ecc/wt-2".to_string()),
            branch: Some("ecc/b".to_string()),
            base_branch: Some("main".to_string()),
            diff_summary: Some("Branch 1 file changed".to_string()),
            file_preview: vec!["Branch M README.md".to_string()],
            patch_preview: None,
            merge_readiness: Some(WorktreeMergeReadinessReport {
                status: "ready".to_string(),
                summary: "Merge ready into main".to_string(),
                conflicts: Vec::new(),
            }),
        };
        let conflicted = WorktreeStatusReport {
            session_id: "c".to_string(),
            task: "conflict".to_string(),
            session_state: "running".to_string(),
            health: "conflicted".to_string(),
            check_exit_code: 2,
            patch_included: false,
            attached: true,
            path: Some("/tmp/ecc/wt-3".to_string()),
            branch: Some("ecc/c".to_string()),
            base_branch: Some("main".to_string()),
            diff_summary: Some("Branch 1 file changed".to_string()),
            file_preview: vec!["Branch M README.md".to_string()],
            patch_preview: None,
            merge_readiness: Some(WorktreeMergeReadinessReport {
                status: "conflicted".to_string(),
                summary: "Merge blocked by 1 conflict(s): README.md".to_string(),
                conflicts: vec!["README.md".to_string()],
            }),
        };

        assert_eq!(worktree_status_exit_code(&clear), 0);
        assert_eq!(worktree_status_exit_code(&in_progress), 1);
        assert_eq!(worktree_status_exit_code(&conflicted), 2);
    }

    #[test]
    fn worktree_status_reports_exit_code_uses_highest_severity() {
        let reports = vec![
            WorktreeStatusReport {
                session_id: "sess-a".to_string(),
                task: "first".to_string(),
                session_state: "running".to_string(),
                health: "clear".to_string(),
                check_exit_code: 0,
                patch_included: false,
                attached: false,
                path: None,
                branch: None,
                base_branch: None,
                diff_summary: None,
                file_preview: Vec::new(),
                patch_preview: None,
                merge_readiness: None,
            },
            WorktreeStatusReport {
                session_id: "sess-b".to_string(),
                task: "second".to_string(),
                session_state: "running".to_string(),
                health: "in_progress".to_string(),
                check_exit_code: 1,
                patch_included: false,
                attached: false,
                path: None,
                branch: None,
                base_branch: None,
                diff_summary: None,
                file_preview: Vec::new(),
                patch_preview: None,
                merge_readiness: None,
            },
            WorktreeStatusReport {
                session_id: "sess-c".to_string(),
                task: "third".to_string(),
                session_state: "running".to_string(),
                health: "conflicted".to_string(),
                check_exit_code: 2,
                patch_included: false,
                attached: false,
                path: None,
                branch: None,
                base_branch: None,
                diff_summary: None,
                file_preview: Vec::new(),
                patch_preview: None,
                merge_readiness: None,
            },
        ];

        assert_eq!(worktree_status_reports_exit_code(&reports), 2);
    }

    #[test]
    fn cli_parses_assign_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "assign",
            "lead",
            "--task",
            "Review auth changes",
            "--agent",
            "claude",
        ])
        .expect("assign should parse");

        match cli.command {
            Some(Commands::Assign {
                from_session,
                task,
                agent,
                ..
            }) => {
                assert_eq!(from_session, "lead");
                assert_eq!(task, "Review auth changes");
                assert_eq!(agent.as_deref(), Some("claude"));
            }
            _ => panic!("expected assign subcommand"),
        }
    }

    #[test]
    fn cli_parses_drain_inbox_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "drain-inbox",
            "lead",
            "--agent",
            "claude",
            "--limit",
            "3",
        ])
        .expect("drain-inbox should parse");

        match cli.command {
            Some(Commands::DrainInbox {
                session_id,
                agent,
                limit,
                ..
            }) => {
                assert_eq!(session_id, "lead");
                assert_eq!(agent.as_deref(), Some("claude"));
                assert_eq!(limit, 3);
            }
            _ => panic!("expected drain-inbox subcommand"),
        }
    }

    #[test]
    fn cli_parses_auto_dispatch_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "auto-dispatch",
            "--agent",
            "claude",
            "--lead-limit",
            "4",
        ])
        .expect("auto-dispatch should parse");

        match cli.command {
            Some(Commands::AutoDispatch {
                agent, lead_limit, ..
            }) => {
                assert_eq!(agent.as_deref(), Some("claude"));
                assert_eq!(lead_limit, 4);
            }
            _ => panic!("expected auto-dispatch subcommand"),
        }
    }

    #[test]
    fn cli_parses_coordinate_backlog_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "coordinate-backlog",
            "--agent",
            "claude",
            "--lead-limit",
            "7",
        ])
        .expect("coordinate-backlog should parse");

        match cli.command {
            Some(Commands::CoordinateBacklog {
                agent,
                lead_limit,
                check,
                until_healthy,
                max_passes,
                ..
            }) => {
                assert_eq!(agent.as_deref(), Some("claude"));
                assert_eq!(lead_limit, 7);
                assert!(!check);
                assert!(!until_healthy);
                assert_eq!(max_passes, 5);
            }
            _ => panic!("expected coordinate-backlog subcommand"),
        }
    }

    #[test]
    fn cli_parses_coordinate_backlog_until_healthy_flags() {
        let cli = Cli::try_parse_from([
            "ecc",
            "coordinate-backlog",
            "--until-healthy",
            "--max-passes",
            "3",
        ])
        .expect("coordinate-backlog looping flags should parse");

        match cli.command {
            Some(Commands::CoordinateBacklog {
                json,
                until_healthy,
                max_passes,
                ..
            }) => {
                assert!(!json);
                assert!(until_healthy);
                assert_eq!(max_passes, 3);
            }
            _ => panic!("expected coordinate-backlog subcommand"),
        }
    }

    #[test]
    fn cli_parses_coordinate_backlog_json_flag() {
        let cli = Cli::try_parse_from(["ecc", "coordinate-backlog", "--json"])
            .expect("coordinate-backlog --json should parse");

        match cli.command {
            Some(Commands::CoordinateBacklog {
                json,
                check,
                until_healthy,
                max_passes,
                ..
            }) => {
                assert!(json);
                assert!(!check);
                assert!(!until_healthy);
                assert_eq!(max_passes, 5);
            }
            _ => panic!("expected coordinate-backlog subcommand"),
        }
    }

    #[test]
    fn cli_parses_coordinate_backlog_check_flag() {
        let cli = Cli::try_parse_from(["ecc", "coordinate-backlog", "--check"])
            .expect("coordinate-backlog --check should parse");

        match cli.command {
            Some(Commands::CoordinateBacklog {
                json,
                check,
                until_healthy,
                max_passes,
                ..
            }) => {
                assert!(!json);
                assert!(check);
                assert!(!until_healthy);
                assert_eq!(max_passes, 5);
            }
            _ => panic!("expected coordinate-backlog subcommand"),
        }
    }

    #[test]
    fn cli_parses_rebalance_all_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "rebalance-all",
            "--agent",
            "claude",
            "--lead-limit",
            "6",
        ])
        .expect("rebalance-all should parse");

        match cli.command {
            Some(Commands::RebalanceAll {
                agent, lead_limit, ..
            }) => {
                assert_eq!(agent.as_deref(), Some("claude"));
                assert_eq!(lead_limit, 6);
            }
            _ => panic!("expected rebalance-all subcommand"),
        }
    }

    #[test]
    fn cli_parses_coordination_status_command() {
        let cli = Cli::try_parse_from(["ecc", "coordination-status"])
            .expect("coordination-status should parse");

        match cli.command {
            Some(Commands::CoordinationStatus { json, check }) => {
                assert!(!json);
                assert!(!check);
            }
            _ => panic!("expected coordination-status subcommand"),
        }
    }

    #[test]
    fn cli_parses_log_decision_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "log-decision",
            "latest",
            "--decision",
            "Use sqlite",
            "--reasoning",
            "It is already embedded",
            "--alternative",
            "json files",
            "--alternative",
            "memory only",
            "--json",
        ])
        .expect("log-decision should parse");

        match cli.command {
            Some(Commands::LogDecision {
                session_id,
                decision,
                reasoning,
                alternatives,
                json,
            }) => {
                assert_eq!(session_id.as_deref(), Some("latest"));
                assert_eq!(decision, "Use sqlite");
                assert_eq!(reasoning, "It is already embedded");
                assert_eq!(alternatives, vec!["json files", "memory only"]);
                assert!(json);
            }
            _ => panic!("expected log-decision subcommand"),
        }
    }

    #[test]
    fn cli_parses_decisions_command() {
        let cli = Cli::try_parse_from(["ecc", "decisions", "--all", "--limit", "5", "--json"])
            .expect("decisions should parse");

        match cli.command {
            Some(Commands::Decisions {
                session_id,
                all,
                json,
                limit,
            }) => {
                assert!(session_id.is_none());
                assert!(all);
                assert!(json);
                assert_eq!(limit, 5);
            }
            _ => panic!("expected decisions subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_add_entity_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "graph",
            "add-entity",
            "--session-id",
            "latest",
            "--type",
            "file",
            "--name",
            "dashboard.rs",
            "--path",
            "ecc2/src/tui/dashboard.rs",
            "--summary",
            "Primary TUI surface",
            "--meta",
            "language=rust",
            "--json",
        ])
        .expect("graph add-entity should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::AddEntity {
                        session_id,
                        entity_type,
                        name,
                        path,
                        summary,
                        metadata,
                        json,
                    },
            }) => {
                assert_eq!(session_id.as_deref(), Some("latest"));
                assert_eq!(entity_type, "file");
                assert_eq!(name, "dashboard.rs");
                assert_eq!(path.as_deref(), Some("ecc2/src/tui/dashboard.rs"));
                assert_eq!(summary, "Primary TUI surface");
                assert_eq!(metadata, vec!["language=rust"]);
                assert!(json);
            }
            _ => panic!("expected graph add-entity subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_sync_command() {
        let cli = Cli::try_parse_from(["ecc", "graph", "sync", "--all", "--limit", "12", "--json"])
            .expect("graph sync should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::Sync {
                        session_id,
                        all,
                        limit,
                        json,
                    },
            }) => {
                assert!(session_id.is_none());
                assert!(all);
                assert_eq!(limit, 12);
                assert!(json);
            }
            _ => panic!("expected graph sync subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_recall_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "graph",
            "recall",
            "--session-id",
            "latest",
            "--limit",
            "4",
            "--json",
            "auth callback recovery",
        ])
        .expect("graph recall should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::Recall {
                        session_id,
                        query,
                        limit,
                        json,
                    },
            }) => {
                assert_eq!(session_id.as_deref(), Some("latest"));
                assert_eq!(query, "auth callback recovery");
                assert_eq!(limit, 4);
                assert!(json);
            }
            _ => panic!("expected graph recall subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_add_observation_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "graph",
            "add-observation",
            "--session-id",
            "latest",
            "--entity-id",
            "7",
            "--type",
            "completion_summary",
            "--pinned",
            "--summary",
            "Finished auth callback recovery",
            "--detail",
            "tests_run=2",
            "--json",
        ])
        .expect("graph add-observation should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::AddObservation {
                        session_id,
                        entity_id,
                        observation_type,
                        priority,
                        pinned,
                        summary,
                        details,
                        json,
                    },
            }) => {
                assert_eq!(session_id.as_deref(), Some("latest"));
                assert_eq!(entity_id, 7);
                assert_eq!(observation_type, "completion_summary");
                assert!(matches!(priority, ObservationPriorityArg::Normal));
                assert!(pinned);
                assert_eq!(summary, "Finished auth callback recovery");
                assert_eq!(details, vec!["tests_run=2"]);
                assert!(json);
            }
            _ => panic!("expected graph add-observation subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_pin_observation_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "graph",
            "pin-observation",
            "--observation-id",
            "42",
            "--json",
        ])
        .expect("graph pin-observation should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::PinObservation {
                        observation_id,
                        json,
                    },
            }) => {
                assert_eq!(observation_id, 42);
                assert!(json);
            }
            _ => panic!("expected graph pin-observation subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_unpin_observation_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "graph",
            "unpin-observation",
            "--observation-id",
            "42",
            "--json",
        ])
        .expect("graph unpin-observation should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::UnpinObservation {
                        observation_id,
                        json,
                    },
            }) => {
                assert_eq!(observation_id, 42);
                assert!(json);
            }
            _ => panic!("expected graph unpin-observation subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_compact_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "graph",
            "compact",
            "--session-id",
            "latest",
            "--keep-observations-per-entity",
            "6",
            "--json",
        ])
        .expect("graph compact should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::Compact {
                        session_id,
                        keep_observations_per_entity,
                        json,
                    },
            }) => {
                assert_eq!(session_id.as_deref(), Some("latest"));
                assert_eq!(keep_observations_per_entity, 6);
                assert!(json);
            }
            _ => panic!("expected graph compact subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_connector_sync_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "graph",
            "connector-sync",
            "hermes_notes",
            "--limit",
            "32",
            "--json",
        ])
        .expect("graph connector-sync should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::ConnectorSync {
                        name,
                        all,
                        limit,
                        json,
                    },
            }) => {
                assert_eq!(name.as_deref(), Some("hermes_notes"));
                assert!(!all);
                assert_eq!(limit, 32);
                assert!(json);
            }
            _ => panic!("expected graph connector-sync subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_connector_sync_all_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "graph",
            "connector-sync",
            "--all",
            "--limit",
            "16",
            "--json",
        ])
        .expect("graph connector-sync --all should parse");

        match cli.command {
            Some(Commands::Graph {
                command:
                    GraphCommands::ConnectorSync {
                        name,
                        all,
                        limit,
                        json,
                    },
            }) => {
                assert_eq!(name, None);
                assert!(all);
                assert_eq!(limit, 16);
                assert!(json);
            }
            _ => panic!("expected graph connector-sync --all subcommand"),
        }
    }

    #[test]
    fn cli_parses_graph_connectors_command() {
        let cli = Cli::try_parse_from(["ecc", "graph", "connectors", "--json"])
            .expect("graph connectors should parse");

        match cli.command {
            Some(Commands::Graph {
                command: GraphCommands::Connectors { json },
            }) => {
                assert!(json);
            }
            _ => panic!("expected graph connectors subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_audit_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "audit",
            "--source",
            "/tmp/hermes",
            "--json",
        ])
        .expect("migrate audit should parse");

        match cli.command {
            Some(Commands::Migrate {
                command: MigrationCommands::Audit { source, json },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert!(json);
            }
            _ => panic!("expected migrate audit subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_plan_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "plan",
            "--source",
            "/tmp/hermes",
            "--output",
            "/tmp/plan.md",
        ])
        .expect("migrate plan should parse");

        match cli.command {
            Some(Commands::Migrate {
                command:
                    MigrationCommands::Plan {
                        source,
                        output,
                        json,
                    },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert_eq!(output, Some(PathBuf::from("/tmp/plan.md")));
                assert!(!json);
            }
            _ => panic!("expected migrate plan subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_scaffold_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "scaffold",
            "--source",
            "/tmp/hermes",
            "--output-dir",
            "/tmp/migration-scaffold",
            "--json",
        ])
        .expect("migrate scaffold should parse");

        match cli.command {
            Some(Commands::Migrate {
                command:
                    MigrationCommands::Scaffold {
                        source,
                        output_dir,
                        json,
                    },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert_eq!(output_dir, PathBuf::from("/tmp/migration-scaffold"));
                assert!(json);
            }
            _ => panic!("expected migrate scaffold subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_import_schedules_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "import-schedules",
            "--source",
            "/tmp/hermes",
            "--dry-run",
            "--json",
        ])
        .expect("migrate import-schedules should parse");

        match cli.command {
            Some(Commands::Migrate {
                command:
                    MigrationCommands::ImportSchedules {
                        source,
                        dry_run,
                        json,
                    },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert!(dry_run);
                assert!(json);
            }
            _ => panic!("expected migrate import-schedules subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_import_memory_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "import-memory",
            "--source",
            "/tmp/hermes",
            "--limit",
            "24",
            "--json",
        ])
        .expect("migrate import-memory should parse");

        match cli.command {
            Some(Commands::Migrate {
                command:
                    MigrationCommands::ImportMemory {
                        source,
                        limit,
                        json,
                    },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert_eq!(limit, 24);
                assert!(json);
            }
            _ => panic!("expected migrate import-memory subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_import_env_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "import-env",
            "--source",
            "/tmp/hermes",
            "--dry-run",
            "--limit",
            "42",
            "--json",
        ])
        .expect("migrate import-env should parse");

        match cli.command {
            Some(Commands::Migrate {
                command:
                    MigrationCommands::ImportEnv {
                        source,
                        dry_run,
                        limit,
                        json,
                    },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert!(dry_run);
                assert_eq!(limit, 42);
                assert!(json);
            }
            _ => panic!("expected migrate import-env subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_import_skills_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "import-skills",
            "--source",
            "/tmp/hermes",
            "--output-dir",
            "/tmp/out",
            "--json",
        ])
        .expect("migrate import-skills should parse");

        match cli.command {
            Some(Commands::Migrate {
                command:
                    MigrationCommands::ImportSkills {
                        source,
                        output_dir,
                        json,
                    },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert_eq!(output_dir, PathBuf::from("/tmp/out"));
                assert!(json);
            }
            _ => panic!("expected migrate import-skills subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_import_tools_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "import-tools",
            "--source",
            "/tmp/hermes",
            "--output-dir",
            "/tmp/out",
            "--json",
        ])
        .expect("migrate import-tools should parse");

        match cli.command {
            Some(Commands::Migrate {
                command:
                    MigrationCommands::ImportTools {
                        source,
                        output_dir,
                        json,
                    },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert_eq!(output_dir, PathBuf::from("/tmp/out"));
                assert!(json);
            }
            _ => panic!("expected migrate import-tools subcommand"),
        }
    }

    #[test]
    fn cli_parses_migrate_import_plugins_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "migrate",
            "import-plugins",
            "--source",
            "/tmp/hermes",
            "--output-dir",
            "/tmp/out",
            "--json",
        ])
        .expect("migrate import-plugins should parse");

        match cli.command {
            Some(Commands::Migrate {
                command:
                    MigrationCommands::ImportPlugins {
                        source,
                        output_dir,
                        json,
                    },
            }) => {
                assert_eq!(source, PathBuf::from("/tmp/hermes"));
                assert_eq!(output_dir, PathBuf::from("/tmp/out"));
                assert!(json);
            }
            _ => panic!("expected migrate import-plugins subcommand"),
        }
    }

    #[test]
    fn legacy_migration_audit_report_maps_detected_artifacts() -> Result<()> {
        let tempdir = TestDir::new("legacy-migration-audit")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("cron"))?;
        fs::create_dir_all(root.join("gateway"))?;
        fs::create_dir_all(root.join("workspace/notes"))?;
        fs::create_dir_all(root.join("skills/ecc-imports"))?;
        fs::create_dir_all(root.join("tools"))?;
        fs::create_dir_all(root.join("plugins"))?;
        fs::write(root.join("config.yaml"), "model: claude\n")?;
        fs::write(root.join("cron/scheduler.py"), "print('tick')\n")?;
        fs::write(root.join("jobs.py"), "JOBS = []\n")?;
        fs::write(root.join("gateway/router.py"), "route = True\n")?;
        fs::write(root.join("memory_tool.py"), "class MemoryTool: pass\n")?;
        fs::write(root.join("workspace/notes/recovery.md"), "# recovery\n")?;
        fs::write(root.join("skills/ecc-imports/research.md"), "# skill\n")?;
        fs::write(root.join("tools/browser.py"), "print('browser')\n")?;
        fs::write(root.join("plugins/reminders.py"), "print('reminders')\n")?;
        fs::write(
            root.join(".env.local"),
            "STRIPE_SECRET_KEY=sk_test_secret\n",
        )?;

        let report = build_legacy_migration_audit_report(root)?;

        assert_eq!(report.detected_systems, vec!["hermes"]);
        assert_eq!(report.summary.artifact_categories_detected, 8);
        assert_eq!(report.summary.ready_now_categories, 4);
        assert_eq!(report.summary.manual_translation_categories, 3);
        assert_eq!(report.summary.local_auth_required_categories, 1);
        assert!(report
            .recommended_next_steps
            .iter()
            .any(|step| step.contains("ecc schedule add")));
        assert!(report
            .recommended_next_steps
            .iter()
            .any(|step| step.contains("ecc remote serve")));

        let scheduler = report
            .artifacts
            .iter()
            .find(|artifact| artifact.category == "scheduler")
            .expect("scheduler artifact");
        assert_eq!(scheduler.readiness, LegacyMigrationReadiness::ReadyNow);
        assert_eq!(scheduler.detected_items, 2);

        let env_services = report
            .artifacts
            .iter()
            .find(|artifact| artifact.category == "env_services")
            .expect("env services artifact");
        assert_eq!(
            env_services.readiness,
            LegacyMigrationReadiness::LocalAuthRequired
        );
        assert!(env_services
            .source_paths
            .contains(&"config.yaml".to_string()));
        assert!(env_services
            .source_paths
            .contains(&".env.local".to_string()));

        Ok(())
    }

    #[test]
    fn legacy_migration_plan_report_generates_workspace_connector_step() -> Result<()> {
        let tempdir = TestDir::new("legacy-migration-plan")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("cron"))?;
        fs::create_dir_all(root.join("gateway"))?;
        fs::create_dir_all(root.join("workspace/notes"))?;
        fs::create_dir_all(root.join("skills/ecc-imports"))?;
        fs::create_dir_all(root.join("tools"))?;
        fs::create_dir_all(root.join("plugins"))?;
        fs::write(root.join("config.yaml"), "model: claude\n")?;
        fs::write(
            root.join("cron/jobs.json"),
            serde_json::json!({
                "jobs": [
                    {
                        "name": "portal-recovery",
                        "cron": "*/15 * * * *",
                        "prompt": "Check portal-first recovery flow",
                        "agent": "codex",
                        "project": "billing-web",
                        "task_group": "recovery",
                        "use_worktree": false
                    },
                    {
                        "name": "paused-job",
                        "cron": "0 12 * * *",
                        "prompt": "This one stays paused",
                        "disabled": true
                    }
                ]
            })
            .to_string(),
        )?;
        fs::write(
            root.join("gateway/dispatch.jsonl"),
            [
                serde_json::json!({
                    "name": "route-account-recovery",
                    "task": "Handle account recovery triage",
                    "priority": "high",
                    "agent": "codex",
                    "project": "ecc-tools",
                    "task_group": "recovery"
                })
                .to_string(),
                serde_json::json!({
                    "name": "browser-billing-check",
                    "kind": "computer_use",
                    "goal": "Verify the billing portal warning banner",
                    "target_url": "https://ecc.tools/account",
                    "context": "Use the production account flow",
                    "priority": "critical",
                    "use_worktree": false
                })
                .to_string(),
                serde_json::json!({
                    "name": "paused-remote",
                    "task": "Do not migrate this now",
                    "disabled": true
                })
                .to_string(),
            ]
            .join("\n"),
        )?;
        fs::write(root.join("workspace/notes/recovery.md"), "# recovery\n")?;
        fs::write(root.join("skills/ecc-imports/research.md"), "# research\n")?;
        fs::create_dir_all(root.join("tools"))?;
        fs::write(
            root.join("tools/browser.py"),
            "# Verify the billing portal banner\nprint('browser')\n",
        )?;
        fs::write(
            root.join("plugins/recovery.py"),
            "# Account recovery command bridge\nprint('recovery')\n",
        )?;

        let audit = build_legacy_migration_audit_report(root)?;
        let plan = build_legacy_migration_plan_report(&audit);

        let workspace_step = plan
            .steps
            .iter()
            .find(|step| step.category == "workspace_memory")
            .expect("workspace memory step");
        assert_eq!(workspace_step.readiness, LegacyMigrationReadiness::ReadyNow);
        assert!(workspace_step
            .config_snippets
            .iter()
            .any(|snippet| snippet.contains("[memory_connectors.hermes_workspace]")));
        assert!(workspace_step
            .command_snippets
            .contains(&"ecc graph connector-sync hermes_workspace".to_string()));

        let scheduler_step = plan
            .steps
            .iter()
            .find(|step| step.category == "scheduler")
            .expect("scheduler step");
        assert!(scheduler_step
            .command_snippets
            .iter()
            .any(|command| command.contains("ecc schedule add --cron \"*/15 * * * *\"")));
        assert!(!scheduler_step
            .command_snippets
            .iter()
            .any(|command| command.contains("<legacy-cron>")));
        assert!(scheduler_step
            .notes
            .iter()
            .any(|note| note.contains("disabled")));

        let gateway_step = plan
            .steps
            .iter()
            .find(|step| step.category == "gateway_dispatch")
            .expect("gateway step");
        assert!(gateway_step
            .command_snippets
            .iter()
            .any(|command| command
                .contains("ecc remote add --task \"Handle account recovery triage\"")));
        assert!(gateway_step
            .command_snippets
            .iter()
            .any(|command| command.contains(
                "ecc remote computer-use --goal \"Verify the billing portal warning banner\""
            )));
        assert!(!gateway_step
            .command_snippets
            .iter()
            .any(|command| command.contains("Translate legacy dispatch workflow")));
        assert!(gateway_step
            .notes
            .iter()
            .any(|note| note.contains("disabled")));

        let rendered = format_legacy_migration_plan_human(&plan);
        assert!(rendered.contains("Legacy migration plan"));
        assert!(rendered.contains("Import sanitized workspace memory through ECC2 connectors"));
        let env_step = plan
            .steps
            .iter()
            .find(|step| step.category == "env_services")
            .expect("env services step");
        assert!(env_step
            .command_snippets
            .iter()
            .any(|command| command.contains("ecc migrate import-env --source")));
        let skills_step = plan
            .steps
            .iter()
            .find(|step| step.category == "skills")
            .expect("skills step");
        assert!(skills_step
            .command_snippets
            .iter()
            .any(|command| command.contains("ecc migrate import-skills --source")));
        let tools_step = plan
            .steps
            .iter()
            .find(|step| step.category == "tools")
            .expect("tools step");
        assert!(tools_step
            .command_snippets
            .iter()
            .any(|command| command.contains("ecc migrate import-tools --source")));
        let plugins_step = plan
            .steps
            .iter()
            .find(|step| step.category == "plugins")
            .expect("plugins step");
        assert!(plugins_step
            .command_snippets
            .iter()
            .any(|command| command.contains("ecc migrate import-plugins --source")));

        Ok(())
    }

    #[test]
    fn import_legacy_schedules_dry_run_reports_ready_disabled_and_invalid_jobs() -> Result<()> {
        let tempdir = TestDir::new("legacy-schedule-import-dry-run")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("cron"))?;
        fs::write(
            root.join("cron/jobs.json"),
            serde_json::json!({
                "jobs": [
                    {
                        "name": "portal-recovery",
                        "cron": "*/15 * * * *",
                        "prompt": "Check portal-first recovery flow",
                        "agent": "codex",
                        "project": "billing-web",
                        "task_group": "recovery",
                        "use_worktree": false
                    },
                    {
                        "name": "paused-job",
                        "cron": "0 12 * * *",
                        "prompt": "This one stays paused",
                        "disabled": true
                    },
                    {
                        "name": "broken-job",
                        "prompt": "Missing cron"
                    }
                ]
            })
            .to_string(),
        )?;

        let tempdb = TestDir::new("legacy-schedule-import-dry-run-db")?;
        let db = StateStore::open(&tempdb.path().join("state.db"))?;
        let report = import_legacy_schedules(&db, &config::Config::default(), root, true)?;

        assert!(report.dry_run);
        assert_eq!(report.jobs_detected, 3);
        assert_eq!(report.ready_jobs, 1);
        assert_eq!(report.imported_jobs, 0);
        assert_eq!(report.disabled_jobs, 1);
        assert_eq!(report.invalid_jobs, 1);
        assert_eq!(report.skipped_jobs, 0);
        assert_eq!(report.jobs.len(), 3);
        assert!(report
            .jobs
            .iter()
            .any(|job| job.command_snippet.as_deref() == Some("ecc schedule add --cron \"*/15 * * * *\" --task \"Check portal-first recovery flow\" --agent \"codex\" --no-worktree --project \"billing-web\" --task-group \"recovery\"")));

        Ok(())
    }

    #[test]
    fn import_legacy_schedules_creates_real_ecc2_schedules() -> Result<()> {
        let tempdir = TestDir::new("legacy-schedule-import-live")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("cron"))?;
        fs::write(
            root.join("cron/jobs.json"),
            serde_json::json!({
                "jobs": [
                    {
                        "name": "portal-recovery",
                        "cron": "*/15 * * * *",
                        "prompt": "Check portal-first recovery flow",
                        "agent": "codex",
                        "project": "billing-web",
                        "task_group": "recovery",
                        "use_worktree": false
                    }
                ]
            })
            .to_string(),
        )?;

        let target_repo = tempdir.path().join("target");
        fs::create_dir_all(&target_repo)?;
        fs::write(target_repo.join(".gitignore"), "target\n")?;

        let tempdb = TestDir::new("legacy-schedule-import-live-db")?;
        let db = StateStore::open(&tempdb.path().join("state.db"))?;
        let _cwd_guard = crate::test_support::CurrentDirGuard::enter(&target_repo)?;
        let report = import_legacy_schedules(&db, &config::Config::default(), root, false)?;

        assert!(!report.dry_run);
        assert_eq!(report.ready_jobs, 1);
        assert_eq!(report.imported_jobs, 1);
        assert_eq!(
            report.jobs[0].status,
            LegacyScheduleImportJobStatus::Imported
        );
        assert!(report.jobs[0].imported_schedule_id.is_some());

        let schedules = db.list_scheduled_tasks()?;
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].task, "Check portal-first recovery flow");
        assert_eq!(schedules[0].agent_type, "codex");
        assert_eq!(schedules[0].project, "billing-web");
        assert_eq!(schedules[0].task_group, "recovery");
        assert!(!schedules[0].use_worktree);
        assert_eq!(
            schedules[0].working_dir.canonicalize()?,
            target_repo.canonicalize()?
        );

        Ok(())
    }

    #[test]
    fn import_legacy_memory_imports_workspace_markdown_and_jsonl() -> Result<()> {
        let tempdir = TestDir::new("legacy-memory-import")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("workspace/notes"))?;
        fs::create_dir_all(root.join("workspace/memory"))?;
        fs::write(
            root.join("workspace/notes/recovery.md"),
            r#"# Billing incident
Customer wiped setup and got charged twice after reinstalling.

## Portal routing
Route existing installs to portal first before checkout.
"#,
        )?;
        fs::write(
            root.join("workspace/memory/hermes.jsonl"),
            [
                serde_json::json!({
                    "entity_name": "Billing recovery checklist",
                    "summary": "Use portal-first routing before offering checkout again"
                })
                .to_string(),
                serde_json::json!({
                    "entity_name": "Repair before reinstall",
                    "summary": "Recommend ecc repair before purchase flows"
                })
                .to_string(),
            ]
            .join("\n"),
        )?;

        let tempdb = TestDir::new("legacy-memory-import-db")?;
        let db = StateStore::open(&tempdb.path().join("state.db"))?;
        let report = import_legacy_memory(&db, &config::Config::default(), root, 10)?;

        assert_eq!(report.connectors_detected, 2);
        assert_eq!(report.report.connectors_synced, 2);
        assert_eq!(report.report.records_read, 4);
        assert_eq!(report.report.entities_upserted, 4);
        assert_eq!(report.report.observations_added, 4);

        let recalled = db.recall_context_entities(None, "charged twice portal reinstall", 10)?;
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Billing incident"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Billing recovery checklist"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Repair before reinstall"));

        Ok(())
    }

    #[test]
    fn import_legacy_memory_reports_no_workspace_connectors_when_absent() -> Result<()> {
        let tempdir = TestDir::new("legacy-memory-import-empty")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("skills"))?;

        let tempdb = TestDir::new("legacy-memory-import-empty-db")?;
        let db = StateStore::open(&tempdb.path().join("state.db"))?;
        let report = import_legacy_memory(&db, &config::Config::default(), root, 10)?;

        assert_eq!(report.connectors_detected, 0);
        assert_eq!(report.report.connectors_synced, 0);
        assert_eq!(report.report.records_read, 0);
        assert_eq!(report.report.entities_upserted, 0);
        assert_eq!(report.report.observations_added, 0);

        Ok(())
    }

    #[test]
    fn import_legacy_remote_dispatch_dry_run_reports_ready_disabled_and_invalid_requests(
    ) -> Result<()> {
        let tempdir = TestDir::new("legacy-remote-import-dry-run")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("gateway"))?;
        fs::write(
            root.join("gateway/dispatch.json"),
            serde_json::json!({
                "requests": [
                    {
                        "name": "route-account-recovery",
                        "task": "Handle account recovery triage",
                        "priority": "high",
                        "agent": "codex",
                        "project": "ecc-tools",
                        "task_group": "recovery",
                        "use_worktree": false
                    },
                    {
                        "name": "browser-billing-check",
                        "kind": "computer_use",
                        "goal": "Verify the billing portal warning banner",
                        "target_url": "https://ecc.tools/account",
                        "context": "Use the production account flow",
                        "priority": "critical"
                    },
                    {
                        "name": "paused-remote",
                        "task": "Do not migrate this now",
                        "disabled": true
                    },
                    {
                        "name": "broken-remote",
                        "kind": "computer_use",
                        "context": "Missing goal"
                    }
                ]
            })
            .to_string(),
        )?;

        let tempdb = TestDir::new("legacy-remote-import-dry-run-db")?;
        let db = StateStore::open(&tempdb.path().join("state.db"))?;
        let report = import_legacy_remote_dispatch(&db, &Config::default(), root, true)?;

        assert!(report.dry_run);
        assert_eq!(report.requests_detected, 4);
        assert_eq!(report.ready_requests, 2);
        assert_eq!(report.imported_requests, 0);
        assert_eq!(report.disabled_requests, 1);
        assert_eq!(report.invalid_requests, 1);
        assert_eq!(report.skipped_requests, 0);
        assert_eq!(report.requests.len(), 4);
        assert!(report.requests.iter().any(|request| request.command_snippet.as_deref()
            == Some("ecc remote add --task \"Handle account recovery triage\" --priority high --agent \"codex\" --no-worktree --project \"ecc-tools\" --task-group \"recovery\"")));
        assert!(report.requests.iter().any(|request| request.command_snippet.as_deref()
            == Some("ecc remote computer-use --goal \"Verify the billing portal warning banner\" --target-url \"https://ecc.tools/account\" --context \"Use the production account flow\" --priority critical")));

        Ok(())
    }

    #[test]
    fn import_legacy_remote_dispatch_creates_real_pending_requests() -> Result<()> {
        let tempdir = TestDir::new("legacy-remote-import-live")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("gateway"))?;
        fs::write(
            root.join("gateway/dispatch.jsonl"),
            [
                serde_json::json!({
                    "name": "route-account-recovery",
                    "task": "Handle account recovery triage",
                    "priority": "high",
                    "agent": "codex",
                    "project": "ecc-tools",
                    "task_group": "recovery",
                    "use_worktree": false
                })
                .to_string(),
                serde_json::json!({
                    "name": "browser-billing-check",
                    "kind": "computer_use",
                    "goal": "Verify the billing portal warning banner",
                    "target_url": "https://ecc.tools/account",
                    "context": "Use the production account flow",
                    "priority": "critical",
                    "project": "remote-ops",
                    "task_group": "browser"
                })
                .to_string(),
            ]
            .join("\n"),
        )?;

        let target_repo = tempdir.path().join("target");
        fs::create_dir_all(&target_repo)?;
        fs::write(target_repo.join(".gitignore"), "target\n")?;

        let tempdb = TestDir::new("legacy-remote-import-live-db")?;
        let db = StateStore::open(&tempdb.path().join("state.db"))?;
        let _cwd_guard = crate::test_support::CurrentDirGuard::enter(&target_repo)?;

        let report = import_legacy_remote_dispatch(&db, &Config::default(), root, false)?;

        assert!(!report.dry_run);
        assert_eq!(report.ready_requests, 2);
        assert_eq!(report.imported_requests, 2);
        assert_eq!(
            report.requests[0].status,
            LegacyRemoteImportRequestStatus::Imported
        );
        assert!(report
            .requests
            .iter()
            .all(|request| request.imported_request_id.is_some()));

        let requests = db.list_pending_remote_dispatch_requests(10)?;
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].request_kind,
            session::RemoteDispatchKind::ComputerUse
        );
        assert_eq!(requests[0].priority, comms::TaskPriority::Critical);
        assert_eq!(requests[0].project, "remote-ops");
        assert_eq!(requests[0].task_group, "browser");
        assert_eq!(
            requests[0].target_url.as_deref(),
            Some("https://ecc.tools/account")
        );
        assert!(requests[0].task.contains("Computer-use task."));
        assert_eq!(
            requests[1].request_kind,
            session::RemoteDispatchKind::Standard
        );
        assert_eq!(requests[1].priority, comms::TaskPriority::High);
        assert_eq!(requests[1].agent_type, "codex");
        assert_eq!(requests[1].project, "ecc-tools");
        assert_eq!(requests[1].task_group, "recovery");
        assert!(!requests[1].use_worktree);
        assert_eq!(requests[1].task, "Handle account recovery triage");
        assert_eq!(
            requests[1].working_dir.canonicalize()?,
            target_repo.canonicalize()?
        );

        Ok(())
    }

    #[test]
    fn import_legacy_env_dry_run_reports_importable_and_manual_sources() -> Result<()> {
        let tempdir = TestDir::new("legacy-env-import-dry-run")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("services"))?;
        fs::write(
            root.join(".env.local"),
            "STRIPE_SECRET_KEY=sk_test_secret\nPUBLIC_BASE_URL=https://ecc.tools\n",
        )?;
        fs::write(
            root.join(".envrc"),
            "export OPENAI_API_KEY=sk-openai-secret\nexport PUBLIC_DOCS_URL=https://docs.ecc.tools\n",
        )?;
        fs::write(root.join("config.yaml"), "model: claude\n")?;
        fs::write(
            root.join("services").join("billing.json"),
            "{\"port\": 3000}\n",
        )?;

        let tempdb = TestDir::new("legacy-env-import-dry-run-db")?;
        let db = StateStore::open(&tempdb.path().join("state.db"))?;
        let report = import_legacy_env_services(&db, root, true, 10)?;

        assert!(report.dry_run);
        assert_eq!(report.importable_sources, 2);
        assert_eq!(report.imported_sources, 0);
        assert_eq!(report.manual_reentry_sources, 2);
        assert_eq!(report.connectors_detected, 2);
        assert_eq!(report.report.connectors_synced, 0);
        assert_eq!(
            report
                .sources
                .iter()
                .filter(|item| item.status == LegacyEnvImportSourceStatus::Ready)
                .count(),
            2
        );
        assert!(report.sources.iter().any(|item| {
            item.source_path == "config.yaml"
                && item.status == LegacyEnvImportSourceStatus::ManualOnly
        }));
        assert!(report.sources.iter().any(|item| {
            item.source_path == "services" && item.status == LegacyEnvImportSourceStatus::ManualOnly
        }));

        Ok(())
    }

    #[test]
    fn import_legacy_env_imports_safe_context_into_graph() -> Result<()> {
        let tempdir = TestDir::new("legacy-env-import-live")?;
        let root = tempdir.path();
        fs::write(
            root.join(".env.local"),
            "STRIPE_SECRET_KEY=sk_test_secret\nPUBLIC_BASE_URL=https://ecc.tools\n",
        )?;
        fs::write(
            root.join(".env.production"),
            "export OPENAI_API_KEY=sk-openai-secret\nexport PUBLIC_DOCS_URL=https://docs.ecc.tools\n",
        )?;

        let tempdb = TestDir::new("legacy-env-import-live-db")?;
        let db = StateStore::open(&tempdb.path().join("state.db"))?;
        let report = import_legacy_env_services(&db, root, false, 10)?;

        assert!(!report.dry_run);
        assert_eq!(report.importable_sources, 2);
        assert_eq!(report.imported_sources, 2);
        assert_eq!(report.manual_reentry_sources, 0);
        assert_eq!(report.report.connectors_synced, 2);
        assert_eq!(report.report.records_read, 4);
        assert!(report.sources.iter().all(|item| {
            item.status == LegacyEnvImportSourceStatus::Imported
                || item.status == LegacyEnvImportSourceStatus::Ready
        }));

        let recalled = db.recall_context_entities(None, "stripe docs ecc.tools", 10)?;
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "STRIPE_SECRET_KEY"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "PUBLIC_BASE_URL"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "PUBLIC_DOCS_URL"));

        let secret = recalled
            .iter()
            .find(|entry| entry.entity.name == "STRIPE_SECRET_KEY")
            .expect("secret entry should exist");
        let observations = db.list_context_observations(Some(secret.entity.id), 5)?;
        assert_eq!(
            observations[0]
                .details
                .get("secret_redacted")
                .map(String::as_str),
            Some("true")
        );
        assert!(!observations[0].details.contains_key("value"));

        Ok(())
    }

    #[test]
    fn import_legacy_skills_writes_template_artifacts() -> Result<()> {
        let tempdir = TestDir::new("legacy-skill-import")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("skills/ecc-imports"))?;
        fs::create_dir_all(root.join("skills/ops"))?;
        fs::write(
            root.join("skills/ecc-imports/research.md"),
            "# Recovery research\nGather billing/account context before touching checkout logic.\n",
        )?;
        fs::write(
            root.join("skills/ops/recovery.markdown"),
            "# Portal repair\nRoute wiped installs toward repair before presenting new checkout.\n",
        )?;

        let output_dir = root.join("out");
        let report = import_legacy_skills(root, &output_dir)?;

        assert_eq!(report.skills_detected, 2);
        assert_eq!(report.templates_generated, 2);
        assert_eq!(report.files_written.len(), 2);
        assert!(report
            .skills
            .iter()
            .any(|skill| skill.template_name == "ecc_imports_research_md"));
        assert!(report
            .skills
            .iter()
            .any(|skill| skill.template_name == "ops_recovery_markdown"));

        let config_text = fs::read_to_string(output_dir.join("ecc2.imported-skills.toml"))?;
        assert!(config_text.contains("[orchestration_templates.ecc_imports_research_md]"));
        assert!(config_text.contains("[orchestration_templates.ops_recovery_markdown]"));
        assert!(config_text.contains("Translate and run that workflow for {{task}}."));

        let summary_text = fs::read_to_string(output_dir.join("imported-skills.md"))?;
        assert!(summary_text.contains("skills/ecc-imports/research.md"));
        assert!(summary_text.contains("skills/ops/recovery.markdown"));

        Ok(())
    }

    #[test]
    fn import_legacy_tools_writes_template_artifacts() -> Result<()> {
        let tempdir = TestDir::new("legacy-tool-import")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("tools/browser"))?;
        fs::create_dir_all(root.join("tools/hooks"))?;
        fs::write(
            root.join("tools/browser/check_portal.py"),
            "# Verify the billing portal warning banner\nprint('check banner')\n",
        )?;
        fs::write(
            root.join("tools/hooks/preflight.sh"),
            "#!/usr/bin/env bash\n# PretoolUse guard for dangerous commands\nexit 0\n",
        )?;

        let output_dir = root.join("out");
        let report = import_legacy_tools(root, &output_dir)?;

        assert_eq!(report.tools_detected, 2);
        assert_eq!(report.templates_generated, 2);
        assert_eq!(report.files_written.len(), 2);
        assert!(report
            .tools
            .iter()
            .any(|tool| tool.template_name == "tool_browser_check_portal_py"));
        assert!(report
            .tools
            .iter()
            .any(|tool| tool.template_name == "tool_hooks_preflight_sh"));
        assert!(report
            .tools
            .iter()
            .any(|tool| tool.suggested_surface == "command"));
        assert!(report
            .tools
            .iter()
            .any(|tool| tool.suggested_surface == "hook"));

        let config_text = fs::read_to_string(output_dir.join("ecc2.imported-tools.toml"))?;
        assert!(config_text.contains("[orchestration_templates.tool_browser_check_portal_py]"));
        assert!(config_text.contains("[orchestration_templates.tool_hooks_preflight_sh]"));
        assert!(config_text.contains("Rebuild or wrap that behavior as an ECC-native"));

        let summary_text = fs::read_to_string(output_dir.join("imported-tools.md"))?;
        assert!(summary_text.contains("tools/browser/check_portal.py"));
        assert!(summary_text.contains("tools/hooks/preflight.sh"));
        assert!(summary_text.contains("Suggested surface: hook"));

        Ok(())
    }

    #[test]
    fn import_legacy_plugins_writes_template_artifacts() -> Result<()> {
        let tempdir = TestDir::new("legacy-plugin-import")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("plugins/hooks"))?;
        fs::create_dir_all(root.join("plugins/skills"))?;
        fs::write(
            root.join("plugins/hooks/review.py"),
            "# PostToolUse notifier for risky changes\nprint('review')\n",
        )?;
        fs::write(
            root.join("plugins/skills/recovery.py"),
            "# Recovery skill bridge for wiped setups\nprint('recovery')\n",
        )?;

        let output_dir = root.join("out");
        let report = import_legacy_plugins(root, &output_dir)?;

        assert_eq!(report.plugins_detected, 2);
        assert_eq!(report.templates_generated, 2);
        assert_eq!(report.files_written.len(), 2);
        assert!(report
            .plugins
            .iter()
            .any(|plugin| plugin.template_name == "plugin_hooks_review_py"));
        assert!(report
            .plugins
            .iter()
            .any(|plugin| plugin.template_name == "plugin_skills_recovery_py"));
        assert!(report
            .plugins
            .iter()
            .any(|plugin| plugin.suggested_surface == "hook"));
        assert!(report
            .plugins
            .iter()
            .any(|plugin| plugin.suggested_surface == "skill"));

        let config_text = fs::read_to_string(output_dir.join("ecc2.imported-plugins.toml"))?;
        assert!(config_text.contains("[orchestration_templates.plugin_hooks_review_py]"));
        assert!(config_text.contains("[orchestration_templates.plugin_skills_recovery_py]"));
        assert!(config_text.contains("Port that behavior into an ECC-native"));

        let summary_text = fs::read_to_string(output_dir.join("imported-plugins.md"))?;
        assert!(summary_text.contains("plugins/hooks/review.py"));
        assert!(summary_text.contains("plugins/skills/recovery.py"));
        assert!(summary_text.contains("Suggested surface: skill"));

        Ok(())
    }

    #[test]
    fn legacy_migration_scaffold_writes_plan_and_config_files() -> Result<()> {
        let tempdir = TestDir::new("legacy-migration-scaffold")?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("workspace/notes"))?;
        fs::create_dir_all(root.join("skills/ecc-imports"))?;
        fs::write(root.join("config.yaml"), "model: claude\n")?;
        fs::write(root.join("workspace/notes/recovery.md"), "# recovery\n")?;
        fs::write(root.join("skills/ecc-imports/triage.md"), "# triage\n")?;

        let audit = build_legacy_migration_audit_report(root)?;
        let plan = build_legacy_migration_plan_report(&audit);
        let output_dir = root.join("out");
        let report = write_legacy_migration_scaffold(&plan, &output_dir)?;

        assert_eq!(report.steps_scaffolded, plan.steps.len());
        assert_eq!(report.files_written.len(), 2);

        let plan_text = fs::read_to_string(output_dir.join("migration-plan.md"))?;
        let config_text = fs::read_to_string(output_dir.join("ecc2.migration.toml"))?;
        assert!(plan_text.contains("Legacy migration plan"));
        assert!(config_text.contains("[memory_connectors.hermes_workspace]"));
        assert!(config_text.contains("[orchestration_templates.legacy_workflow]"));

        Ok(())
    }

    #[test]
    fn format_decisions_human_renders_details() {
        let text = format_decisions_human(
            &[session::DecisionLogEntry {
                id: 1,
                session_id: "sess-12345678".to_string(),
                decision: "Use sqlite for the shared context graph".to_string(),
                alternatives: vec!["json files".to_string(), "memory only".to_string()],
                reasoning: "SQLite keeps the audit trail queryable.".to_string(),
                timestamp: chrono::DateTime::parse_from_rfc3339("2026-04-09T01:02:03Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            }],
            true,
        );

        assert!(text.contains("Decision log: 1 entries"));
        assert!(text.contains("sess-123"));
        assert!(text.contains("Use sqlite for the shared context graph"));
        assert!(text.contains("why SQLite keeps the audit trail queryable."));
        assert!(text.contains("alternative json files"));
        assert!(text.contains("alternative memory only"));
    }

    #[test]
    fn format_graph_entity_detail_human_renders_relations() {
        let detail = session::ContextGraphEntityDetail {
            entity: session::ContextGraphEntity {
                id: 7,
                session_id: Some("sess-12345678".to_string()),
                entity_type: "function".to_string(),
                name: "render_metrics".to_string(),
                path: Some("ecc2/src/tui/dashboard.rs".to_string()),
                summary: "Renders the metrics pane".to_string(),
                metadata: BTreeMap::from([("language".to_string(), "rust".to_string())]),
                created_at: chrono::DateTime::parse_from_rfc3339("2026-04-10T01:02:03Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339("2026-04-10T01:02:03Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            },
            outgoing: vec![session::ContextGraphRelation {
                id: 9,
                session_id: Some("sess-12345678".to_string()),
                from_entity_id: 7,
                from_entity_type: "function".to_string(),
                from_entity_name: "render_metrics".to_string(),
                to_entity_id: 10,
                to_entity_type: "type".to_string(),
                to_entity_name: "MetricsSnapshot".to_string(),
                relation_type: "returns".to_string(),
                summary: "Produces the rendered metrics model".to_string(),
                created_at: chrono::DateTime::parse_from_rfc3339("2026-04-10T01:02:03Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            }],
            incoming: vec![session::ContextGraphRelation {
                id: 8,
                session_id: Some("sess-12345678".to_string()),
                from_entity_id: 6,
                from_entity_type: "file".to_string(),
                from_entity_name: "dashboard.rs".to_string(),
                to_entity_id: 7,
                to_entity_type: "function".to_string(),
                to_entity_name: "render_metrics".to_string(),
                relation_type: "contains".to_string(),
                summary: "Dashboard owns the render path".to_string(),
                created_at: chrono::DateTime::parse_from_rfc3339("2026-04-10T01:02:03Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            }],
        };

        let text = format_graph_entity_detail_human(&detail);
        assert!(text.contains("Context graph entity #7"));
        assert!(text.contains("Outgoing relations: 1"));
        assert!(text.contains("[returns] render_metrics -> #10 MetricsSnapshot"));
        assert!(text.contains("Incoming relations: 1"));
        assert!(text.contains("[contains] #6 dashboard.rs -> render_metrics"));
    }

    #[test]
    fn format_graph_recall_human_renders_scores_and_matches() {
        let text = format_graph_recall_human(
            &[session::ContextGraphRecallEntry {
                entity: session::ContextGraphEntity {
                    id: 11,
                    session_id: Some("sess-12345678".to_string()),
                    entity_type: "file".to_string(),
                    name: "callback.ts".to_string(),
                    path: Some("src/routes/auth/callback.ts".to_string()),
                    summary: "Handles auth callback recovery".to_string(),
                    metadata: BTreeMap::new(),
                    created_at: chrono::DateTime::parse_from_rfc3339("2026-04-10T01:02:03Z")
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                    updated_at: chrono::DateTime::parse_from_rfc3339("2026-04-10T01:02:03Z")
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                },
                score: 319,
                matched_terms: vec![
                    "auth".to_string(),
                    "callback".to_string(),
                    "recovery".to_string(),
                ],
                relation_count: 2,
                observation_count: 1,
                max_observation_priority: session::ContextObservationPriority::High,
                has_pinned_observation: true,
            }],
            Some("sess-12345678"),
            "auth callback recovery",
        );

        assert!(text.contains("Relevant memory: 1 entries"));
        assert!(text.contains("[file] callback.ts | score 319 | relations 2 | observations 1"));
        assert!(text.contains("priority high"));
        assert!(text.contains("| pinned"));
        assert!(text.contains("matches auth, callback, recovery"));
        assert!(text.contains("path src/routes/auth/callback.ts"));
    }

    #[test]
    fn format_graph_observations_human_renders_summaries() {
        let text = format_graph_observations_human(&[session::ContextGraphObservation {
            id: 5,
            session_id: Some("sess-12345678".to_string()),
            entity_id: 11,
            entity_type: "session".to_string(),
            entity_name: "sess-12345678".to_string(),
            observation_type: "completion_summary".to_string(),
            priority: session::ContextObservationPriority::High,
            pinned: true,
            summary: "Finished auth callback recovery with 2 tests".to_string(),
            details: BTreeMap::from([("tests_run".to_string(), "2".to_string())]),
            created_at: chrono::DateTime::parse_from_rfc3339("2026-04-10T01:02:03Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        }]);

        assert!(text.contains("Context graph observations: 1"));
        assert!(text.contains("[completion_summary/high/pinned] sess-12345678"));
        assert!(text.contains("summary Finished auth callback recovery with 2 tests"));
    }

    #[test]
    fn format_graph_compaction_stats_human_renders_counts() {
        let text = format_graph_compaction_stats_human(
            &session::ContextGraphCompactionStats {
                entities_scanned: 3,
                duplicate_observations_deleted: 2,
                overflow_observations_deleted: 4,
                observations_retained: 9,
            },
            Some("sess-12345678"),
            6,
        );

        assert!(text.contains("Context graph compaction complete for sess-123"));
        assert!(text.contains("keep 6 observations per entity"));
        assert!(text.contains("- entities scanned 3"));
        assert!(text.contains("- duplicate observations deleted 2"));
        assert!(text.contains("- overflow observations deleted 4"));
        assert!(text.contains("- observations retained 9"));
    }

    #[test]
    fn format_graph_connector_sync_stats_human_renders_counts() {
        let text = format_graph_connector_sync_stats_human(&GraphConnectorSyncStats {
            connector_name: "hermes_notes".to_string(),
            records_read: 4,
            entities_upserted: 3,
            observations_added: 3,
            skipped_records: 1,
            skipped_unchanged_sources: 2,
        });

        assert!(text.contains("Memory connector sync complete: hermes_notes"));
        assert!(text.contains("- records read 4"));
        assert!(text.contains("- entities upserted 3"));
        assert!(text.contains("- observations added 3"));
        assert!(text.contains("- skipped records 1"));
        assert!(text.contains("- skipped unchanged sources 2"));
    }

    #[test]
    fn format_graph_connector_sync_report_human_renders_totals_and_connectors() {
        let text = format_graph_connector_sync_report_human(&GraphConnectorSyncReport {
            connectors_synced: 2,
            records_read: 7,
            entities_upserted: 5,
            observations_added: 5,
            skipped_records: 2,
            skipped_unchanged_sources: 3,
            connectors: vec![
                GraphConnectorSyncStats {
                    connector_name: "hermes_notes".to_string(),
                    records_read: 4,
                    entities_upserted: 3,
                    observations_added: 3,
                    skipped_records: 1,
                    skipped_unchanged_sources: 2,
                },
                GraphConnectorSyncStats {
                    connector_name: "workspace_note".to_string(),
                    records_read: 3,
                    entities_upserted: 2,
                    observations_added: 2,
                    skipped_records: 1,
                    skipped_unchanged_sources: 1,
                },
            ],
        });

        assert!(text.contains("Memory connector sync complete: 2 connector(s)"));
        assert!(text.contains("- records read 7"));
        assert!(text.contains("- skipped unchanged sources 3"));
        assert!(text.contains("Connectors:"));
        assert!(text.contains("- hermes_notes"));
        assert!(text.contains("- workspace_note"));
        assert!(text.contains("  skipped unchanged sources 2"));
    }

    #[test]
    fn format_graph_connector_status_report_human_renders_connector_details() {
        let text = format_graph_connector_status_report_human(&GraphConnectorStatusReport {
            configured_connectors: 2,
            connectors: vec![
                GraphConnectorStatus {
                    connector_name: "hermes_notes".to_string(),
                    connector_kind: "jsonl_directory".to_string(),
                    source_path: "/tmp/hermes-notes".to_string(),
                    recurse: true,
                    default_session_id: Some("latest".to_string()),
                    default_entity_type: Some("incident".to_string()),
                    default_observation_type: Some("external_note".to_string()),
                    synced_sources: 3,
                    last_synced_at: Some(
                        chrono::DateTime::parse_from_rfc3339("2026-04-10T12:34:56Z")
                            .unwrap()
                            .with_timezone(&chrono::Utc),
                    ),
                },
                GraphConnectorStatus {
                    connector_name: "workspace_env".to_string(),
                    connector_kind: "dotenv_file".to_string(),
                    source_path: "/tmp/.env".to_string(),
                    recurse: false,
                    default_session_id: None,
                    default_entity_type: None,
                    default_observation_type: None,
                    synced_sources: 0,
                    last_synced_at: None,
                },
            ],
        });

        assert!(text.contains("Memory connectors: 2 configured"));
        assert!(text.contains("- hermes_notes [jsonl_directory]"));
        assert!(text.contains("  source /tmp/hermes-notes"));
        assert!(text.contains("  recurse true"));
        assert!(text.contains("  synced sources 3"));
        assert!(text.contains("  last synced 2026-04-10T12:34:56+00:00"));
        assert!(text.contains("  default session latest"));
        assert!(text.contains("  default entity type incident"));
        assert!(text.contains("  default observation type external_note"));
        assert!(text.contains("- workspace_env [dotenv_file]"));
        assert!(text.contains("  last synced never"));
    }

    #[test]
    fn memory_connector_status_report_includes_checkpoint_state() -> Result<()> {
        let tempdir = TestDir::new("graph-connector-status-report")?;
        let db = session::store::StateStore::open(&tempdir.path().join("state.db"))?;

        let markdown_path = tempdir.path().join("workspace-memory.md");
        fs::write(
            &markdown_path,
            r#"# Billing incident
Customer wiped setup and got charged twice after reinstalling.
"#,
        )?;

        let mut cfg = config::Config::default();
        cfg.memory_connectors.insert(
            "workspace_note".to_string(),
            config::MemoryConnectorConfig::MarkdownFile(
                config::MemoryConnectorMarkdownFileConfig {
                    path: markdown_path.clone(),
                    session_id: Some("latest".to_string()),
                    default_entity_type: Some("note_section".to_string()),
                    default_observation_type: Some("external_note".to_string()),
                },
            ),
        );
        cfg.memory_connectors.insert(
            "workspace_env".to_string(),
            config::MemoryConnectorConfig::DotenvFile(config::MemoryConnectorDotenvFileConfig {
                path: tempdir.path().join(".env"),
                session_id: None,
                default_entity_type: Some("service_config".to_string()),
                default_observation_type: Some("external_config".to_string()),
                key_prefixes: vec!["PUBLIC_".to_string()],
                include_keys: Vec::new(),
                exclude_keys: Vec::new(),
                include_safe_values: true,
            }),
        );

        db.upsert_connector_source_checkpoint(
            "workspace_note",
            &markdown_path.display().to_string(),
            "sig-a",
        )?;

        let report = memory_connector_status_report(&db, &cfg)?;
        assert_eq!(report.configured_connectors, 2);
        assert_eq!(
            report
                .connectors
                .iter()
                .map(|connector| connector.connector_name.as_str())
                .collect::<Vec<_>>(),
            vec!["workspace_env", "workspace_note"]
        );

        let workspace_env = report
            .connectors
            .iter()
            .find(|connector| connector.connector_name == "workspace_env")
            .expect("workspace_env connector should exist");
        assert_eq!(workspace_env.connector_kind, "dotenv_file");
        assert_eq!(workspace_env.synced_sources, 0);
        assert!(workspace_env.last_synced_at.is_none());

        let workspace_note = report
            .connectors
            .iter()
            .find(|connector| connector.connector_name == "workspace_note")
            .expect("workspace_note connector should exist");
        assert_eq!(workspace_note.connector_kind, "markdown_file");
        assert_eq!(
            workspace_note.source_path,
            markdown_path.display().to_string()
        );
        assert_eq!(workspace_note.default_session_id.as_deref(), Some("latest"));
        assert_eq!(
            workspace_note.default_entity_type.as_deref(),
            Some("note_section")
        );
        assert_eq!(
            workspace_note.default_observation_type.as_deref(),
            Some("external_note")
        );
        assert_eq!(workspace_note.synced_sources, 1);
        assert!(workspace_note.last_synced_at.is_some());

        Ok(())
    }

    #[test]
    fn sync_memory_connector_imports_jsonl_observations() -> Result<()> {
        let tempdir = TestDir::new("graph-connector-sync")?;
        let db = session::store::StateStore::open(&tempdir.path().join("state.db"))?;
        let now = chrono::Utc::now();
        db.insert_session(&session::Session {
            id: "session-1".to_string(),
            task: "recovery incident".to_string(),
            project: "ecc-tools".to_string(),
            task_group: "incident".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: session::SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: session::SessionMetrics::default(),
        })?;

        let connector_path = tempdir.path().join("hermes-memory.jsonl");
        std::fs::write(
            &connector_path,
            [
                serde_json::json!({
                    "entity_name": "Auth callback recovery",
                    "summary": "Customer wiped setup and got charged twice",
                    "details": {"customer": "viktor"}
                })
                .to_string(),
                serde_json::json!({
                    "session_id": "latest",
                    "entity_type": "file",
                    "entity_name": "callback.ts",
                    "path": "src/routes/auth/callback.ts",
                    "observation_type": "incident_note",
                    "summary": "Recovery flow needs portal-first routing"
                })
                .to_string(),
            ]
            .join("\n"),
        )?;

        let mut cfg = config::Config::default();
        cfg.memory_connectors.insert(
            "hermes_notes".to_string(),
            config::MemoryConnectorConfig::JsonlFile(config::MemoryConnectorJsonlFileConfig {
                path: connector_path,
                session_id: Some("latest".to_string()),
                default_entity_type: Some("incident".to_string()),
                default_observation_type: Some("external_note".to_string()),
            }),
        );

        let stats = sync_memory_connector(&db, &cfg, "hermes_notes", 10)?;
        assert_eq!(stats.records_read, 2);
        assert_eq!(stats.entities_upserted, 2);
        assert_eq!(stats.observations_added, 2);
        assert_eq!(stats.skipped_records, 0);

        let recalled = db.recall_context_entities(None, "charged twice routing", 5)?;
        assert_eq!(recalled.len(), 2);
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Auth callback recovery"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "callback.ts"));

        Ok(())
    }

    #[test]
    fn sync_memory_connector_skips_unchanged_jsonl_sources() -> Result<()> {
        let tempdir = TestDir::new("graph-connector-sync-unchanged")?;
        let db = session::store::StateStore::open(&tempdir.path().join("state.db"))?;
        let now = chrono::Utc::now();
        db.insert_session(&session::Session {
            id: "session-1".to_string(),
            task: "recovery incident".to_string(),
            project: "ecc-tools".to_string(),
            task_group: "incident".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: session::SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: session::SessionMetrics::default(),
        })?;

        let connector_path = tempdir.path().join("hermes-memory.jsonl");
        fs::write(
            &connector_path,
            serde_json::json!({
                "entity_name": "Portal routing",
                "summary": "Route reinstalls to portal before checkout",
            })
            .to_string(),
        )?;

        let mut cfg = config::Config::default();
        cfg.memory_connectors.insert(
            "hermes_notes".to_string(),
            config::MemoryConnectorConfig::JsonlFile(config::MemoryConnectorJsonlFileConfig {
                path: connector_path,
                session_id: Some("latest".to_string()),
                default_entity_type: Some("incident".to_string()),
                default_observation_type: Some("external_note".to_string()),
            }),
        );

        let first = sync_memory_connector(&db, &cfg, "hermes_notes", 10)?;
        assert_eq!(first.records_read, 1);
        assert_eq!(first.skipped_unchanged_sources, 0);

        let second = sync_memory_connector(&db, &cfg, "hermes_notes", 10)?;
        assert_eq!(second.records_read, 0);
        assert_eq!(second.entities_upserted, 0);
        assert_eq!(second.observations_added, 0);
        assert_eq!(second.skipped_unchanged_sources, 1);

        Ok(())
    }

    #[test]
    fn sync_memory_connector_imports_jsonl_directory_observations() -> Result<()> {
        let tempdir = TestDir::new("graph-connector-sync-dir")?;
        let db = session::store::StateStore::open(&tempdir.path().join("state.db"))?;
        let now = chrono::Utc::now();
        db.insert_session(&session::Session {
            id: "session-1".to_string(),
            task: "recovery incident".to_string(),
            project: "ecc-tools".to_string(),
            task_group: "incident".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: session::SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: session::SessionMetrics::default(),
        })?;

        let connector_dir = tempdir.path().join("hermes-memory");
        fs::create_dir_all(connector_dir.join("nested"))?;
        fs::write(
            connector_dir.join("a.jsonl"),
            [
                serde_json::json!({
                    "entity_name": "Auth callback recovery",
                    "summary": "Customer wiped setup and got charged twice",
                })
                .to_string(),
                serde_json::json!({
                    "entity_name": "Portal routing",
                    "summary": "Route existing installs to portal first",
                })
                .to_string(),
            ]
            .join("\n"),
        )?;
        fs::write(
            connector_dir.join("nested").join("b.jsonl"),
            [
                serde_json::json!({
                    "entity_name": "Billing UX note",
                    "summary": "Warn against buying twice after wiping setup",
                })
                .to_string(),
                "{invalid json}".to_string(),
            ]
            .join("\n"),
        )?;
        fs::write(connector_dir.join("ignore.txt"), "not imported")?;

        let mut cfg = config::Config::default();
        cfg.memory_connectors.insert(
            "hermes_dir".to_string(),
            config::MemoryConnectorConfig::JsonlDirectory(
                config::MemoryConnectorJsonlDirectoryConfig {
                    path: connector_dir,
                    recurse: true,
                    session_id: Some("latest".to_string()),
                    default_entity_type: Some("incident".to_string()),
                    default_observation_type: Some("external_note".to_string()),
                },
            ),
        );

        let stats = sync_memory_connector(&db, &cfg, "hermes_dir", 10)?;
        assert_eq!(stats.records_read, 4);
        assert_eq!(stats.entities_upserted, 3);
        assert_eq!(stats.observations_added, 3);
        assert_eq!(stats.skipped_records, 1);

        let recalled = db.recall_context_entities(None, "charged twice portal billing", 10)?;
        assert_eq!(recalled.len(), 3);
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Auth callback recovery"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Portal routing"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Billing UX note"));

        Ok(())
    }

    #[test]
    fn sync_memory_connector_imports_markdown_file_sections() -> Result<()> {
        let tempdir = TestDir::new("graph-connector-sync-markdown")?;
        let db = session::store::StateStore::open(&tempdir.path().join("state.db"))?;
        let now = chrono::Utc::now();
        db.insert_session(&session::Session {
            id: "session-1".to_string(),
            task: "knowledge import".to_string(),
            project: "everything-claude-code".to_string(),
            task_group: "memory".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: session::SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: session::SessionMetrics::default(),
        })?;

        let connector_path = tempdir.path().join("workspace-memory.md");
        fs::write(
            &connector_path,
            r#"# Billing incident
Customer wiped setup and got charged twice after reinstalling.

## Portal routing
Route existing installs to portal first before presenting checkout again.

## Docs fix
Guide users to repair before reinstall so wiped setups do not buy twice.
"#,
        )?;

        let mut cfg = config::Config::default();
        cfg.memory_connectors.insert(
            "workspace_note".to_string(),
            config::MemoryConnectorConfig::MarkdownFile(
                config::MemoryConnectorMarkdownFileConfig {
                    path: connector_path.clone(),
                    session_id: Some("latest".to_string()),
                    default_entity_type: Some("note_section".to_string()),
                    default_observation_type: Some("external_note".to_string()),
                },
            ),
        );

        let stats = sync_memory_connector(&db, &cfg, "workspace_note", 10)?;
        assert_eq!(stats.records_read, 3);
        assert_eq!(stats.entities_upserted, 3);
        assert_eq!(stats.observations_added, 3);
        assert_eq!(stats.skipped_records, 0);

        let recalled = db.recall_context_entities(None, "charged twice reinstall", 10)?;
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Billing incident"));
        assert!(recalled.iter().any(|entry| entry.entity.name == "Docs fix"));

        let billing = recalled
            .iter()
            .find(|entry| entry.entity.name == "Billing incident")
            .expect("billing section should exist");
        let expected_anchor_path = format!("{}#billing-incident", connector_path.display());
        assert_eq!(
            billing.entity.path.as_deref(),
            Some(expected_anchor_path.as_str())
        );
        let observations = db.list_context_observations(Some(billing.entity.id), 5)?;
        assert_eq!(observations.len(), 1);
        let expected_source_path = connector_path.display().to_string();
        assert_eq!(
            observations[0]
                .details
                .get("source_path")
                .map(String::as_str),
            Some(expected_source_path.as_str())
        );
        assert!(observations[0]
            .details
            .get("body")
            .is_some_and(|value: &String| value.contains("charged twice")));

        Ok(())
    }

    #[test]
    fn sync_memory_connector_imports_markdown_directory_sections() -> Result<()> {
        let tempdir = TestDir::new("graph-connector-sync-markdown-dir")?;
        let db = session::store::StateStore::open(&tempdir.path().join("state.db"))?;
        let now = chrono::Utc::now();
        db.insert_session(&session::Session {
            id: "session-1".to_string(),
            task: "knowledge import".to_string(),
            project: "everything-claude-code".to_string(),
            task_group: "memory".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: session::SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: session::SessionMetrics::default(),
        })?;

        let connector_dir = tempdir.path().join("workspace-notes");
        fs::create_dir_all(connector_dir.join("nested"))?;
        fs::write(
            connector_dir.join("incident.md"),
            r#"# Billing incident
Customer wiped setup and got charged twice after reinstalling.

## Portal routing
Route existing installs to portal first before presenting checkout again.
"#,
        )?;
        fs::write(
            connector_dir.join("nested").join("docs.markdown"),
            r#"# Docs fix
Guide users to repair before reinstall so wiped setups do not buy twice.
"#,
        )?;
        fs::write(connector_dir.join("ignore.txt"), "not imported")?;

        let mut cfg = config::Config::default();
        cfg.memory_connectors.insert(
            "workspace_notes".to_string(),
            config::MemoryConnectorConfig::MarkdownDirectory(
                config::MemoryConnectorMarkdownDirectoryConfig {
                    path: connector_dir.clone(),
                    recurse: true,
                    session_id: Some("latest".to_string()),
                    default_entity_type: Some("note_section".to_string()),
                    default_observation_type: Some("external_note".to_string()),
                },
            ),
        );

        let stats = sync_memory_connector(&db, &cfg, "workspace_notes", 10)?;
        assert_eq!(stats.records_read, 3);
        assert_eq!(stats.entities_upserted, 3);
        assert_eq!(stats.observations_added, 3);
        assert_eq!(stats.skipped_records, 0);

        let recalled = db.recall_context_entities(None, "charged twice portal docs", 10)?;
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Billing incident"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "Portal routing"));
        assert!(recalled.iter().any(|entry| entry.entity.name == "Docs fix"));

        let docs_fix = recalled
            .iter()
            .find(|entry| entry.entity.name == "Docs fix")
            .expect("docs section should exist");
        let expected_anchor_path = format!(
            "{}#docs-fix",
            connector_dir.join("nested").join("docs.markdown").display()
        );
        assert_eq!(
            docs_fix.entity.path.as_deref(),
            Some(expected_anchor_path.as_str())
        );

        Ok(())
    }

    #[test]
    fn sync_memory_connector_imports_dotenv_entries_safely() -> Result<()> {
        let tempdir = TestDir::new("graph-connector-sync-dotenv")?;
        let db = session::store::StateStore::open(&tempdir.path().join("state.db"))?;
        let now = chrono::Utc::now();
        db.insert_session(&session::Session {
            id: "session-1".to_string(),
            task: "service config import".to_string(),
            project: "ecc-tools".to_string(),
            task_group: "memory".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: session::SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: session::SessionMetrics::default(),
        })?;

        let connector_path = tempdir.path().join("hermes.env");
        fs::write(
            &connector_path,
            r#"# Hermes service config
STRIPE_SECRET_KEY=sk_test_secret
STRIPE_PRO_PRICE_ID=price_pro_monthly
PUBLIC_BASE_URL="https://ecc.tools"
STRIPE_WEBHOOK_SECRET=whsec_secret
GITHUB_TOKEN=ghp_should_not_import
INVALID LINE
"#,
        )?;

        let mut cfg = config::Config::default();
        cfg.memory_connectors.insert(
            "hermes_env".to_string(),
            config::MemoryConnectorConfig::DotenvFile(config::MemoryConnectorDotenvFileConfig {
                path: connector_path.clone(),
                session_id: Some("latest".to_string()),
                default_entity_type: Some("service_config".to_string()),
                default_observation_type: Some("external_config".to_string()),
                key_prefixes: vec!["STRIPE_".to_string(), "PUBLIC_".to_string()],
                include_keys: Vec::new(),
                exclude_keys: vec!["STRIPE_WEBHOOK_SECRET".to_string()],
                include_safe_values: true,
            }),
        );

        let stats = sync_memory_connector(&db, &cfg, "hermes_env", 10)?;
        assert_eq!(stats.records_read, 3);
        assert_eq!(stats.entities_upserted, 3);
        assert_eq!(stats.observations_added, 3);
        assert_eq!(stats.skipped_records, 0);

        let recalled = db.recall_context_entities(None, "stripe ecc.tools", 10)?;
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "STRIPE_SECRET_KEY"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "STRIPE_PRO_PRICE_ID"));
        assert!(recalled
            .iter()
            .any(|entry| entry.entity.name == "PUBLIC_BASE_URL"));
        assert!(!recalled
            .iter()
            .any(|entry| entry.entity.name == "STRIPE_WEBHOOK_SECRET"));
        assert!(!recalled
            .iter()
            .any(|entry| entry.entity.name == "GITHUB_TOKEN"));

        let secret = recalled
            .iter()
            .find(|entry| entry.entity.name == "STRIPE_SECRET_KEY")
            .expect("secret entry should exist");
        let secret_observations = db.list_context_observations(Some(secret.entity.id), 5)?;
        assert_eq!(secret_observations.len(), 1);
        assert_eq!(
            secret_observations[0]
                .details
                .get("secret_redacted")
                .map(String::as_str),
            Some("true")
        );
        assert!(!secret_observations[0].details.contains_key("value"));

        let public_base = recalled
            .iter()
            .find(|entry| entry.entity.name == "PUBLIC_BASE_URL")
            .expect("public base url should exist");
        let public_observations = db.list_context_observations(Some(public_base.entity.id), 5)?;
        assert_eq!(public_observations.len(), 1);
        assert_eq!(
            public_observations[0]
                .details
                .get("value")
                .map(String::as_str),
            Some("https://ecc.tools")
        );

        Ok(())
    }

    #[test]
    fn sync_all_memory_connectors_aggregates_results() -> Result<()> {
        let tempdir = TestDir::new("graph-connector-sync-all")?;
        let db = session::store::StateStore::open(&tempdir.path().join("state.db"))?;
        let now = chrono::Utc::now();
        db.insert_session(&session::Session {
            id: "session-1".to_string(),
            task: "memory import".to_string(),
            project: "everything-claude-code".to_string(),
            task_group: "memory".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: session::SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: session::SessionMetrics::default(),
        })?;

        let jsonl_path = tempdir.path().join("hermes-memory.jsonl");
        fs::write(
            &jsonl_path,
            serde_json::json!({
                "entity_name": "Portal routing",
                "summary": "Route reinstalls to portal before checkout",
            })
            .to_string(),
        )?;

        let markdown_path = tempdir.path().join("workspace-memory.md");
        fs::write(
            &markdown_path,
            r#"# Billing incident
Customer wiped setup and got charged twice after reinstalling.

## Docs fix
Guide users to repair before reinstall.
"#,
        )?;

        let mut cfg = config::Config::default();
        cfg.memory_connectors.insert(
            "hermes_notes".to_string(),
            config::MemoryConnectorConfig::JsonlFile(config::MemoryConnectorJsonlFileConfig {
                path: jsonl_path,
                session_id: Some("latest".to_string()),
                default_entity_type: Some("incident".to_string()),
                default_observation_type: Some("external_note".to_string()),
            }),
        );
        cfg.memory_connectors.insert(
            "workspace_note".to_string(),
            config::MemoryConnectorConfig::MarkdownFile(
                config::MemoryConnectorMarkdownFileConfig {
                    path: markdown_path,
                    session_id: Some("latest".to_string()),
                    default_entity_type: Some("note_section".to_string()),
                    default_observation_type: Some("external_note".to_string()),
                },
            ),
        );

        let report = sync_all_memory_connectors(&db, &cfg, 10)?;
        assert_eq!(report.connectors_synced, 2);
        assert_eq!(report.records_read, 3);
        assert_eq!(report.entities_upserted, 3);
        assert_eq!(report.observations_added, 3);
        assert_eq!(report.skipped_records, 0);
        assert_eq!(
            report
                .connectors
                .iter()
                .map(|stats| stats.connector_name.as_str())
                .collect::<Vec<_>>(),
            vec!["hermes_notes", "workspace_note"]
        );

        let recalled = db.recall_context_entities(None, "charged twice portal reinstall", 10)?;
        assert_eq!(recalled.len(), 3);

        Ok(())
    }

    #[test]
    fn format_graph_sync_stats_human_renders_counts() {
        let text = format_graph_sync_stats_human(
            &session::ContextGraphSyncStats {
                sessions_scanned: 2,
                decisions_processed: 3,
                file_events_processed: 5,
                messages_processed: 4,
            },
            Some("sess-12345678"),
        );

        assert!(text.contains("Context graph sync complete for sess-123"));
        assert!(text.contains("- sessions scanned 2"));
        assert!(text.contains("- decisions processed 3"));
        assert!(text.contains("- file events processed 5"));
        assert!(text.contains("- messages processed 4"));
    }

    #[test]
    fn cli_parses_coordination_status_json_flag() {
        let cli = Cli::try_parse_from(["ecc", "coordination-status", "--json"])
            .expect("coordination-status --json should parse");

        match cli.command {
            Some(Commands::CoordinationStatus { json, check }) => {
                assert!(json);
                assert!(!check);
            }
            _ => panic!("expected coordination-status subcommand"),
        }
    }

    #[test]
    fn cli_parses_coordination_status_check_flag() {
        let cli = Cli::try_parse_from(["ecc", "coordination-status", "--check"])
            .expect("coordination-status --check should parse");

        match cli.command {
            Some(Commands::CoordinationStatus { json, check }) => {
                assert!(!json);
                assert!(check);
            }
            _ => panic!("expected coordination-status subcommand"),
        }
    }

    #[test]
    fn cli_parses_maintain_coordination_command() {
        let cli = Cli::try_parse_from(["ecc", "maintain-coordination"])
            .expect("maintain-coordination should parse");

        match cli.command {
            Some(Commands::MaintainCoordination {
                agent,
                json,
                check,
                max_passes,
                ..
            }) => {
                assert!(agent.is_none());
                assert!(!json);
                assert!(!check);
                assert_eq!(max_passes, 5);
            }
            _ => panic!("expected maintain-coordination subcommand"),
        }
    }

    #[test]
    fn cli_parses_maintain_coordination_json_flag() {
        let cli = Cli::try_parse_from(["ecc", "maintain-coordination", "--json"])
            .expect("maintain-coordination --json should parse");

        match cli.command {
            Some(Commands::MaintainCoordination {
                json,
                check,
                max_passes,
                ..
            }) => {
                assert!(json);
                assert!(!check);
                assert_eq!(max_passes, 5);
            }
            _ => panic!("expected maintain-coordination subcommand"),
        }
    }

    #[test]
    fn cli_parses_maintain_coordination_check_flag() {
        let cli = Cli::try_parse_from(["ecc", "maintain-coordination", "--check"])
            .expect("maintain-coordination --check should parse");

        match cli.command {
            Some(Commands::MaintainCoordination {
                json,
                check,
                max_passes,
                ..
            }) => {
                assert!(!json);
                assert!(check);
                assert_eq!(max_passes, 5);
            }
            _ => panic!("expected maintain-coordination subcommand"),
        }
    }

    #[test]
    fn format_coordination_status_emits_json() {
        let status = session::manager::CoordinationStatus {
            backlog_leads: 2,
            backlog_messages: 5,
            absorbable_sessions: 1,
            saturated_sessions: 1,
            mode: session::manager::CoordinationMode::RebalanceFirstChronicSaturation,
            health: session::manager::CoordinationHealth::Saturated,
            operator_escalation_required: false,
            auto_dispatch_enabled: true,
            auto_dispatch_limit_per_session: 4,
            daemon_activity: session::store::DaemonActivity {
                last_dispatch_routed: 3,
                last_dispatch_deferred: 1,
                last_dispatch_leads: 2,
                ..Default::default()
            },
        };

        let rendered =
            format_coordination_status(&status, true).expect("json formatting should succeed");
        let value: serde_json::Value =
            serde_json::from_str(&rendered).expect("valid json should be emitted");
        assert_eq!(value["backlog_leads"], 2);
        assert_eq!(value["backlog_messages"], 5);
        assert_eq!(value["daemon_activity"]["last_dispatch_routed"], 3);
    }

    #[test]
    fn coordination_status_exit_codes_reflect_pressure() {
        let clear = session::manager::CoordinationStatus {
            backlog_leads: 0,
            backlog_messages: 0,
            absorbable_sessions: 0,
            saturated_sessions: 0,
            mode: session::manager::CoordinationMode::DispatchFirst,
            health: session::manager::CoordinationHealth::Healthy,
            operator_escalation_required: false,
            auto_dispatch_enabled: false,
            auto_dispatch_limit_per_session: 5,
            daemon_activity: Default::default(),
        };
        assert_eq!(coordination_status_exit_code(&clear), 0);

        let absorbable = session::manager::CoordinationStatus {
            backlog_messages: 2,
            backlog_leads: 1,
            absorbable_sessions: 1,
            health: session::manager::CoordinationHealth::BacklogAbsorbable,
            ..clear.clone()
        };
        assert_eq!(coordination_status_exit_code(&absorbable), 1);

        let saturated = session::manager::CoordinationStatus {
            saturated_sessions: 1,
            health: session::manager::CoordinationHealth::Saturated,
            ..absorbable
        };
        assert_eq!(coordination_status_exit_code(&saturated), 2);
    }

    #[test]
    fn summarize_coordinate_backlog_reports_clear_state() {
        let summary = summarize_coordinate_backlog(&session::manager::CoordinateBacklogOutcome {
            dispatched: Vec::new(),
            rebalanced: Vec::new(),
            remaining_backlog_sessions: 0,
            remaining_backlog_messages: 0,
            remaining_absorbable_sessions: 0,
            remaining_saturated_sessions: 0,
        });

        assert_eq!(summary.message, "Backlog already clear");
        assert_eq!(summary.processed, 0);
        assert_eq!(summary.rerouted, 0);
    }

    #[test]
    fn summarize_coordinate_backlog_structures_counts() {
        let summary = summarize_coordinate_backlog(&session::manager::CoordinateBacklogOutcome {
            dispatched: vec![session::manager::LeadDispatchOutcome {
                lead_session_id: "lead".into(),
                unread_count: 2,
                routed: vec![
                    session::manager::InboxDrainOutcome {
                        message_id: 1,
                        task: "one".into(),
                        session_id: "a".into(),
                        action: session::manager::AssignmentAction::Spawned,
                    },
                    session::manager::InboxDrainOutcome {
                        message_id: 2,
                        task: "two".into(),
                        session_id: "lead".into(),
                        action: session::manager::AssignmentAction::DeferredSaturated,
                    },
                ],
            }],
            rebalanced: vec![session::manager::LeadRebalanceOutcome {
                lead_session_id: "lead".into(),
                rerouted: vec![session::manager::RebalanceOutcome {
                    from_session_id: "a".into(),
                    message_id: 3,
                    task: "three".into(),
                    session_id: "b".into(),
                    action: session::manager::AssignmentAction::ReusedIdle,
                }],
            }],
            remaining_backlog_sessions: 1,
            remaining_backlog_messages: 2,
            remaining_absorbable_sessions: 1,
            remaining_saturated_sessions: 0,
        });

        assert_eq!(summary.processed, 2);
        assert_eq!(summary.routed, 1);
        assert_eq!(summary.deferred, 1);
        assert_eq!(summary.rerouted, 1);
        assert_eq!(summary.dispatched_leads, 1);
        assert_eq!(summary.rebalanced_leads, 1);
        assert_eq!(summary.remaining_backlog_messages, 2);
    }

    #[test]
    fn cli_parses_rebalance_team_command() {
        let cli = Cli::try_parse_from([
            "ecc",
            "rebalance-team",
            "lead",
            "--agent",
            "claude",
            "--limit",
            "2",
        ])
        .expect("rebalance-team should parse");

        match cli.command {
            Some(Commands::RebalanceTeam {
                session_id,
                agent,
                limit,
                ..
            }) => {
                assert_eq!(session_id, "lead");
                assert_eq!(agent.as_deref(), Some("claude"));
                assert_eq!(limit, 2);
            }
            _ => panic!("expected rebalance-team subcommand"),
        }
    }
