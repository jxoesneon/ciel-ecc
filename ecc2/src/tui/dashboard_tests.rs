    use anyhow::{Context, Result};
    use chrono::Utc;
    use ratatui::{backend::TestBackend, Terminal};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use uuid::Uuid;

    use super::*;
    use crate::config::{Config, PaneLayout, Theme};

    #[test]
    fn render_sessions_shows_summary_headers_and_selected_row() {
        let mut dashboard = test_dashboard(
            vec![
                sample_session(
                    "run-12345678",
                    "planner",
                    SessionState::Running,
                    Some("feat/run"),
                    128,
                    15,
                ),
                sample_session(
                    "done-87654321",
                    "reviewer",
                    SessionState::Completed,
                    Some("release/v1"),
                    2048,
                    125,
                ),
            ],
            1,
        );
        dashboard.approval_queue_counts = HashMap::from([(String::from("run-12345678"), 2usize)]);
        dashboard.approval_queue_preview = vec![SessionMessage {
            id: 1,
            from_session: "lead-12345678".to_string(),
            to_session: "run-12345678".to_string(),
            content: "{\"question\":\"Need approval to continue\"}".to_string(),
            msg_type: "query".to_string(),
            read: false,
            timestamp: Utc::now(),
        }];

        let rendered = render_dashboard_text(dashboard, 220, 24);
        assert!(rendered.contains("ID"));
        assert!(rendered.contains("Project"));
        assert!(rendered.contains("Group"));
        assert!(rendered.contains("Branch"));
        assert!(rendered.contains("Total 2"));
        assert!(rendered.contains("Running 1"));
        assert!(rendered.contains("Completed 1"));
        assert!(rendered.contains("Approval queue"));
        assert!(rendered.contains("done-876"));
    }

    #[test]
    fn approval_queue_preview_line_uses_target_session_and_preview() {
        let line = approval_queue_preview_line(&[SessionMessage {
            id: 1,
            from_session: "lead-12345678".to_string(),
            to_session: "run-12345678".to_string(),
            content: "{\"question\":\"Need approval to continue\"}".to_string(),
            msg_type: "query".to_string(),
            read: false,
            timestamp: Utc::now(),
        }])
        .expect("approval preview line");

        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(rendered.contains("run-123"));
        assert!(rendered.contains("query"));
    }

    #[test]
    fn sync_selected_messages_refreshes_approval_queue_after_marking_read() {
        let sessions = vec![
            sample_session(
                "lead-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/lead"),
                512,
                42,
            ),
            sample_session(
                "worker-123456",
                "reviewer",
                SessionState::Idle,
                Some("ecc/worker"),
                64,
                5,
            ),
        ];
        let mut dashboard = test_dashboard(sessions, 1);
        for session in &dashboard.sessions {
            dashboard.db.insert_session(session).unwrap();
        }
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-123456",
                "{\"question\":\"Need operator input\"}",
                "query",
            )
            .unwrap();
        dashboard.unread_message_counts = dashboard.db.unread_message_counts().unwrap();

        dashboard.sync_selected_messages();

        assert_eq!(dashboard.approval_queue_counts.get("worker-123456"), None);
        assert!(dashboard.approval_queue_preview.is_empty());
    }

    #[test]
    fn refresh_tracks_latest_unread_approval_before_selected_messages_mark_read() {
        let sessions = vec![sample_session(
            "worker-123456",
            "reviewer",
            SessionState::Idle,
            Some("ecc/worker"),
            64,
            5,
        )];
        let mut dashboard = test_dashboard(sessions, 0);
        for session in &dashboard.sessions {
            dashboard.db.insert_session(session).unwrap();
        }
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-123456",
                "{\"question\":\"Need operator input\"}",
                "query",
            )
            .unwrap();
        let message_id = dashboard
            .db
            .latest_unread_approval_message()
            .unwrap()
            .expect("approval message should exist")
            .id;

        dashboard.refresh();

        assert_eq!(dashboard.last_seen_approval_message_id, Some(message_id));
        assert!(dashboard.approval_queue_preview.is_empty());
    }

    #[test]
    fn focus_next_approval_target_selects_oldest_unread_target() {
        let sessions = vec![
            sample_session(
                "lead-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/lead"),
                512,
                42,
            ),
            sample_session(
                "worker-a",
                "reviewer",
                SessionState::Idle,
                Some("ecc/worker-a"),
                64,
                5,
            ),
            sample_session(
                "worker-b",
                "reviewer",
                SessionState::Idle,
                Some("ecc/worker-b"),
                64,
                5,
            ),
        ];
        let mut dashboard = test_dashboard(sessions, 0);
        for session in &dashboard.sessions {
            dashboard.db.insert_session(session).unwrap();
        }
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-b",
                "{\"question\":\"Need approval on B\"}",
                "query",
            )
            .unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-a",
                "{\"question\":\"Need approval on A\"}",
                "query",
            )
            .unwrap();
        dashboard.sync_approval_queue();

        dashboard.focus_next_approval_target();

        assert_eq!(dashboard.selected_session_id(), Some("worker-b"));
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("focused approval target worker-b")
        );
    }

    #[test]
    fn focus_next_approval_target_cycles_distinct_targets() {
        let sessions = vec![
            sample_session(
                "lead-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/lead"),
                512,
                42,
            ),
            sample_session(
                "worker-a",
                "reviewer",
                SessionState::Idle,
                Some("ecc/worker-a"),
                64,
                5,
            ),
            sample_session(
                "worker-b",
                "reviewer",
                SessionState::Idle,
                Some("ecc/worker-b"),
                64,
                5,
            ),
        ];
        let mut dashboard = test_dashboard(sessions, 1);
        for session in &dashboard.sessions {
            dashboard.db.insert_session(session).unwrap();
        }
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-a",
                "{\"question\":\"Need approval on A\"}",
                "query",
            )
            .unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-a",
                "{\"question\":\"Need another approval on A\"}",
                "conflict",
            )
            .unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-b",
                "{\"question\":\"Need approval on B\"}",
                "query",
            )
            .unwrap();
        dashboard.sync_approval_queue();

        dashboard.focus_next_approval_target();

        assert_eq!(dashboard.selected_session_id(), Some("worker-b"));
        assert_eq!(dashboard.approval_queue_counts.get("worker-a"), Some(&2));
        assert_eq!(dashboard.approval_queue_counts.get("worker-b"), None);
    }

    #[test]
    fn focus_next_approval_target_reports_clear_queue() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "lead-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/lead"),
                512,
                42,
            )],
            0,
        );

        dashboard.focus_next_approval_target();

        assert_eq!(dashboard.selected_session_id(), Some("lead-12345678"));
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("approval queue clear")
        );
    }

    #[test]
    fn selected_session_metrics_text_includes_worktree_output_and_attention_queue() {
        let mut dashboard = test_dashboard(
            vec![
                sample_session(
                    "focus-12345678",
                    "planner",
                    SessionState::Running,
                    Some("ecc/focus"),
                    512,
                    42,
                ),
                sample_session(
                    "failed-87654321",
                    "reviewer",
                    SessionState::Failed,
                    Some("ecc/failed"),
                    64,
                    5,
                ),
            ],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![test_output_line(OutputStream::Stdout, "last useful output")],
        );
        dashboard.selected_diff_summary = Some("1 file changed, 2 insertions(+)".to_string());
        dashboard.selected_diff_preview = vec![
            "Branch M src/main.rs".to_string(),
            "Working ?? notes.txt".to_string(),
        ];
        dashboard.selected_merge_readiness = Some(worktree::MergeReadiness {
            status: worktree::MergeReadinessStatus::Conflicted,
            summary: "Merge blocked by 1 conflict(s): src/main.rs".to_string(),
            conflicts: vec!["src/main.rs".to_string()],
        });

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Branch ecc/focus | Base main"));
        assert!(text.contains("Worktree /tmp/ecc/focus"));
        assert!(text.contains("Diff 1 file changed, 2 insertions(+)"));
        assert!(text.contains("Changed files"));
        assert!(text.contains("- Branch M src/main.rs"));
        assert!(text.contains("- Working ?? notes.txt"));
        assert!(text.contains("Merge blocked by 1 conflict(s): src/main.rs"));
        assert!(text.contains("- conflict src/main.rs"));
        assert!(text.contains("Tokens 512 total | In 384 | Out 128"));
        assert!(text.contains("Last output last useful output"));
        assert!(text.contains("Needs attention:"));
        assert!(text.contains("Failed failed-8 | Render dashboard rows"));
    }

    #[test]
    fn toggle_output_mode_switches_to_worktree_diff_preview() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.selected_diff_summary = Some("1 file changed".to_string());
        dashboard.selected_diff_patch = Some(
            "--- Branch diff vs main ---\ndiff --git a/src/lib.rs b/src/lib.rs\n@@ -1 +1 @@\n-old line\n+new line".to_string(),
        );

        dashboard.toggle_output_mode();

        assert_eq!(dashboard.output_mode, OutputMode::WorktreeDiff);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("showing selected worktree diff")
        );
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("Diff"));
        assert!(rendered.contains("Removals"));
        assert!(rendered.contains("Additions"));
        assert!(rendered.contains("-old line"));
        assert!(rendered.contains("+new line"));
    }

    #[test]
    fn toggle_git_status_mode_renders_selected_worktree_status() -> Result<()> {
        let root = std::env::temp_dir().join(format!("ecc2-git-status-{}", Uuid::new_v4()));
        init_git_repo(&root)?;
        fs::write(root.join("README.md"), "hello from git status\n")?;

        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.working_dir = root.clone();
        session.worktree = Some(WorktreeInfo {
            path: root.clone(),
            branch: "main".to_string(),
            base_branch: "main".to_string(),
        });
        let mut dashboard = test_dashboard(vec![session], 0);

        dashboard.toggle_git_status_mode();

        assert_eq!(dashboard.output_mode, OutputMode::GitStatus);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("showing selected worktree git status")
        );
        assert_eq!(
            dashboard.output_title(),
            " Git status staged:0 unstaged:1 1/1 "
        );
        let rendered = dashboard.rendered_output_text(180, 20);
        assert!(rendered.contains("Git status"));
        assert!(rendered.contains("README.md"));

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn toggle_output_mode_from_git_status_opens_selected_file_patch() -> Result<()> {
        let root = std::env::temp_dir().join(format!("ecc2-git-patch-view-{}", Uuid::new_v4()));
        init_git_repo(&root)?;
        fs::write(
            root.join("README.md"),
            "line 1\nline 2\nline 3\nline 4\nline 5\nline 6 updated\n",
        )?;

        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.working_dir = root.clone();
        session.worktree = Some(WorktreeInfo {
            path: root.clone(),
            branch: "main".to_string(),
            base_branch: "main".to_string(),
        });
        let mut dashboard = test_dashboard(vec![session], 0);
        let stored = dashboard.sessions[0].clone();
        dashboard.db.insert_session(&stored)?;

        dashboard.toggle_git_status_mode();
        dashboard.toggle_output_mode();

        assert_eq!(dashboard.output_mode, OutputMode::GitPatch);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("showing selected file patch")
        );
        assert!(dashboard.output_title().contains("Git patch README.md"));
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("Git patch README.md"));
        assert!(rendered.contains("+line 6 updated"));

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn git_patch_mode_stages_only_selected_hunk() -> Result<()> {
        let root = std::env::temp_dir().join(format!("ecc2-git-patch-stage-{}", Uuid::new_v4()));
        init_git_repo(&root)?;
        let original = (1..=12)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(root.join("notes.txt"), format!("{original}\n"))?;
        run_git(&root, &["add", "notes.txt"])?;
        run_git(&root, &["commit", "-qm", "add notes"])?;

        let updated = (1..=12)
            .map(|index| match index {
                2 => "line 2 changed".to_string(),
                11 => "line 11 changed".to_string(),
                _ => format!("line {index}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(root.join("notes.txt"), format!("{updated}\n"))?;

        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.working_dir = root.clone();
        session.worktree = Some(WorktreeInfo {
            path: root.clone(),
            branch: "main".to_string(),
            base_branch: "main".to_string(),
        });
        let mut dashboard = test_dashboard(vec![session], 0);
        let stored = dashboard.sessions[0].clone();
        dashboard.db.insert_session(&stored)?;

        dashboard.toggle_git_status_mode();
        dashboard.toggle_output_mode();
        dashboard.stage_selected_git_status();

        assert_eq!(dashboard.output_mode, OutputMode::GitPatch);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("staged hunk in notes.txt")
        );
        let cached = git_stdout(&root, &["diff", "--cached", "--", "notes.txt"])?;
        assert!(cached.contains("line 2 changed"));
        assert!(!cached.contains("line 11 changed"));
        let working = git_stdout(&root, &["diff", "--", "notes.txt"])?;
        assert!(!working.contains("line 2 changed"));
        assert!(working.contains("line 11 changed"));
        assert!(dashboard.output_title().contains("Git patch notes.txt"));

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn begin_commit_prompt_opens_commit_input_for_staged_entries() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.output_mode = OutputMode::GitStatus;
        dashboard.selected_git_status_entries = vec![worktree::GitStatusEntry {
            path: "README.md".to_string(),
            display_path: "README.md".to_string(),
            index_status: 'M',
            worktree_status: ' ',
            staged: true,
            unstaged: false,
            untracked: false,
            conflicted: false,
        }];

        dashboard.begin_commit_prompt();

        assert_eq!(dashboard.commit_input.as_deref(), Some(""));
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("commit mode | type a message and press Enter")
        );
        let rendered = render_dashboard_text(dashboard, 180, 20);
        assert!(rendered.contains("commit>_"));
    }

    #[test]
    fn begin_pr_prompt_seeds_latest_commit_subject() -> Result<()> {
        let root = std::env::temp_dir().join(format!("ecc2-pr-prompt-{}", Uuid::new_v4()));
        init_git_repo(&root)?;
        fs::write(root.join("README.md"), "seed pr title\n")?;
        run_git(&root, &["commit", "-am", "seed pr title"])?;

        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.working_dir = root.clone();
        session.worktree = Some(WorktreeInfo {
            path: root.clone(),
            branch: "main".to_string(),
            base_branch: "main".to_string(),
        });
        let mut dashboard = test_dashboard(vec![session], 0);

        dashboard.begin_pr_prompt();

        assert_eq!(dashboard.pr_input.as_deref(), Some("seed pr title"));
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("pr mode | title | base=branch | labels=a,b | reviewers=a,b")
        );

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn parse_pr_prompt_supports_base_labels_and_reviewers() {
        let parsed = parse_pr_prompt(
            "Improve retry flow | base=release/2.0 | labels=billing, ux | reviewers=alice, bob",
        )
        .expect("parse prompt");

        assert_eq!(parsed.title, "Improve retry flow");
        assert_eq!(parsed.base_branch.as_deref(), Some("release/2.0"));
        assert_eq!(parsed.labels, vec!["billing", "ux"]);
        assert_eq!(parsed.reviewers, vec!["alice", "bob"]);
    }

    #[test]
    fn submit_pr_prompt_passes_custom_metadata_to_gh() -> Result<()> {
        let temp_root =
            std::env::temp_dir().join(format!("ecc2-dashboard-pr-submit-{}", Uuid::new_v4()));
        let root = temp_root.join("repo");
        init_git_repo(&root)?;
        let remote = temp_root.join("remote.git");
        run_git(
            &root,
            &["init", "--bare", remote.to_str().expect("utf8 path")],
        )?;
        run_git(
            &root,
            &[
                "remote",
                "add",
                "origin",
                remote.to_str().expect("utf8 path"),
            ],
        )?;
        run_git(&root, &["push", "-u", "origin", "main"])?;
        run_git(&root, &["checkout", "-b", "feat/dashboard-pr"])?;
        fs::write(root.join("README.md"), "dashboard pr\n")?;
        run_git(&root, &["commit", "-am", "dashboard pr"])?;

        let bin_dir = temp_root.join("bin");
        fs::create_dir_all(&bin_dir)?;
        let gh_path = bin_dir.join("gh");
        let args_path = temp_root.join("gh-dashboard-args.txt");
        fs::write(
            &gh_path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nprintf '%s\\n' 'https://github.com/example/repo/pull/789'\n",
                args_path.display()
            ),
        )?;
        let mut perms = fs::metadata(&gh_path)?.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&gh_path, perms)?;
        }
        #[cfg(not(unix))]
        fs::set_permissions(&gh_path, perms)?;

        let original_path = std::env::var_os("PATH");
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin_dir.display(),
                original_path
                    .as_deref()
                    .map(std::ffi::OsStr::to_string_lossy)
                    .unwrap_or_default()
            ),
        );

        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.working_dir = root.clone();
        session.worktree = Some(WorktreeInfo {
            path: root.clone(),
            branch: "feat/dashboard-pr".to_string(),
            base_branch: "main".to_string(),
        });
        let mut dashboard = test_dashboard(vec![session], 0);
        dashboard.pr_input = Some(
            "Improve retry flow | base=release/2.0 | labels=billing,ux | reviewers=alice,bob"
                .to_string(),
        );

        dashboard.submit_pr_prompt();

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("created draft PR for focus-12 against release/2.0: https://github.com/example/repo/pull/789")
        );
        let gh_args = fs::read_to_string(&args_path)?;
        assert!(gh_args.contains("--base\nrelease/2.0"));
        assert!(gh_args.contains("--label\nbilling"));
        assert!(gh_args.contains("--label\nux"));
        assert!(gh_args.contains("--reviewer\nalice"));
        assert!(gh_args.contains("--reviewer\nbob"));

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        let _ = fs::remove_dir_all(temp_root);
        Ok(())
    }

    #[test]
    fn toggle_diff_view_mode_switches_to_unified_rendering() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        let patch = "--- Branch diff vs main ---\n\
diff --git a/src/lib.rs b/src/lib.rs\n\
@@ -1 +1 @@\n\
-old line\n\
+new line"
            .to_string();
        dashboard.selected_diff_summary = Some("1 file changed".to_string());
        dashboard.selected_diff_patch = Some(patch.clone());
        dashboard.selected_diff_hunk_offsets_split =
            build_worktree_diff_columns(&patch, dashboard.theme_palette()).hunk_offsets;
        dashboard.selected_diff_hunk_offsets_unified = build_unified_diff_hunk_offsets(&patch);
        dashboard.toggle_output_mode();

        dashboard.toggle_diff_view_mode();

        assert_eq!(dashboard.diff_view_mode, DiffViewMode::Unified);
        assert_eq!(dashboard.output_title(), " Diff unified 1/1 ");
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("diff view set to unified")
        );
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("Diff unified 1/1"));
        assert!(rendered.contains("@@ -1 +1 @@"));
        assert!(rendered.contains("-old line"));
        assert!(rendered.contains("+new line"));
        assert!(!rendered.contains("Removals"));
        assert!(!rendered.contains("Additions"));
    }

    #[test]
    fn diff_hunk_navigation_updates_scroll_offset_and_wraps() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        let patch = "--- Branch diff vs main ---\n\
diff --git a/src/lib.rs b/src/lib.rs\n\
@@ -1 +1 @@\n\
-old line\n\
+new line\n\
@@ -5 +5 @@\n\
-second old\n\
+second new"
            .to_string();
        dashboard.selected_diff_patch = Some(patch.clone());
        let split_offsets =
            build_worktree_diff_columns(&patch, dashboard.theme_palette()).hunk_offsets;
        dashboard.selected_diff_hunk_offsets_split = split_offsets.clone();
        dashboard.selected_diff_hunk_offsets_unified = build_unified_diff_hunk_offsets(&patch);
        dashboard.output_mode = OutputMode::WorktreeDiff;

        dashboard.next_diff_hunk();
        assert_eq!(dashboard.selected_diff_hunk, 1);
        assert_eq!(dashboard.output_scroll_offset, split_offsets[1]);
        assert_eq!(dashboard.output_title(), " Diff split 2/2 ");
        assert_eq!(dashboard.operator_note.as_deref(), Some("diff hunk 2/2"));

        dashboard.next_diff_hunk();
        assert_eq!(dashboard.selected_diff_hunk, 0);
        assert_eq!(dashboard.output_scroll_offset, split_offsets[0]);
        assert_eq!(dashboard.output_title(), " Diff split 1/2 ");
        assert_eq!(dashboard.operator_note.as_deref(), Some("diff hunk 1/2"));

        dashboard.prev_diff_hunk();
        assert_eq!(dashboard.selected_diff_hunk, 1);
        assert_eq!(dashboard.output_scroll_offset, split_offsets[1]);
        assert_eq!(dashboard.operator_note.as_deref(), Some("diff hunk 2/2"));
    }

    #[test]
    fn toggle_timeline_mode_renders_selected_session_events() {
        let now = Utc::now();
        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.created_at = now - chrono::Duration::hours(2);
        session.updated_at = now - chrono::Duration::minutes(5);
        session.metrics.files_changed = 3;

        let mut dashboard = test_dashboard(vec![session.clone()], 0);
        dashboard.db.insert_session(&session).unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "focus-12345678",
                "{\"question\":\"Need review\"}",
                "query",
            )
            .unwrap();
        dashboard
            .db
            .insert_tool_log(
                "focus-12345678",
                "bash",
                "cargo test -q",
                "{\"command\":\"cargo test -q\"}",
                "ok",
                "stabilize planner session",
                240,
                0.2,
                &(now - chrono::Duration::minutes(3)).to_rfc3339(),
            )
            .unwrap();

        dashboard.toggle_timeline_mode();

        assert_eq!(dashboard.output_mode, OutputMode::Timeline);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("showing selected session timeline")
        );
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("Timeline"));
        assert!(rendered.contains("created session as planner"));
        assert!(rendered.contains("received query lead-123"));
        assert!(rendered.contains("tool bash"));
        assert!(rendered.contains("why stabilize planner session"));
        assert!(rendered.contains("params {\"command\":\"cargo test -q\"}"));
        assert!(rendered.contains("files touched 3"));
    }

    #[test]
    fn cycle_timeline_event_filter_limits_rendered_events() {
        let now = Utc::now();
        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.created_at = now - chrono::Duration::hours(2);
        session.updated_at = now - chrono::Duration::minutes(5);
        session.metrics.files_changed = 1;

        let mut dashboard = test_dashboard(vec![session.clone()], 0);
        dashboard.db.insert_session(&session).unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "focus-12345678",
                "{\"question\":\"Need review\"}",
                "query",
            )
            .unwrap();
        dashboard
            .db
            .insert_tool_log(
                "focus-12345678",
                "bash",
                "cargo test -q",
                "{}",
                "ok",
                "",
                240,
                0.2,
                &(now - chrono::Duration::minutes(3)).to_rfc3339(),
            )
            .unwrap();
        dashboard.toggle_timeline_mode();

        dashboard.cycle_timeline_event_filter();
        dashboard.cycle_timeline_event_filter();

        assert_eq!(
            dashboard.timeline_event_filter,
            TimelineEventFilter::Messages
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("timeline filter set to messages")
        );
        assert_eq!(dashboard.output_title(), " Timeline messages ");

        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("received query lead-123"));
        assert!(!rendered.contains("tool bash"));
        assert!(!rendered.contains("files touched 1"));
    }

    #[test]
    fn timeline_and_metrics_render_recent_file_activity_details() -> Result<()> {
        let root = std::env::temp_dir().join(format!("ecc2-file-activity-{}", Uuid::new_v4()));
        fs::create_dir_all(&root)?;
        let now = Utc::now();
        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.created_at = now - chrono::Duration::hours(2);
        session.updated_at = now - chrono::Duration::minutes(5);

        let mut dashboard = test_dashboard(vec![session.clone()], 0);
        dashboard.db.insert_session(&session)?;

        let metrics_path = root.join("tool-usage.jsonl");
        fs::write(
            &metrics_path,
            concat!(
                "{\"id\":\"evt-1\",\"session_id\":\"focus-12345678\",\"tool_name\":\"Read\",\"input_summary\":\"Read src/lib.rs\",\"output_summary\":\"ok\",\"file_paths\":[\"src/lib.rs\"],\"timestamp\":\"2026-04-09T00:00:00Z\"}\n",
                "{\"id\":\"evt-2\",\"session_id\":\"focus-12345678\",\"tool_name\":\"Write\",\"input_summary\":\"Write README.md\",\"output_summary\":\"updated readme\",\"file_paths\":[\"README.md\"],\"file_events\":[{\"path\":\"README.md\",\"action\":\"create\",\"diff_preview\":\"+ # ECC 2.0\",\"patch_preview\":\"+ # ECC 2.0\\n+ \\n+ A richer dashboard\"}],\"timestamp\":\"2026-04-09T00:01:00Z\"}\n"
            ),
        )?;
        dashboard.db.sync_tool_activity_metrics(&metrics_path)?;
        dashboard.sync_from_store();

        dashboard.toggle_timeline_mode();
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("read src/lib.rs"));
        assert!(rendered.contains("create README.md"));
        assert!(rendered.contains("+ # ECC 2.0"));
        assert!(rendered.contains("+ A richer dashboard"));
        assert!(!rendered.contains("files touched 2"));

        let metrics_text = dashboard.selected_session_metrics_text();
        assert!(metrics_text.contains("Recent file activity"));
        assert!(metrics_text.contains("create README.md"));
        assert!(metrics_text.contains("+ # ECC 2.0"));
        assert!(metrics_text.contains("+ A richer dashboard"));
        assert!(metrics_text.contains("read src/lib.rs"));

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn metrics_text_surfaces_file_activity_conflicts() -> Result<()> {
        let root = std::env::temp_dir().join(format!("ecc2-file-overlaps-{}", Uuid::new_v4()));
        fs::create_dir_all(&root)?;
        let now = Utc::now();
        let mut focus = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        focus.created_at = now - chrono::Duration::hours(1);
        focus.updated_at = now - chrono::Duration::minutes(3);

        let mut delegate = sample_session(
            "delegate-87654321",
            "coder",
            SessionState::Idle,
            Some("ecc/delegate"),
            256,
            12,
        );
        delegate.created_at = now - chrono::Duration::minutes(50);
        delegate.updated_at = now - chrono::Duration::minutes(2);

        let mut dashboard = test_dashboard(vec![focus.clone(), delegate.clone()], 0);
        dashboard.db.insert_session(&focus)?;
        dashboard.db.insert_session(&delegate)?;

        let metrics_path = root.join("tool-usage.jsonl");
        fs::write(
            &metrics_path,
            concat!(
                "{\"id\":\"evt-1\",\"session_id\":\"focus-12345678\",\"tool_name\":\"Edit\",\"input_summary\":\"Edit src/lib.rs\",\"output_summary\":\"updated lib\",\"file_events\":[{\"path\":\"src/lib.rs\",\"action\":\"modify\"}],\"timestamp\":\"2026-04-09T00:00:00Z\"}\n",
                "{\"id\":\"evt-2\",\"session_id\":\"delegate-87654321\",\"tool_name\":\"Write\",\"input_summary\":\"Write src/lib.rs\",\"output_summary\":\"touched lib\",\"file_events\":[{\"path\":\"src/lib.rs\",\"action\":\"modify\"}],\"timestamp\":\"2026-04-09T00:01:00Z\"}\n"
            ),
        )?;
        dashboard.db.sync_tool_activity_metrics(&metrics_path)?;
        dashboard.sync_from_store();

        let metrics_text = dashboard.selected_session_metrics_text();
        assert!(metrics_text.contains("Active conflicts"));
        assert!(metrics_text.contains("src/lib.rs"));
        assert!(metrics_text.contains("escalate"));
        assert_eq!(
            dashboard
                .db
                .get_session("delegate-87654321")?
                .expect("delegate should exist")
                .state,
            SessionState::Stopped
        );

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn timeline_and_metrics_render_decision_log_entries() -> Result<()> {
        let now = Utc::now();
        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            256,
            7,
        );
        session.created_at = now - chrono::Duration::hours(1);
        session.updated_at = now - chrono::Duration::minutes(2);

        let mut dashboard = test_dashboard(vec![session.clone()], 0);
        dashboard.db.insert_session(&session)?;
        dashboard.db.insert_decision(
            &session.id,
            "Use sqlite for the shared context graph",
            &["json files".to_string(), "memory only".to_string()],
            "SQLite keeps the audit trail queryable from CLI and TUI.",
        )?;

        dashboard.toggle_timeline_mode();
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("decision"));
        assert!(rendered.contains("decided Use sqlite for the shared context graph"));
        assert!(rendered.contains("why SQLite keeps the audit trail queryable"));
        assert!(rendered.contains("alternative json files"));
        assert!(rendered.contains("alternative memory only"));

        let metrics_text = dashboard.selected_session_metrics_text();
        assert!(metrics_text.contains("Recent decisions"));
        assert!(metrics_text.contains("decided Use sqlite for the shared context graph"));
        assert!(metrics_text.contains("alternative json files"));

        dashboard.cycle_timeline_event_filter();
        dashboard.cycle_timeline_event_filter();
        dashboard.cycle_timeline_event_filter();
        dashboard.cycle_timeline_event_filter();
        dashboard.cycle_timeline_event_filter();

        assert_eq!(
            dashboard.timeline_event_filter,
            TimelineEventFilter::Decisions
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("timeline filter set to decisions")
        );
        assert_eq!(dashboard.output_title(), " Timeline decisions ");

        Ok(())
    }

    #[test]
    fn timeline_time_filter_hides_old_events() {
        let now = Utc::now();
        let mut session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        session.created_at = now - chrono::Duration::hours(3);
        session.updated_at = now - chrono::Duration::hours(2);

        let mut dashboard = test_dashboard(vec![session.clone()], 0);
        dashboard.db.insert_session(&session).unwrap();
        dashboard
            .db
            .insert_tool_log(
                "focus-12345678",
                "bash",
                "cargo test -q",
                "{}",
                "ok",
                "",
                240,
                0.2,
                &(now - chrono::Duration::minutes(3)).to_rfc3339(),
            )
            .unwrap();
        dashboard.toggle_timeline_mode();

        dashboard.cycle_output_time_filter();
        dashboard.cycle_output_time_filter();

        assert_eq!(dashboard.output_time_filter, OutputTimeFilter::LastHour);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("timeline range set to last 1h")
        );
        assert_eq!(dashboard.output_title(), " Timeline last 1h ");

        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("tool bash"));
        assert!(!rendered.contains("created session as planner"));
        assert!(!rendered.contains("state running"));
    }

    #[test]
    fn timeline_scope_all_sessions_renders_cross_session_events() {
        let now = Utc::now();
        let mut focus = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        );
        focus.created_at = now - chrono::Duration::hours(2);
        focus.updated_at = now - chrono::Duration::minutes(5);

        let mut review = sample_session(
            "review-87654321",
            "reviewer",
            SessionState::Idle,
            Some("ecc/review"),
            256,
            12,
        );
        review.created_at = now - chrono::Duration::hours(1);
        review.updated_at = now - chrono::Duration::minutes(3);
        review.metrics.files_changed = 2;

        let mut dashboard = test_dashboard(vec![focus.clone(), review.clone()], 0);
        dashboard.db.insert_session(&focus).unwrap();
        dashboard.db.insert_session(&review).unwrap();
        dashboard
            .db
            .insert_tool_log(
                "focus-12345678",
                "bash",
                "cargo test -q",
                "{}",
                "ok",
                "",
                240,
                0.2,
                &(now - chrono::Duration::minutes(4)).to_rfc3339(),
            )
            .unwrap();
        dashboard
            .db
            .insert_tool_log(
                "review-87654321",
                "git",
                "git status --short",
                "{}",
                "ok",
                "",
                120,
                0.1,
                &(now - chrono::Duration::minutes(2)).to_rfc3339(),
            )
            .unwrap();
        dashboard.toggle_timeline_mode();

        dashboard.toggle_search_scope();

        assert_eq!(dashboard.timeline_scope, SearchScope::AllSessions);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("timeline scope set to all sessions")
        );
        assert_eq!(dashboard.output_title(), " Timeline all sessions ");

        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("focus-12"));
        assert!(rendered.contains("review-8"));
        assert!(rendered.contains("tool bash"));
        assert!(rendered.contains("tool git"));
    }

    #[test]
    fn toggle_context_graph_mode_renders_selected_session_entities_and_relations() -> Result<()> {
        let session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            None,
            1,
            1,
        );
        let mut dashboard = test_dashboard(vec![session.clone()], 0);
        dashboard.db.insert_session(&session)?;

        let file = dashboard.db.upsert_context_entity(
            Some(&session.id),
            "file",
            "dashboard.rs",
            Some("ecc2/src/tui/dashboard.rs"),
            "dashboard renderer",
            &std::collections::BTreeMap::new(),
        )?;
        let function = dashboard.db.upsert_context_entity(
            Some(&session.id),
            "function",
            "render_output",
            None,
            "renders the output pane",
            &std::collections::BTreeMap::new(),
        )?;
        dashboard.db.upsert_context_relation(
            Some(&session.id),
            file.id,
            function.id,
            "contains",
            "output rendering path",
        )?;

        dashboard.toggle_context_graph_mode();

        assert_eq!(dashboard.output_mode, OutputMode::ContextGraph);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("showing selected session context graph")
        );
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("Graph"));
        assert!(rendered.contains("dashboard.rs"));
        assert!(rendered.contains("summary dashboard renderer"));
        assert!(rendered.contains("-> contains function:render_output"));
        Ok(())
    }

    #[test]
    fn cycle_graph_entity_filter_limits_rendered_entities() -> Result<()> {
        let session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            None,
            1,
            1,
        );
        let mut dashboard = test_dashboard(vec![session.clone()], 0);
        dashboard.db.insert_session(&session)?;
        dashboard.db.insert_decision(
            &session.id,
            "Use sqlite graph sync",
            &[],
            "Keeps shared memory queryable",
        )?;
        dashboard.db.upsert_context_entity(
            Some(&session.id),
            "file",
            "dashboard.rs",
            Some("ecc2/src/tui/dashboard.rs"),
            "dashboard renderer",
            &std::collections::BTreeMap::new(),
        )?;

        dashboard.toggle_context_graph_mode();
        dashboard.cycle_graph_entity_filter();

        assert_eq!(dashboard.graph_entity_filter, GraphEntityFilter::Decisions);
        assert_eq!(dashboard.output_title(), " Graph decisions ");
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("Use sqlite graph sync"));
        assert!(!rendered.contains("dashboard.rs"));

        dashboard.cycle_graph_entity_filter();
        assert_eq!(dashboard.graph_entity_filter, GraphEntityFilter::Files);
        assert_eq!(dashboard.output_title(), " Graph files ");
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("dashboard.rs"));
        assert!(!rendered.contains("Use sqlite graph sync"));
        Ok(())
    }

    #[test]
    fn graph_scope_all_sessions_renders_cross_session_entities() -> Result<()> {
        let focus = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            None,
            1,
            1,
        );
        let review = sample_session(
            "review-87654321",
            "reviewer",
            SessionState::Running,
            None,
            1,
            1,
        );
        let mut dashboard = test_dashboard(vec![focus.clone(), review.clone()], 0);
        dashboard.db.insert_session(&focus)?;
        dashboard.db.insert_session(&review)?;
        dashboard
            .db
            .insert_decision(&focus.id, "Alpha graph path", &[], "planner path")?;
        dashboard
            .db
            .insert_decision(&review.id, "Beta graph path", &[], "review path")?;

        dashboard.toggle_context_graph_mode();
        dashboard.toggle_search_scope();

        assert_eq!(dashboard.search_scope, SearchScope::AllSessions);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("graph scope set to all sessions")
        );
        assert_eq!(dashboard.output_title(), " Graph all sessions ");
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("focus-12"));
        assert!(rendered.contains("review-8"));
        assert!(rendered.contains("Alpha graph path"));
        assert!(rendered.contains("Beta graph path"));
        Ok(())
    }

    #[test]
    fn graph_search_matches_and_switches_selected_session() -> Result<()> {
        let focus = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            None,
            1,
            1,
        );
        let review = sample_session(
            "review-87654321",
            "reviewer",
            SessionState::Running,
            None,
            1,
            1,
        );
        let mut dashboard = test_dashboard(vec![focus.clone(), review.clone()], 0);
        dashboard.db.insert_session(&focus)?;
        dashboard.db.insert_session(&review)?;
        dashboard
            .db
            .insert_decision(&focus.id, "alpha local graph", &[], "planner path")?;
        dashboard
            .db
            .insert_decision(&review.id, "alpha remote graph", &[], "review path")?;

        dashboard.toggle_context_graph_mode();
        dashboard.toggle_search_scope();
        dashboard.cycle_graph_entity_filter();
        dashboard.begin_search();
        for ch in "alpha.*".chars() {
            dashboard.push_input_char(ch);
        }
        dashboard.submit_search();

        assert_eq!(dashboard.graph_entity_filter, GraphEntityFilter::Decisions);
        assert_eq!(dashboard.search_matches.len(), 2);
        let first_session = dashboard.selected_session_id().map(str::to_string);
        dashboard.next_search_match();
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("graph search /alpha.* match 2/2 | all sessions")
        );
        assert_ne!(
            dashboard.selected_session_id().map(str::to_string),
            first_session
        );
        Ok(())
    }

    #[test]
    fn graph_sessions_filter_renders_auto_session_relations() -> Result<()> {
        let session = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            None,
            1,
            1,
        );
        let mut dashboard = test_dashboard(vec![session.clone()], 0);
        dashboard.db.insert_session(&session)?;
        dashboard.db.insert_decision(
            &session.id,
            "Use graph relations",
            &[],
            "Edges make the context graph navigable",
        )?;

        dashboard.toggle_context_graph_mode();
        dashboard.cycle_graph_entity_filter();
        dashboard.cycle_graph_entity_filter();
        dashboard.cycle_graph_entity_filter();
        dashboard.cycle_graph_entity_filter();

        assert_eq!(dashboard.graph_entity_filter, GraphEntityFilter::Sessions);
        assert_eq!(dashboard.output_title(), " Graph sessions ");
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("focus-12345678"));
        assert!(rendered.contains("summary running | planner |"));
        assert!(rendered.contains("-> decided decision:Use graph relations"));
        Ok(())
    }

    #[test]
    fn selected_session_metrics_text_includes_context_graph_relations() -> Result<()> {
        let focus = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            None,
            1,
            1,
        );
        let delegate = sample_session("delegate-87654321", "coder", SessionState::Idle, None, 1, 1);
        let dashboard = test_dashboard(vec![focus.clone(), delegate.clone()], 0);
        dashboard.db.insert_session(&focus)?;
        dashboard.db.insert_session(&delegate)?;
        dashboard.db.insert_decision(
            &focus.id,
            "Use sqlite graph sync",
            &[],
            "Keeps shared memory queryable",
        )?;
        dashboard.db.send_message(
            &focus.id,
            &delegate.id,
            "{\"task\":\"Review graph edge\",\"context\":\"coordination smoke\"}",
            "task_handoff",
        )?;

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Context graph"));
        assert!(text.contains("outgoing 2 | incoming 0"));
        assert!(text.contains("-> decided decision:Use sqlite graph sync"));
        assert!(text.contains("-> delegates_to session:delegate-87654321"));
        Ok(())
    }

    #[test]
    fn selected_session_metrics_text_includes_relevant_memory() -> Result<()> {
        let mut focus = sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            None,
            1,
            1,
        );
        focus.task = "Investigate auth callback recovery".to_string();
        let mut memory = sample_session("memory-87654321", "coder", SessionState::Idle, None, 1, 1);
        memory.task = "Auth callback recovery notes".to_string();
        let dashboard = test_dashboard(vec![focus.clone(), memory.clone()], 0);
        dashboard.db.insert_session(&focus)?;
        dashboard.db.insert_session(&memory)?;
        dashboard.db.upsert_context_entity(
            Some(&memory.id),
            "file",
            "callback.ts",
            Some("src/routes/auth/callback.ts"),
            "Handles auth callback recovery and billing fallback",
            &BTreeMap::from([("area".to_string(), "auth".to_string())]),
        )?;
        let entity = dashboard
            .db
            .list_context_entities(Some(&memory.id), Some("file"), 10)?
            .into_iter()
            .find(|entry| entry.name == "callback.ts")
            .expect("callback entity");
        dashboard.db.add_context_observation(
            Some(&memory.id),
            entity.id,
            "completion_summary",
            ContextObservationPriority::Normal,
            true,
            "Recovered auth callback incident with billing fallback",
            &BTreeMap::new(),
        )?;

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Relevant memory"));
        assert!(text.contains("[file] callback.ts"));
        assert!(text.contains("| pinned"));
        assert!(text.contains("matches auth, callback, recovery"));
        assert!(text.contains(
            "memory [normal/pinned] Recovered auth callback incident with billing fallback"
        ));
        Ok(())
    }

    #[test]
    fn worktree_diff_columns_split_removed_and_added_lines() {
        let patch = "\
--- Branch diff vs main ---
diff --git a/src/lib.rs b/src/lib.rs
@@ -1,2 +1,2 @@
-old line
 context
+new line

--- Working tree diff ---
diff --git a/src/next.rs b/src/next.rs
@@ -3 +3 @@
-bye
+hello";

        let palette = test_dashboard(Vec::new(), 0).theme_palette();
        let columns = build_worktree_diff_columns(patch, palette);
        let removals = text_plain_text(&columns.removals);
        let additions = text_plain_text(&columns.additions);
        assert!(removals.contains("Branch diff vs main"));
        assert!(removals.contains("-old line"));
        assert!(removals.contains("-bye"));
        assert!(additions.contains("Working tree diff"));
        assert!(additions.contains("+new line"));
        assert!(additions.contains("+hello"));
    }

    #[test]
    fn split_diff_highlights_changed_words() {
        let palette = test_dashboard(Vec::new(), 0).theme_palette();
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
@@ -1 +1 @@
-old line
+new line";

        let columns = build_worktree_diff_columns(patch, palette);
        let removal = columns
            .removals
            .lines
            .iter()
            .find(|line| line_plain_text(line) == "-old line")
            .expect("removal line");
        let addition = columns
            .additions
            .lines
            .iter()
            .find(|line| line_plain_text(line) == "+new line")
            .expect("addition line");

        assert_eq!(removal.spans[1].content.as_ref(), "old");
        assert_eq!(removal.spans[1].style, diff_removal_word_style());
        assert_eq!(removal.spans[2].content.as_ref(), " ");
        assert_eq!(removal.spans[2].style, diff_removal_style(palette));
        assert_eq!(addition.spans[1].content.as_ref(), "new");
        assert_eq!(addition.spans[1].style, diff_addition_word_style());
    }

    #[test]
    fn unified_diff_highlights_changed_words() {
        let palette = test_dashboard(Vec::new(), 0).theme_palette();
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
@@ -1 +1 @@
-old line
+new line";

        let text = build_unified_diff_text(patch, palette);
        let removal = text
            .lines
            .iter()
            .find(|line| line_plain_text(line) == "-old line")
            .expect("removal line");
        let addition = text
            .lines
            .iter()
            .find(|line| line_plain_text(line) == "+new line")
            .expect("addition line");

        assert_eq!(removal.spans[1].content.as_ref(), "old");
        assert_eq!(removal.spans[1].style, diff_removal_word_style());
        assert_eq!(addition.spans[1].content.as_ref(), "new");
        assert_eq!(addition.spans[1].style, diff_addition_word_style());
    }

    #[test]
    fn toggle_conflict_protocol_mode_switches_to_protocol_view() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.selected_merge_readiness = Some(worktree::MergeReadiness {
            status: worktree::MergeReadinessStatus::Conflicted,
            summary: "Merge blocked by 1 conflict(s): src/main.rs".to_string(),
            conflicts: vec!["src/main.rs".to_string()],
        });
        dashboard.selected_conflict_protocol = Some(
            "Conflict protocol for focus-12\nResolution steps\n1. Inspect current patch: ecc worktree-status focus-12345678 --patch"
                .to_string(),
        );

        dashboard.toggle_conflict_protocol_mode();

        assert_eq!(dashboard.output_mode, OutputMode::ConflictProtocol);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("showing worktree conflict protocol")
        );
        let rendered = dashboard.rendered_output_text(180, 30);
        assert!(rendered.contains("Conflict Protocol"));
        assert!(rendered.contains("Resolution steps"));
    }

    #[test]
    fn selected_session_metrics_text_includes_team_capacity_summary() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.selected_team_summary = Some(TeamSummary {
            total: 3,
            idle: 1,
            running: 1,
            pending: 1,
            stale: 0,
            failed: 0,
            stopped: 0,
        });
        dashboard.global_handoff_backlog_leads = 2;
        dashboard.global_handoff_backlog_messages = 5;
        dashboard.selected_route_preview = Some("reuse idle worker-1".to_string());

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Team 3/8 | idle 1 | running 1 | pending 1 | failed 0 | stopped 0"));
        assert!(text.contains(
            "Global handoff backlog 2 lead(s) / 5 handoff(s) | Auto-dispatch off @ 5/lead | Auto-worktree on | Auto-merge off"
        ));
        assert!(text.contains("Coordination mode dispatch-first"));
        assert!(text.contains("Next route reuse idle worker-1"));
    }

    #[test]
    fn selected_session_metrics_text_includes_delegate_task_board() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.selected_child_sessions = vec![DelegatedChildSummary {
            session_id: "delegate-12345678".to_string(),
            state: SessionState::Running,
            worktree_health: Some(worktree::WorktreeHealth::Conflicted),
            approval_backlog: 1,
            handoff_backlog: 2,
            tokens_used: 1_280,
            files_changed: 3,
            duration_secs: 12,
            task_preview: "Implement rust tui delegate board".to_string(),
            branch: Some("ecc/delegate-12345678".to_string()),
            last_output_preview: Some("Investigating pane selection behavior".to_string()),
        }];

        let text = dashboard.selected_session_metrics_text();
        assert!(
            text.contains(
                "- delegate [Running] | next resolve conflict | worktree conflicted | approvals 1 | backlog 2 | progress 1,280 tok / 3 files / 00:00:12 | task Implement rust tui delegate board | branch ecc/delegate-12345678"
            )
        );
        assert!(text.contains("  last output Investigating pane selection behavior"));
    }

    #[test]
    fn selected_session_metrics_text_marks_focused_delegate_row() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.selected_child_sessions = vec![
            DelegatedChildSummary {
                session_id: "delegate-12345678".to_string(),
                state: SessionState::Running,
                worktree_health: None,
                approval_backlog: 0,
                handoff_backlog: 0,
                tokens_used: 128,
                files_changed: 1,
                duration_secs: 5,
                task_preview: "First delegate".to_string(),
                branch: None,
                last_output_preview: None,
            },
            DelegatedChildSummary {
                session_id: "delegate-22345678".to_string(),
                state: SessionState::Idle,
                worktree_health: Some(worktree::WorktreeHealth::InProgress),
                approval_backlog: 1,
                handoff_backlog: 2,
                tokens_used: 64,
                files_changed: 2,
                duration_secs: 10,
                task_preview: "Second delegate".to_string(),
                branch: Some("ecc/delegate-22345678".to_string()),
                last_output_preview: Some("Waiting on approval".to_string()),
            },
        ];
        dashboard.focused_delegate_session_id = Some("delegate-22345678".to_string());

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("- delegate [Running] | next let it run"));
        assert!(text.contains(
            ">> delegate [Idle] | next review approvals | worktree in progress | approvals 1 | backlog 2 | progress 64 tok / 2 files / 00:00:10 | task Second delegate | branch ecc/delegate-22345678"
        ));
        assert!(text.contains("  last output Waiting on approval"));
    }

    #[test]
    fn focus_next_delegate_wraps_across_delegate_board() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.selected_child_sessions = vec![
            DelegatedChildSummary {
                session_id: "delegate-12345678".to_string(),
                state: SessionState::Running,
                worktree_health: None,
                approval_backlog: 0,
                handoff_backlog: 0,
                tokens_used: 128,
                files_changed: 1,
                duration_secs: 5,
                task_preview: "First delegate".to_string(),
                branch: None,
                last_output_preview: None,
            },
            DelegatedChildSummary {
                session_id: "delegate-22345678".to_string(),
                state: SessionState::Idle,
                worktree_health: None,
                approval_backlog: 0,
                handoff_backlog: 0,
                tokens_used: 64,
                files_changed: 2,
                duration_secs: 10,
                task_preview: "Second delegate".to_string(),
                branch: None,
                last_output_preview: None,
            },
        ];
        dashboard.focused_delegate_session_id = Some("delegate-12345678".to_string());

        dashboard.focus_next_delegate();
        assert_eq!(
            dashboard.focused_delegate_session_id.as_deref(),
            Some("delegate-22345678")
        );

        dashboard.focus_next_delegate();
        assert_eq!(
            dashboard.focused_delegate_session_id.as_deref(),
            Some("delegate-12345678")
        );
    }

    #[test]
    fn open_focused_delegate_switches_selected_session() {
        let sessions = vec![
            sample_session(
                "lead-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/lead"),
                512,
                42,
            ),
            sample_session(
                "delegate-12345678",
                "claude",
                SessionState::Running,
                Some("ecc/delegate"),
                256,
                12,
            ),
        ];
        let mut dashboard = test_dashboard(sessions, 0);
        dashboard.selected_child_sessions = vec![DelegatedChildSummary {
            session_id: "delegate-12345678".to_string(),
            state: SessionState::Running,
            worktree_health: Some(worktree::WorktreeHealth::InProgress),
            approval_backlog: 1,
            handoff_backlog: 0,
            tokens_used: 256,
            files_changed: 2,
            duration_secs: 12,
            task_preview: "Investigate focused delegate navigation".to_string(),
            branch: Some("ecc/delegate".to_string()),
            last_output_preview: Some("Reviewing lead metrics".to_string()),
        }];
        dashboard.focused_delegate_session_id = Some("delegate-12345678".to_string());
        dashboard.output_follow = false;
        dashboard.output_scroll_offset = 9;
        dashboard.metrics_scroll_offset = 4;

        dashboard.open_focused_delegate();

        assert_eq!(dashboard.selected_session_id(), Some("delegate-12345678"));
        assert!(dashboard.output_follow);
        assert_eq!(dashboard.output_scroll_offset, 0);
        assert_eq!(dashboard.metrics_scroll_offset, 0);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("opened delegate delegate")
        );
    }

    #[test]
    fn selected_session_metrics_text_shows_worktree_and_auto_merge_policy_state() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.cfg.auto_dispatch_unread_handoffs = true;
        dashboard.cfg.auto_create_worktrees = false;
        dashboard.cfg.auto_merge_ready_worktrees = true;
        dashboard.global_handoff_backlog_leads = 1;
        dashboard.global_handoff_backlog_messages = 2;

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains(
            "Global handoff backlog 1 lead(s) / 2 handoff(s) | Auto-dispatch on @ 5/lead | Auto-worktree off | Auto-merge on"
        ));
    }

    #[test]
    fn toggle_auto_worktree_policy_persists_config() {
        let tempdir = std::env::temp_dir().join(format!("ecc2-worktree-policy-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tempdir).unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &tempdir);

        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.cfg.auto_create_worktrees = true;

        dashboard.toggle_auto_worktree_policy();

        assert!(!dashboard.cfg.auto_create_worktrees);
        let expected_note = format!(
            "default worktree creation disabled | saved to {}",
            crate::config::Config::config_path().display()
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some(expected_note.as_str())
        );

        let saved = std::fs::read_to_string(crate::config::Config::config_path()).unwrap();
        assert!(saved.contains("auto_create_worktrees = false"));

        if let Some(home) = previous_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
        let _ = std::fs::remove_dir_all(tempdir);
    }

    #[test]
    fn selected_session_metrics_text_includes_daemon_activity() {
        let now = Utc::now();
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.daemon_activity = DaemonActivity {
            last_dispatch_at: Some(now),
            last_dispatch_routed: 4,
            last_dispatch_deferred: 2,
            last_dispatch_leads: 2,
            chronic_saturation_streak: 0,
            last_recovery_dispatch_at: Some(now + chrono::Duration::seconds(1)),
            last_recovery_dispatch_routed: 1,
            last_recovery_dispatch_leads: 1,
            last_rebalance_at: Some(now + chrono::Duration::seconds(2)),
            last_rebalance_rerouted: 1,
            last_rebalance_leads: 1,
            last_auto_merge_at: Some(now + chrono::Duration::seconds(3)),
            last_auto_merge_merged: 2,
            last_auto_merge_active_skipped: 1,
            last_auto_merge_conflicted_skipped: 1,
            last_auto_merge_dirty_skipped: 0,
            last_auto_merge_failed: 0,
            last_auto_prune_at: Some(now + chrono::Duration::seconds(4)),
            last_auto_prune_pruned: 3,
            last_auto_prune_active_skipped: 1,
        };

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Coordination mode dispatch-first"));
        assert!(text.contains("Chronic saturation cleared @"));
        assert!(text.contains("Last daemon dispatch 4 routed / 2 deferred across 2 lead(s)"));
        assert!(text.contains("Last daemon recovery dispatch 1 handoff(s) across 1 lead(s)"));
        assert!(text.contains("Last daemon rebalance 1 handoff(s) across 1 lead(s)"));
        assert!(text.contains(
            "Last daemon auto-merge 2 merged / 1 active / 1 conflicted / 0 dirty / 0 failed"
        ));
        assert!(text.contains("Last daemon auto-prune 3 pruned / 1 active"));
    }

    #[test]
    fn selected_session_metrics_text_shows_rebalance_first_mode_when_saturation_is_unrecovered() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.daemon_activity = DaemonActivity {
            last_dispatch_at: Some(Utc::now()),
            last_dispatch_routed: 0,
            last_dispatch_deferred: 1,
            last_dispatch_leads: 1,
            chronic_saturation_streak: 1,
            last_recovery_dispatch_at: None,
            last_recovery_dispatch_routed: 0,
            last_recovery_dispatch_leads: 0,
            last_rebalance_at: Some(Utc::now()),
            last_rebalance_rerouted: 1,
            last_rebalance_leads: 1,
            last_auto_merge_at: None,
            last_auto_merge_merged: 0,
            last_auto_merge_active_skipped: 0,
            last_auto_merge_conflicted_skipped: 0,
            last_auto_merge_dirty_skipped: 0,
            last_auto_merge_failed: 0,
            last_auto_prune_at: None,
            last_auto_prune_pruned: 0,
            last_auto_prune_active_skipped: 0,
        };

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Coordination mode rebalance-first (chronic saturation)"));
    }

    #[test]
    fn selected_session_metrics_text_shows_rebalance_cooloff_mode_when_saturation_is_chronic() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.daemon_activity = DaemonActivity {
            last_dispatch_at: Some(Utc::now()),
            last_dispatch_routed: 0,
            last_dispatch_deferred: 3,
            last_dispatch_leads: 1,
            chronic_saturation_streak: 3,
            last_recovery_dispatch_at: None,
            last_recovery_dispatch_routed: 0,
            last_recovery_dispatch_leads: 0,
            last_rebalance_at: Some(Utc::now()),
            last_rebalance_rerouted: 1,
            last_rebalance_leads: 1,
            last_auto_merge_at: None,
            last_auto_merge_merged: 0,
            last_auto_merge_active_skipped: 0,
            last_auto_merge_conflicted_skipped: 0,
            last_auto_merge_dirty_skipped: 0,
            last_auto_merge_failed: 0,
            last_auto_prune_at: None,
            last_auto_prune_pruned: 0,
            last_auto_prune_active_skipped: 0,
        };

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Coordination mode rebalance-cooloff (chronic saturation)"));
        assert!(text.contains("Chronic saturation streak 3 cycle(s)"));
    }

    #[test]
    fn selected_session_metrics_text_recommends_operator_escalation_when_chronic_saturation_is_stuck(
    ) {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.daemon_activity = DaemonActivity {
            last_dispatch_at: Some(Utc::now()),
            last_dispatch_routed: 0,
            last_dispatch_deferred: 2,
            last_dispatch_leads: 1,
            chronic_saturation_streak: 5,
            last_recovery_dispatch_at: None,
            last_recovery_dispatch_routed: 0,
            last_recovery_dispatch_leads: 0,
            last_rebalance_at: Some(Utc::now()),
            last_rebalance_rerouted: 0,
            last_rebalance_leads: 1,
            last_auto_merge_at: None,
            last_auto_merge_merged: 0,
            last_auto_merge_active_skipped: 0,
            last_auto_merge_conflicted_skipped: 0,
            last_auto_merge_dirty_skipped: 0,
            last_auto_merge_failed: 0,
            last_auto_prune_at: None,
            last_auto_prune_pruned: 0,
            last_auto_prune_active_skipped: 0,
        };

        let text = dashboard.selected_session_metrics_text();
        assert!(
            text.contains("Operator escalation recommended: chronic saturation is not clearing")
        );
    }

    #[test]
    fn selected_session_metrics_text_shows_stabilized_dispatch_mode_after_recovery() {
        let now = Utc::now();
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );
        dashboard.daemon_activity = DaemonActivity {
            last_dispatch_at: Some(now + chrono::Duration::seconds(2)),
            last_dispatch_routed: 2,
            last_dispatch_deferred: 0,
            last_dispatch_leads: 1,
            chronic_saturation_streak: 0,
            last_recovery_dispatch_at: Some(now + chrono::Duration::seconds(1)),
            last_recovery_dispatch_routed: 1,
            last_recovery_dispatch_leads: 1,
            last_rebalance_at: Some(now),
            last_rebalance_rerouted: 1,
            last_rebalance_leads: 1,
            last_auto_merge_at: None,
            last_auto_merge_merged: 0,
            last_auto_merge_active_skipped: 0,
            last_auto_merge_conflicted_skipped: 0,
            last_auto_merge_dirty_skipped: 0,
            last_auto_merge_failed: 0,
            last_auto_prune_at: None,
            last_auto_prune_pruned: 0,
            last_auto_prune_active_skipped: 0,
        };

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Coordination mode dispatch-first (stabilized)"));
        assert!(text.contains("Recovery stabilized @"));
        assert!(!text.contains("Last daemon recovery dispatch"));
        assert!(!text.contains("Last daemon rebalance"));
    }

    #[test]
    fn attention_queue_suppresses_inbox_pressure_when_stabilized() {
        let now = Utc::now();
        let sessions = vec![sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        )];
        let unread = HashMap::from([(String::from("focus-12345678"), 3usize)]);
        let summary = SessionSummary::from_sessions(&sessions, &unread, &HashMap::new(), true);

        let line = attention_queue_line(&summary, true);
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains("Attention queue clear"));
        assert!(rendered.contains("stabilized backlog absorbed"));

        let mut dashboard = test_dashboard(sessions, 0);
        dashboard.unread_message_counts = unread;
        dashboard.handoff_backlog_counts =
            HashMap::from([(String::from("focus-12345678"), 3usize)]);
        dashboard.daemon_activity = DaemonActivity {
            last_dispatch_at: Some(now + chrono::Duration::seconds(2)),
            last_dispatch_routed: 2,
            last_dispatch_deferred: 0,
            last_dispatch_leads: 1,
            chronic_saturation_streak: 0,
            last_recovery_dispatch_at: Some(now + chrono::Duration::seconds(1)),
            last_recovery_dispatch_routed: 1,
            last_recovery_dispatch_leads: 1,
            last_rebalance_at: Some(now),
            last_rebalance_rerouted: 1,
            last_rebalance_leads: 1,
            last_auto_merge_at: None,
            last_auto_merge_merged: 0,
            last_auto_merge_active_skipped: 0,
            last_auto_merge_conflicted_skipped: 0,
            last_auto_merge_dirty_skipped: 0,
            last_auto_merge_failed: 0,
            last_auto_prune_at: None,
            last_auto_prune_pruned: 0,
            last_auto_prune_active_skipped: 0,
        };

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Attention queue clear"));
        assert!(!text.contains("Needs attention:"));
        assert!(!text.contains("Backlog focus-12"));
    }

    #[test]
    fn summary_line_includes_worktree_health_counts() {
        let sessions = vec![
            sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            ),
            sample_session(
                "worker-1234567",
                "claude",
                SessionState::Idle,
                Some("ecc/worker"),
                256,
                21,
            ),
        ];
        let unread = HashMap::new();
        let worktree_health = HashMap::from([
            (
                String::from("focus-12345678"),
                worktree::WorktreeHealth::Conflicted,
            ),
            (
                String::from("worker-1234567"),
                worktree::WorktreeHealth::InProgress,
            ),
        ]);

        let summary = SessionSummary::from_sessions(&sessions, &unread, &worktree_health, false);
        let rendered = summary_line(&summary)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains("Conflicts 1"));
        assert!(rendered.contains("Worktrees 1"));
    }

    #[test]
    fn attention_queue_keeps_conflicted_worktree_pressure_when_stabilized() {
        let now = Utc::now();
        let sessions = vec![sample_session(
            "focus-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/focus"),
            512,
            42,
        )];
        let unread = HashMap::from([(String::from("focus-12345678"), 3usize)]);
        let worktree_health = HashMap::from([(
            String::from("focus-12345678"),
            worktree::WorktreeHealth::Conflicted,
        )]);

        let summary = SessionSummary::from_sessions(&sessions, &unread, &worktree_health, true);
        let rendered = attention_queue_line(&summary, true)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains("Attention queue"));
        assert!(rendered.contains("Conflicts 1"));
        assert!(!rendered.contains("Attention queue clear"));

        let mut dashboard = test_dashboard(sessions, 0);
        dashboard.unread_message_counts = unread;
        dashboard.handoff_backlog_counts =
            HashMap::from([(String::from("focus-12345678"), 3usize)]);
        dashboard.worktree_health_by_session = worktree_health;
        dashboard.daemon_activity = DaemonActivity {
            last_dispatch_at: Some(now + chrono::Duration::seconds(2)),
            last_dispatch_routed: 2,
            last_dispatch_deferred: 0,
            last_dispatch_leads: 1,
            chronic_saturation_streak: 0,
            last_recovery_dispatch_at: Some(now + chrono::Duration::seconds(1)),
            last_recovery_dispatch_routed: 1,
            last_recovery_dispatch_leads: 1,
            last_rebalance_at: Some(now),
            last_rebalance_rerouted: 1,
            last_rebalance_leads: 1,
            last_auto_merge_at: None,
            last_auto_merge_merged: 0,
            last_auto_merge_active_skipped: 0,
            last_auto_merge_conflicted_skipped: 0,
            last_auto_merge_dirty_skipped: 0,
            last_auto_merge_failed: 0,
            last_auto_prune_at: None,
            last_auto_prune_pruned: 0,
            last_auto_prune_active_skipped: 0,
        };

        let text = dashboard.selected_session_metrics_text();
        assert!(text.contains("Needs attention:"));
        assert!(text.contains("Conflicted worktree focus-12"));
        assert!(!text.contains("Backlog focus-12"));
    }

    #[test]
    fn route_preview_uses_graph_context_for_latest_incoming_handoff() {
        let lead = sample_session(
            "lead-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/lead"),
            512,
            42,
        );
        let older_worker = sample_session(
            "older-worker",
            "planner",
            SessionState::Idle,
            Some("ecc/older"),
            128,
            12,
        );
        let auth_worker = sample_session(
            "auth-worker",
            "planner",
            SessionState::Idle,
            Some("ecc/auth"),
            256,
            24,
        );

        let mut dashboard = test_dashboard(
            vec![lead.clone(), older_worker.clone(), auth_worker.clone()],
            0,
        );
        dashboard.db.insert_session(&lead).unwrap();
        dashboard.db.insert_session(&older_worker).unwrap();
        dashboard.db.insert_session(&auth_worker).unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "older-worker",
                "{\"task\":\"Legacy delegated work\",\"context\":\"Delegated from lead\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "auth-worker",
                "{\"task\":\"Auth delegated work\",\"context\":\"Delegated from lead\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard.db.mark_messages_read("older-worker").unwrap();
        dashboard.db.mark_messages_read("auth-worker").unwrap();
        dashboard
            .db
            .send_message(
                "planner-root",
                "lead-12345678",
                "{\"task\":\"Investigate auth callback recovery\",\"context\":\"Delegated from planner-root\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard
            .db
            .upsert_context_entity(
                Some("auth-worker"),
                "file",
                "auth-callback.ts",
                Some("src/auth/callback.ts"),
                "Auth callback recovery edge cases",
                &BTreeMap::new(),
            )
            .unwrap();

        dashboard.unread_message_counts = dashboard.db.unread_message_counts().unwrap();
        dashboard.sync_selected_messages();
        dashboard.sync_selected_lineage();

        assert_eq!(
            dashboard.selected_route_preview.as_deref(),
            Some("for `Investigate auth callback recovery` reuse idle auth-wor | graph auth, callback, recovery")
        );
    }

    #[test]
    fn route_preview_ignores_non_handoff_inbox_noise() {
        let lead = sample_session(
            "lead-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/lead"),
            512,
            42,
        );
        let idle_worker = sample_session(
            "idle-worker",
            "planner",
            SessionState::Idle,
            Some("ecc/idle"),
            128,
            12,
        );

        let mut dashboard = test_dashboard(vec![lead.clone(), idle_worker.clone()], 0);
        dashboard.db.insert_session(&lead).unwrap();
        dashboard.db.insert_session(&idle_worker).unwrap();
        dashboard
            .db
            .send_message("lead-12345678", "idle-worker", "FYI status update", "info")
            .unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "idle-worker",
                "{\"task\":\"Delegated work\",\"context\":\"Delegated from lead\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard.db.mark_messages_read("idle-worker").unwrap();
        dashboard
            .db
            .send_message("lead-12345678", "idle-worker", "FYI status update", "info")
            .unwrap();

        dashboard.unread_message_counts = dashboard.db.unread_message_counts().unwrap();
        dashboard.sync_selected_lineage();

        assert_eq!(
            dashboard.selected_route_preview.as_deref(),
            Some("reuse idle idle-wor")
        );
        assert_eq!(dashboard.selected_child_sessions.len(), 1);
        assert_eq!(dashboard.selected_child_sessions[0].handoff_backlog, 0);
    }

    #[test]
    fn sync_selected_lineage_populates_delegate_task_and_output_previews() {
        let lead = sample_session(
            "lead-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/lead"),
            512,
            42,
        );
        let mut child = sample_session(
            "worker-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/worker"),
            128,
            12,
        );
        child.task = "Implement delegate metrics board for ECC 2.0".to_string();

        let mut dashboard = test_dashboard(vec![lead.clone(), child.clone()], 0);
        dashboard.db.insert_session(&lead).unwrap();
        dashboard.db.insert_session(&child).unwrap();
        dashboard
            .db
            .update_metrics("worker-12345678", &child.metrics)
            .unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-12345678",
                "{\"task\":\"Delegated work\",\"context\":\"Delegated from lead\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard
            .db
            .append_output_line(
                "worker-12345678",
                OutputStream::Stdout,
                "Reviewing delegate metrics board layout",
            )
            .unwrap();
        dashboard
            .approval_queue_counts
            .insert("worker-12345678".into(), 2);
        dashboard.worktree_health_by_session.insert(
            "worker-12345678".into(),
            worktree::WorktreeHealth::InProgress,
        );

        dashboard.sync_selected_lineage();

        assert_eq!(dashboard.selected_child_sessions.len(), 1);
        assert_eq!(
            dashboard.selected_child_sessions[0].worktree_health,
            Some(worktree::WorktreeHealth::InProgress)
        );
        assert_eq!(dashboard.selected_child_sessions[0].approval_backlog, 2);
        assert_eq!(dashboard.selected_child_sessions[0].tokens_used, 128);
        assert_eq!(dashboard.selected_child_sessions[0].files_changed, 2);
        assert_eq!(dashboard.selected_child_sessions[0].duration_secs, 12);
        assert_eq!(
            dashboard.selected_child_sessions[0].task_preview,
            "Implement delegate metrics board for EC…"
        );
        assert_eq!(
            dashboard.selected_child_sessions[0].branch.as_deref(),
            Some("ecc/worker")
        );
        assert_eq!(
            dashboard.selected_child_sessions[0]
                .last_output_preview
                .as_deref(),
            Some("Reviewing delegate metrics board layout")
        );
    }

    #[test]
    fn sync_selected_lineage_prioritizes_conflicted_delegate_rows() {
        let lead = sample_session(
            "lead-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/lead"),
            512,
            42,
        );
        let conflicted = sample_session(
            "worker-conflict",
            "planner",
            SessionState::Running,
            Some("ecc/conflict"),
            128,
            12,
        );
        let idle = sample_session(
            "worker-idle",
            "planner",
            SessionState::Idle,
            Some("ecc/idle"),
            64,
            6,
        );

        let mut dashboard = test_dashboard(vec![lead.clone(), conflicted.clone(), idle.clone()], 0);
        dashboard.db.insert_session(&lead).unwrap();
        dashboard.db.insert_session(&conflicted).unwrap();
        dashboard.db.insert_session(&idle).unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-conflict",
                "{\"task\":\"Handle conflict\",\"context\":\"Delegated from lead\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-idle",
                "{\"task\":\"Idle follow-up\",\"context\":\"Delegated from lead\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard.worktree_health_by_session.insert(
            "worker-conflict".into(),
            worktree::WorktreeHealth::Conflicted,
        );

        dashboard.sync_selected_lineage();

        assert_eq!(dashboard.selected_child_sessions.len(), 2);
        assert_eq!(
            dashboard.selected_child_sessions[0].session_id,
            "worker-conflict"
        );
        assert_eq!(
            dashboard.selected_child_sessions[0].worktree_health,
            Some(worktree::WorktreeHealth::Conflicted)
        );
    }

    #[test]
    fn sync_selected_lineage_preserves_focused_delegate_by_session_id() {
        let lead = sample_session(
            "lead-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/lead"),
            512,
            42,
        );
        let conflicted = sample_session(
            "worker-conflict",
            "planner",
            SessionState::Running,
            Some("ecc/conflict"),
            128,
            12,
        );
        let idle = sample_session(
            "worker-idle",
            "planner",
            SessionState::Idle,
            Some("ecc/idle"),
            64,
            6,
        );

        let mut dashboard = test_dashboard(vec![lead.clone(), conflicted.clone(), idle.clone()], 0);
        dashboard.db.insert_session(&lead).unwrap();
        dashboard.db.insert_session(&conflicted).unwrap();
        dashboard.db.insert_session(&idle).unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-conflict",
                "{\"task\":\"Handle conflict\",\"context\":\"Delegated from lead\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard
            .db
            .send_message(
                "lead-12345678",
                "worker-idle",
                "{\"task\":\"Idle follow-up\",\"context\":\"Delegated from lead\"}",
                "task_handoff",
            )
            .unwrap();
        dashboard.sync_selected_lineage();
        dashboard.focused_delegate_session_id = Some("worker-idle".to_string());
        dashboard.worktree_health_by_session.insert(
            "worker-conflict".into(),
            worktree::WorktreeHealth::Conflicted,
        );

        dashboard.sync_selected_lineage();

        assert_eq!(
            dashboard.focused_delegate_session_id.as_deref(),
            Some("worker-idle")
        );
    }

    #[test]
    fn sync_selected_lineage_keeps_all_delegate_rows() {
        let lead = sample_session(
            "lead-12345678",
            "planner",
            SessionState::Running,
            Some("ecc/lead"),
            512,
            42,
        );

        let mut sessions = vec![lead.clone()];
        let mut dashboard = test_dashboard(vec![lead.clone()], 0);
        dashboard.db.insert_session(&lead).unwrap();

        for index in 0..5 {
            let child_id = format!("worker-{index}");
            let child = sample_session(
                &child_id,
                "planner",
                SessionState::Running,
                Some(&format!("ecc/{child_id}")),
                64,
                6,
            );
            sessions.push(child.clone());
            dashboard.db.insert_session(&child).unwrap();
            dashboard
                .db
                .send_message(
                    "lead-12345678",
                    &child_id,
                    "{\"task\":\"Delegated work\",\"context\":\"Delegated from lead\"}",
                    "task_handoff",
                )
                .unwrap();
        }

        dashboard.sessions = sessions;
        dashboard.sync_selected_lineage();

        assert_eq!(dashboard.selected_child_sessions.len(), 5);
    }

    #[test]
    fn aggregate_cost_summary_mentions_total_cost() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let mut cfg = Config::default();
        cfg.cost_budget_usd = 10.0;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.sessions = vec![budget_session("sess-1", 3_500, 8.25)];

        assert_eq!(
            dashboard.aggregate_cost_summary_text(),
            "Aggregate cost $8.25 / $10.00 | Budget alert 75%"
        );
    }

    #[test]
    fn aggregate_cost_summary_mentions_fifty_percent_alert() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let mut cfg = Config::default();
        cfg.cost_budget_usd = 10.0;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.sessions = vec![budget_session("sess-1", 1_000, 5.0)];

        assert_eq!(
            dashboard.aggregate_cost_summary_text(),
            "Aggregate cost $5.00 / $10.00 | Budget alert 50%"
        );
    }

    #[test]
    fn aggregate_cost_summary_uses_custom_threshold_labels() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let mut cfg = Config::default();
        cfg.cost_budget_usd = 10.0;
        cfg.budget_alert_thresholds = crate::config::BudgetAlertThresholds {
            advisory: 0.40,
            warning: 0.70,
            critical: 0.85,
        };

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.sessions = vec![budget_session("sess-1", 1_000, 7.0)];

        assert_eq!(
            dashboard.aggregate_cost_summary_text(),
            "Aggregate cost $7.00 / $10.00 | Budget alert 70%"
        );
    }

    #[test]
    fn aggregate_cost_summary_mentions_ninety_percent_alert() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let mut cfg = Config::default();
        cfg.cost_budget_usd = 10.0;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.sessions = vec![budget_session("sess-1", 1_000, 9.0)];

        assert_eq!(
            dashboard.aggregate_cost_summary_text(),
            "Aggregate cost $9.00 / $10.00 | Budget alert 90%"
        );
    }

    #[test]
    fn sync_budget_alerts_sets_operator_note_when_threshold_is_crossed() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let mut cfg = Config::default();
        cfg.token_budget = 1_000;
        cfg.cost_budget_usd = 10.0;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.sessions = vec![budget_session("sess-1", 760, 2.0)];
        dashboard.last_budget_alert_state = BudgetState::Alert50;

        dashboard.sync_budget_alerts();

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("Budget alert 75% | tokens 760 / 1,000 | cost $2.00 / $10.00")
        );
        assert_eq!(dashboard.last_budget_alert_state, BudgetState::Alert75);
    }

    #[test]
    fn sync_budget_alerts_uses_custom_threshold_labels() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let mut cfg = Config::default();
        cfg.token_budget = 1_000;
        cfg.cost_budget_usd = 10.0;
        cfg.budget_alert_thresholds = crate::config::BudgetAlertThresholds {
            advisory: 0.40,
            warning: 0.70,
            critical: 0.85,
        };

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.sessions = vec![budget_session("sess-1", 710, 2.0)];
        dashboard.last_budget_alert_state = BudgetState::Alert50;

        dashboard.sync_budget_alerts();

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("Budget alert 70% | tokens 710 / 1,000 | cost $2.00 / $10.00")
        );
        assert_eq!(dashboard.last_budget_alert_state, BudgetState::Alert75);
    }

    #[test]
    fn refresh_auto_pauses_over_budget_sessions_and_sets_operator_note() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let mut cfg = Config::default();
        cfg.token_budget = 100;
        cfg.cost_budget_usd = 0.0;

        db.insert_session(&budget_session("sess-1", 120, 0.0))
            .expect("insert session");
        db.update_metrics(
            "sess-1",
            &SessionMetrics {
                input_tokens: 90,
                output_tokens: 30,
                tokens_used: 120,
                tool_calls: 0,
                files_changed: 0,
                duration_secs: 0,
                cost_usd: 0.0,
            },
        )
        .expect("persist metrics");

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.refresh();

        assert_eq!(dashboard.sessions.len(), 1);
        assert_eq!(dashboard.sessions[0].state, SessionState::Stopped);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("token budget exceeded | auto-paused 1 active session(s)")
        );
    }

    #[test]
    fn refresh_updates_session_state_snapshot_after_completion() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let now = Utc::now();
        let session = Session {
            id: "done-1".to_string(),
            task: "complete session".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        };
        db.insert_session(&session).unwrap();

        let mut dashboard = Dashboard::new(db, Config::default());
        dashboard
            .db
            .update_state("done-1", &SessionState::Completed)
            .unwrap();

        dashboard.refresh();

        assert_eq!(dashboard.sessions[0].state, SessionState::Completed);
        assert_eq!(
            dashboard.last_session_states.get("done-1"),
            Some(&SessionState::Completed)
        );
    }

    #[test]
    fn refresh_builds_completion_summary_popup_from_metrics_activity_and_logs() -> Result<()> {
        let root = std::env::temp_dir().join(format!("ecc2-completion-popup-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join(".claude").join("metrics"))?;

        let mut cfg = build_config(&root.join(".claude"));
        cfg.completion_summary_notifications.delivery =
            crate::notifications::CompletionSummaryDelivery::TuiPopup;
        cfg.desktop_notifications.session_completed = false;

        let db = StateStore::open(&cfg.db_path)?;
        let mut session = sample_session(
            "done-12345678",
            "claude",
            SessionState::Running,
            Some("ecc/done"),
            384,
            95,
        );
        session.task = "Finish session summary notifications".to_string();
        db.insert_session(&session)?;

        let metrics_path = cfg.tool_activity_metrics_path();
        fs::create_dir_all(metrics_path.parent().unwrap())?;
        fs::write(
            &metrics_path,
            concat!(
                "{\"id\":\"evt-1\",\"session_id\":\"done-12345678\",\"tool_name\":\"Bash\",\"input_summary\":\"cargo test -q\",\"input_params_json\":\"{\\\"command\\\":\\\"cargo test -q\\\"}\",\"output_summary\":\"ok\",\"timestamp\":\"2026-04-09T00:00:00Z\"}\n",
                "{\"id\":\"evt-2\",\"session_id\":\"done-12345678\",\"tool_name\":\"Write\",\"input_summary\":\"Write README.md\",\"output_summary\":\"updated readme\",\"file_events\":[{\"path\":\"README.md\",\"action\":\"create\",\"diff_preview\":\"+ session summary notifications\",\"patch_preview\":\"+ session summary notifications\"}],\"timestamp\":\"2026-04-09T00:01:00Z\"}\n",
                "{\"id\":\"evt-3\",\"session_id\":\"done-12345678\",\"tool_name\":\"Bash\",\"input_summary\":\"rm -rf build\",\"input_params_json\":\"{\\\"command\\\":\\\"rm -rf build\\\"}\",\"output_summary\":\"ok\",\"timestamp\":\"2026-04-09T00:02:00Z\"}\n"
            ),
        )?;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard
            .db
            .update_state("done-12345678", &SessionState::Completed)?;

        dashboard.refresh();

        let popup = dashboard
            .active_completion_popup
            .as_ref()
            .expect("completion summary popup");
        let popup_text = popup.popup_text();
        assert!(popup_text.contains("done-123"));
        assert!(popup_text.contains("Tests 1 run / 1 passed"));
        assert!(popup_text.contains("Recent files"));
        assert!(popup_text.contains("create README.md"));
        assert!(popup_text.contains("Warnings"));
        assert!(popup_text.contains("high-risk tool call"));

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn refresh_persists_completion_summary_observation() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("ecc2-completion-observation-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join(".claude").join("metrics"))?;

        let mut cfg = build_config(&root.join(".claude"));
        cfg.completion_summary_notifications.delivery =
            crate::notifications::CompletionSummaryDelivery::TuiPopup;
        cfg.desktop_notifications.session_completed = false;

        let db = StateStore::open(&cfg.db_path)?;
        let mut session = sample_session(
            "done-observation",
            "claude",
            SessionState::Running,
            Some("ecc/observation"),
            144,
            42,
        );
        session.task = "Recover auth callback after wipe".to_string();
        db.insert_session(&session)?;

        let metrics_path = cfg.tool_activity_metrics_path();
        fs::create_dir_all(metrics_path.parent().unwrap())?;
        fs::write(
            &metrics_path,
            concat!(
                "{\"id\":\"evt-1\",\"session_id\":\"done-observation\",\"tool_name\":\"Bash\",\"input_summary\":\"cargo test -q\",\"input_params_json\":\"{\\\"command\\\":\\\"cargo test -q\\\"}\",\"output_summary\":\"ok\",\"timestamp\":\"2026-04-09T00:00:00Z\"}\n",
                "{\"id\":\"evt-2\",\"session_id\":\"done-observation\",\"tool_name\":\"Write\",\"input_summary\":\"Write src/routes/auth/callback.ts\",\"output_summary\":\"updated callback\",\"file_events\":[{\"path\":\"src/routes/auth/callback.ts\",\"action\":\"modify\",\"diff_preview\":\"portal first\",\"patch_preview\":\"+ portal first\"}],\"timestamp\":\"2026-04-09T00:01:00Z\"}\n"
            ),
        )?;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard
            .db
            .update_state("done-observation", &SessionState::Completed)?;

        dashboard.refresh();

        let session_entity = dashboard
            .db
            .list_context_entities(Some("done-observation"), Some("session"), 10)?
            .into_iter()
            .find(|entity| entity.name == "done-observation")
            .expect("session entity");
        let observations = dashboard
            .db
            .list_context_observations(Some(session_entity.id), 10)?;
        assert!(!observations.is_empty());
        assert_eq!(observations[0].observation_type, "completion_summary");
        assert!(observations[0]
            .summary
            .contains("Recover auth callback after wipe"));
        assert_eq!(
            observations[0].details.get("tests_run"),
            Some(&"1".to_string())
        );
        assert!(observations[0]
            .details
            .get("recent_files")
            .is_some_and(|value| value.contains("modify src/routes/auth/callback.ts")));

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn dismiss_completion_popup_promotes_the_next_summary() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.active_completion_popup = Some(SessionCompletionSummary {
            session_id: "sess-a".to_string(),
            task: "First".to_string(),
            state: SessionState::Completed,
            files_changed: 1,
            tokens_used: 10,
            duration_secs: 5,
            cost_usd: 0.01,
            tests_run: 1,
            tests_passed: 1,
            recent_files: vec!["create README.md".to_string()],
            key_decisions: vec!["cargo test -q".to_string()],
            warnings: Vec::new(),
        });
        dashboard
            .queued_completion_popups
            .push_back(SessionCompletionSummary {
                session_id: "sess-b".to_string(),
                task: "Second".to_string(),
                state: SessionState::Completed,
                files_changed: 2,
                tokens_used: 20,
                duration_secs: 8,
                cost_usd: 0.02,
                tests_run: 0,
                tests_passed: 0,
                recent_files: vec!["modify src/lib.rs".to_string()],
                key_decisions: vec!["updated lib".to_string()],
                warnings: vec!["no test runs detected".to_string()],
            });

        dashboard.dismiss_completion_popup();

        assert_eq!(
            dashboard
                .active_completion_popup
                .as_ref()
                .map(|summary| summary.session_id.as_str()),
            Some("sess-b")
        );
        assert!(dashboard.queued_completion_popups.is_empty());

        dashboard.dismiss_completion_popup();
        assert!(dashboard.active_completion_popup.is_none());
    }

    #[test]
    fn refresh_syncs_tool_activity_metrics_from_hook_file() {
        let tempdir = std::env::temp_dir().join(format!("ecc2-activity-sync-{}", Uuid::new_v4()));
        fs::create_dir_all(tempdir.join("metrics")).unwrap();
        let db_path = tempdir.join("state.db");
        let db = StateStore::open(&db_path).unwrap();
        let now = Utc::now();

        db.insert_session(&Session {
            id: "sess-1".to_string(),
            task: "sync activity".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })
        .unwrap();

        let mut cfg = Config::default();
        cfg.db_path = db_path;

        let mut dashboard = Dashboard::new(db, cfg);
        fs::write(
            tempdir.join("metrics").join("tool-usage.jsonl"),
            "{\"id\":\"evt-1\",\"session_id\":\"sess-1\",\"tool_name\":\"Read\",\"input_summary\":\"Read README.md\",\"output_summary\":\"ok\",\"file_paths\":[\"README.md\"],\"timestamp\":\"2026-04-09T00:00:00Z\"}\n",
        )
        .unwrap();

        dashboard.refresh();

        assert_eq!(dashboard.sessions.len(), 1);
        assert_eq!(dashboard.sessions[0].metrics.tool_calls, 1);
        assert_eq!(dashboard.sessions[0].metrics.files_changed, 1);

        let _ = fs::remove_dir_all(tempdir);
    }

    #[test]
    fn refresh_flags_stale_sessions_and_sets_operator_note() {
        let db = StateStore::open(Path::new(":memory:")).unwrap();
        let mut cfg = Config::default();
        cfg.session_timeout_secs = 60;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "stale-1".to_string(),
            task: "stale session".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: Some(4242),
            worktree: None,
            created_at: now - Duration::minutes(5),
            updated_at: now - Duration::minutes(5),
            last_heartbeat_at: now - Duration::minutes(5),
            metrics: SessionMetrics::default(),
        })
        .unwrap();

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.refresh();

        assert_eq!(dashboard.sessions.len(), 1);
        assert_eq!(dashboard.sessions[0].state, SessionState::Stale);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("stale heartbeat detected | flagged 1 session(s) for attention")
        );
    }

    #[test]
    fn refresh_enforces_conflicts_and_surfaces_active_incidents() -> Result<()> {
        let tempdir =
            std::env::temp_dir().join(format!("dashboard-conflict-refresh-{}", Uuid::new_v4()));
        fs::create_dir_all(&tempdir)?;
        let mut cfg = build_config(&tempdir);
        cfg.session_timeout_secs = 3600;
        let db = StateStore::open(&cfg.db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "session-a".to_string(),
            task: "keep active".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now - Duration::minutes(2),
            updated_at: now - Duration::minutes(2),
            last_heartbeat_at: now - Duration::minutes(2),
            metrics: SessionMetrics::default(),
        })?;
        db.insert_session(&Session {
            id: "session-b".to_string(),
            task: "later overlap".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now - Duration::minutes(1),
            updated_at: now - Duration::minutes(1),
            last_heartbeat_at: now - Duration::minutes(1),
            metrics: SessionMetrics::default(),
        })?;

        fs::create_dir_all(
            cfg.tool_activity_metrics_path()
                .parent()
                .expect("metrics dir"),
        )?;
        fs::write(
            cfg.tool_activity_metrics_path(),
            concat!(
                "{\"id\":\"evt-1\",\"session_id\":\"session-a\",\"tool_name\":\"Edit\",\"input_summary\":\"Edit src/lib.rs\",\"output_summary\":\"older change\",\"file_events\":[{\"path\":\"src/lib.rs\",\"action\":\"modify\"}],\"timestamp\":\"2026-04-09T00:02:00Z\"}\n",
                "{\"id\":\"evt-2\",\"session_id\":\"session-b\",\"tool_name\":\"Write\",\"input_summary\":\"Write src/lib.rs\",\"output_summary\":\"later change\",\"file_events\":[{\"path\":\"src/lib.rs\",\"action\":\"modify\"}],\"timestamp\":\"2026-04-09T00:03:00Z\"}\n"
            ),
        )?;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.refresh();
        dashboard.sync_selection_by_id(Some("session-b"));
        dashboard.sync_selected_diff();

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("file conflict detected | opened 1 incident(s), auto-paused 1 session(s) via escalation")
        );
        assert_eq!(
            dashboard
                .db
                .get_session("session-b")?
                .expect("session-b should exist")
                .state,
            SessionState::Stopped
        );

        let metrics_text = dashboard.selected_session_metrics_text();
        assert!(metrics_text.contains("Active conflicts"));
        assert!(metrics_text.contains("src/lib.rs"));
        assert!(metrics_text.contains("escalate"));

        let conflict_protocol = dashboard
            .selected_conflict_protocol
            .clone()
            .expect("conflict protocol should be present");
        assert!(conflict_protocol.contains("Session overlap incidents"));
        assert!(conflict_protocol.contains("ecc resume session-b"));

        dashboard.refresh();
        assert_eq!(
            dashboard
                .db
                .list_open_conflict_incidents_for_session("session-b", 10)?
                .len(),
            1
        );

        let _ = fs::remove_dir_all(tempdir);
        Ok(())
    }

    #[test]
    fn selected_session_metrics_text_includes_harness_summary() -> Result<()> {
        let tempdir = std::env::temp_dir().join(format!(
            "ecc2-dashboard-harness-metrics-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(tempdir.join(".claude"))?;
        fs::create_dir_all(tempdir.join(".codex"))?;

        let now = Utc::now();
        let session = Session {
            id: "sess-harness".to_string(),
            task: "Map harness metadata".to_string(),
            project: "ecc".to_string(),
            task_group: "compat".to_string(),
            agent_type: "claude".to_string(),
            working_dir: tempdir.clone(),
            state: SessionState::Running,
            pid: Some(4242),
            worktree: None,
            created_at: now - Duration::minutes(3),
            updated_at: now - Duration::minutes(1),
            last_heartbeat_at: now - Duration::minutes(1),
            metrics: SessionMetrics::default(),
        };

        let dashboard = test_dashboard(vec![session], 0);
        let metrics_text = dashboard.selected_session_metrics_text();
        assert!(metrics_text.contains("Harness claude | Detected claude, codex"));

        let _ = fs::remove_dir_all(tempdir);
        Ok(())
    }

    #[test]
    fn new_session_task_uses_selected_session_context() {
        let dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );

        assert_eq!(
            dashboard.new_session_task(),
            "Follow up on focus-12: Render dashboard rows"
        );
    }

    #[test]
    fn active_session_count_only_counts_live_queue_states() {
        let dashboard = test_dashboard(
            vec![
                sample_session("pending-1", "planner", SessionState::Pending, None, 1, 1),
                sample_session("running-1", "planner", SessionState::Running, None, 1, 1),
                sample_session("idle-1", "planner", SessionState::Idle, None, 1, 1),
                sample_session("failed-1", "planner", SessionState::Failed, None, 1, 1),
                sample_session("stopped-1", "planner", SessionState::Stopped, None, 1, 1),
                sample_session("done-1", "planner", SessionState::Completed, None, 1, 1),
            ],
            0,
        );

        assert_eq!(dashboard.active_session_count(), 3);
    }

    #[test]
    fn spawn_prompt_seed_uses_selected_session_context() {
        let dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                Some("ecc/focus"),
                512,
                42,
            )],
            0,
        );

        assert_eq!(
            dashboard.spawn_prompt_seed(),
            "give me 2 agents working on Follow up on focus-12: Render dashboard rows"
        );
    }

    #[test]
    fn parse_spawn_request_extracts_count_and_task_from_natural_language() {
        let request = parse_spawn_request("give me 10 agents working on stabilize the queue")
            .expect("spawn request should parse");

        assert_eq!(
            request,
            SpawnRequest::AdHoc {
                requested_count: 10,
                task: "stabilize the queue".to_string(),
            }
        );
    }

    #[test]
    fn parse_spawn_request_defaults_to_single_session_without_count() {
        let request = parse_spawn_request("stabilize the queue").expect("spawn request");

        assert_eq!(
            request,
            SpawnRequest::AdHoc {
                requested_count: 1,
                task: "stabilize the queue".to_string(),
            }
        );
    }

    #[test]
    fn parse_spawn_request_extracts_template_request() {
        let request = parse_spawn_request(
            "template feature_development for stabilize auth callback with component=billing, area=oauth",
        )
        .expect("template request should parse");

        assert_eq!(
            request,
            SpawnRequest::Template {
                name: "feature_development".to_string(),
                task: Some("stabilize auth callback".to_string()),
                variables: BTreeMap::from([
                    ("area".to_string(), "oauth".to_string()),
                    ("component".to_string(), "billing".to_string()),
                ]),
            }
        );
    }

    #[test]
    fn build_spawn_plan_caps_requested_count_to_available_slots() {
        let dashboard = test_dashboard(
            vec![
                sample_session("pending-1", "planner", SessionState::Pending, None, 1, 1),
                sample_session("running-1", "planner", SessionState::Running, None, 1, 1),
                sample_session("idle-1", "planner", SessionState::Idle, None, 1, 1),
            ],
            0,
        );

        let plan = dashboard
            .build_spawn_plan("give me 9 agents working on ship release notes")
            .expect("spawn plan");

        assert_eq!(
            plan,
            SpawnPlan::AdHoc {
                requested_count: 9,
                spawn_count: 5,
                task: "ship release notes".to_string(),
            }
        );
    }

    #[test]
    fn build_spawn_plan_resolves_template_steps() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.orchestration_templates = BTreeMap::from([(
            "feature_development".to_string(),
            crate::config::OrchestrationTemplateConfig {
                description: None,
                project: None,
                task_group: None,
                agent: Some("claude".to_string()),
                profile: None,
                worktree: Some(true),
                steps: vec![
                    crate::config::OrchestrationTemplateStepConfig {
                        name: Some("planner".to_string()),
                        task: "Plan {{task}}".to_string(),
                        project: None,
                        task_group: None,
                        agent: None,
                        profile: None,
                        worktree: None,
                    },
                    crate::config::OrchestrationTemplateStepConfig {
                        name: Some("builder".to_string()),
                        task: "Build {{task}} in {{component}}".to_string(),
                        project: None,
                        task_group: None,
                        agent: None,
                        profile: None,
                        worktree: None,
                    },
                ],
            },
        )]);

        let plan = dashboard
            .build_spawn_plan(
                "template feature_development for stabilize auth callback with component=billing",
            )
            .expect("template spawn plan");

        assert_eq!(
            plan,
            SpawnPlan::Template {
                name: "feature_development".to_string(),
                task: Some("stabilize auth callback".to_string()),
                variables: BTreeMap::from([("component".to_string(), "billing".to_string(),)]),
                step_count: 2,
            }
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn submit_spawn_prompt_launches_orchestration_template() -> Result<()> {
        let tempdir = std::env::temp_dir().join(format!("dashboard-template-{}", Uuid::new_v4()));
        let repo_root = tempdir.join("repo");
        init_git_repo(&repo_root)?;

        let cwd_guard = crate::test_support::CurrentDirGuard::enter(&repo_root)?;

        let mut cfg = build_config(&tempdir);
        cfg.orchestration_templates = BTreeMap::from([(
            "feature_development".to_string(),
            crate::config::OrchestrationTemplateConfig {
                description: None,
                project: Some("ecc2-smoke".to_string()),
                task_group: Some("{{task}}".to_string()),
                agent: Some("claude".to_string()),
                profile: None,
                worktree: Some(false),
                steps: vec![
                    crate::config::OrchestrationTemplateStepConfig {
                        name: Some("planner".to_string()),
                        task: "Plan {{task}}".to_string(),
                        project: None,
                        task_group: None,
                        agent: None,
                        profile: None,
                        worktree: None,
                    },
                    crate::config::OrchestrationTemplateStepConfig {
                        name: Some("builder".to_string()),
                        task: "Build {{task}} in {{component}}".to_string(),
                        project: None,
                        task_group: None,
                        agent: None,
                        profile: None,
                        worktree: None,
                    },
                ],
            },
        )]);

        let db = StateStore::open(&cfg.db_path)?;
        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.spawn_input = Some(
            "template feature_development for stabilize auth callback with component=billing"
                .to_string(),
        );

        dashboard.submit_spawn_prompt().await;

        let operator_note = dashboard
            .operator_note
            .clone()
            .expect("template launch should set an operator note");
        assert!(
            operator_note.contains(
                "launched template feature_development (2/2 step(s)) for stabilize auth callback"
            ),
            "unexpected operator note: {operator_note}"
        );
        assert_eq!(dashboard.sessions.len(), 2);
        assert!(dashboard
            .sessions
            .iter()
            .all(|session| session.project == "ecc2-smoke"));
        assert!(dashboard
            .sessions
            .iter()
            .all(|session| session.task_group == "stabilize auth callback"));
        let tasks = dashboard
            .sessions
            .iter()
            .map(|session| session.task.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            tasks,
            std::collections::BTreeSet::from([
                "Build stabilize auth callback in billing",
                "Plan stabilize auth callback",
            ])
        );

        drop(cwd_guard);
        let _ = std::fs::remove_dir_all(&tempdir);
        Ok(())
    }

    #[test]
    fn expand_spawn_tasks_suffixes_multi_session_requests() {
        assert_eq!(
            expand_spawn_tasks("stabilize the queue", 3),
            vec![
                "stabilize the queue [1/3]".to_string(),
                "stabilize the queue [2/3]".to_string(),
                "stabilize the queue [3/3]".to_string(),
            ]
        );
    }

    #[test]
    fn refresh_preserves_selected_session_by_id() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "older".to_string(),
            task: "older".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Idle,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        db.insert_session(&Session {
            id: "newer".to_string(),
            task: "newer".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now + chrono::Duration::seconds(1),
            last_heartbeat_at: now + chrono::Duration::seconds(1),
            metrics: SessionMetrics::default(),
        })?;

        let mut dashboard = Dashboard::new(db, Config::default());
        dashboard.selected_session = 1;
        dashboard.sync_selection();
        dashboard.refresh();

        assert_eq!(dashboard.selected_session_id(), Some("older"));
        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn metrics_scroll_uses_independent_offset() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "session-1".to_string(),
            task: "inspect output".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        for index in 0..6 {
            db.append_output_line("session-1", OutputStream::Stdout, &format!("line {index}"))?;
        }

        let mut dashboard = Dashboard::new(db, Config::default());
        dashboard.selected_pane = Pane::Output;
        dashboard.refresh();
        dashboard.sync_output_scroll(3);
        dashboard.scroll_up();
        let previous_scroll = dashboard.output_scroll_offset;

        dashboard.selected_pane = Pane::Metrics;
        dashboard.last_metrics_height = 2;
        dashboard.scroll_up();
        dashboard.scroll_down();
        dashboard.scroll_down();

        assert_eq!(dashboard.output_scroll_offset, previous_scroll);
        assert_eq!(dashboard.metrics_scroll_offset, 2);
        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn refresh_loads_selected_session_output_and_follows_tail() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "session-1".to_string(),
            task: "tail output".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        for index in 0..12 {
            db.append_output_line("session-1", OutputStream::Stdout, &format!("line {index}"))?;
        }

        let mut dashboard = Dashboard::new(db, Config::default());
        dashboard.selected_pane = Pane::Output;
        dashboard.refresh();
        dashboard.sync_output_scroll(4);

        assert_eq!(dashboard.output_scroll_offset, 8);
        assert!(dashboard.selected_output_text().contains("line 11"));

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn submit_search_tracks_matches_and_sets_navigation_note() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line(OutputStream::Stdout, "alpha"),
                test_output_line(OutputStream::Stdout, "beta"),
                test_output_line(OutputStream::Stdout, "alpha tail"),
            ],
        );
        dashboard.last_output_height = 2;

        dashboard.begin_search();
        for ch in "alpha.*".chars() {
            dashboard.push_input_char(ch);
        }
        dashboard.submit_search();

        assert_eq!(dashboard.search_query.as_deref(), Some("alpha.*"));
        assert_eq!(
            dashboard.search_matches,
            vec![
                SearchMatch {
                    session_id: "focus-12345678".to_string(),
                    line_index: 0,
                },
                SearchMatch {
                    session_id: "focus-12345678".to_string(),
                    line_index: 2,
                },
            ]
        );
        assert_eq!(dashboard.selected_search_match, 0);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("search /alpha.* matched 2 line(s) across 1 session(s) | n/N navigate matches")
        );
    }

    #[test]
    fn next_search_match_wraps_and_updates_scroll_offset() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line(OutputStream::Stdout, "alpha-1"),
                test_output_line(OutputStream::Stdout, "beta"),
                test_output_line(OutputStream::Stdout, "alpha-2"),
            ],
        );
        dashboard.search_query = Some(r"alpha-\d".to_string());
        dashboard.last_output_height = 1;
        dashboard.recompute_search_matches();

        dashboard.next_search_match();
        assert_eq!(dashboard.selected_search_match, 1);
        assert_eq!(dashboard.output_scroll_offset, 2);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some(r"search /alpha-\d match 2/2 | selected session")
        );

        dashboard.next_search_match();
        assert_eq!(dashboard.selected_search_match, 0);
        assert_eq!(dashboard.output_scroll_offset, 0);
    }

    #[test]
    fn submit_search_rejects_invalid_regex_and_keeps_input() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );

        dashboard.begin_search();
        for ch in "(".chars() {
            dashboard.push_input_char(ch);
        }
        dashboard.submit_search();

        assert_eq!(dashboard.search_input.as_deref(), Some("("));
        assert!(dashboard.search_query.is_none());
        assert!(dashboard.search_matches.is_empty());
        assert!(dashboard
            .operator_note
            .as_deref()
            .unwrap_or_default()
            .starts_with("invalid regex /(:"));
    }

    #[test]
    fn clear_search_resets_active_query_and_matches() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.search_input = Some("draft".to_string());
        dashboard.search_query = Some("alpha".to_string());
        dashboard.search_matches = vec![
            SearchMatch {
                session_id: "focus-12345678".to_string(),
                line_index: 1,
            },
            SearchMatch {
                session_id: "focus-12345678".to_string(),
                line_index: 3,
            },
        ];
        dashboard.selected_search_match = 1;

        dashboard.clear_search();

        assert!(dashboard.search_input.is_none());
        assert!(dashboard.search_query.is_none());
        assert!(dashboard.search_matches.is_empty());
        assert_eq!(dashboard.selected_search_match, 0);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("cleared output search")
        );
    }

    #[test]
    fn toggle_output_filter_keeps_only_stderr_lines() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line(OutputStream::Stdout, "stdout line"),
                test_output_line(OutputStream::Stderr, "stderr line"),
            ],
        );

        dashboard.toggle_output_filter();

        assert_eq!(dashboard.output_filter, OutputFilter::ErrorsOnly);
        assert_eq!(dashboard.visible_output_text(), "stderr line");
        assert_eq!(dashboard.output_title(), " Output errors ");
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("output filter set to errors")
        );
    }

    #[test]
    fn toggle_output_filter_cycles_tool_calls_and_file_changes() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line(OutputStream::Stdout, "normal output"),
                test_output_line(OutputStream::Stdout, "Read(src/lib.rs)"),
                test_output_line(OutputStream::Stdout, "Updated ecc2/src/tui/dashboard.rs"),
                test_output_line(OutputStream::Stderr, "stderr line"),
            ],
        );

        dashboard.toggle_output_filter();
        assert_eq!(dashboard.output_filter, OutputFilter::ErrorsOnly);
        assert_eq!(dashboard.visible_output_text(), "stderr line");

        dashboard.toggle_output_filter();
        assert_eq!(dashboard.output_filter, OutputFilter::ToolCallsOnly);
        assert_eq!(dashboard.visible_output_text(), "Read(src/lib.rs)");
        assert_eq!(dashboard.output_title(), " Output tool calls ");
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("output filter set to tool calls")
        );

        dashboard.toggle_output_filter();
        assert_eq!(dashboard.output_filter, OutputFilter::FileChangesOnly);
        assert_eq!(
            dashboard.visible_output_text(),
            "Updated ecc2/src/tui/dashboard.rs"
        );
        assert_eq!(dashboard.output_title(), " Output file changes ");
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("output filter set to file changes")
        );
    }

    #[test]
    fn search_matches_respect_error_only_filter() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line(OutputStream::Stdout, "alpha stdout"),
                test_output_line(OutputStream::Stderr, "alpha stderr"),
                test_output_line(OutputStream::Stderr, "beta stderr"),
            ],
        );
        dashboard.output_filter = OutputFilter::ErrorsOnly;
        dashboard.search_query = Some("alpha.*".to_string());
        dashboard.last_output_height = 1;

        dashboard.recompute_search_matches();

        assert_eq!(
            dashboard.search_matches,
            vec![SearchMatch {
                session_id: "focus-12345678".to_string(),
                line_index: 0,
            }]
        );
        assert_eq!(dashboard.visible_output_text(), "alpha stderr\nbeta stderr");
    }

    #[test]
    fn search_matches_respect_tool_call_filter() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line(OutputStream::Stdout, "alpha normal"),
                test_output_line(OutputStream::Stdout, "Read(alpha.rs)"),
                test_output_line(OutputStream::Stdout, "Write(beta.rs)"),
            ],
        );
        dashboard.output_filter = OutputFilter::ToolCallsOnly;
        dashboard.search_query = Some("alpha.*".to_string());
        dashboard.last_output_height = 1;

        dashboard.recompute_search_matches();

        assert_eq!(
            dashboard.search_matches,
            vec![SearchMatch {
                session_id: "focus-12345678".to_string(),
                line_index: 0,
            }]
        );
        assert_eq!(
            dashboard.visible_output_text(),
            "Read(alpha.rs)\nWrite(beta.rs)"
        );
    }

    #[test]
    fn search_matches_respect_file_change_filter() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line(OutputStream::Stdout, "alpha normal"),
                test_output_line(OutputStream::Stdout, "Updated alpha.rs"),
                test_output_line(OutputStream::Stdout, "Renamed beta.rs to gamma.rs"),
            ],
        );
        dashboard.output_filter = OutputFilter::FileChangesOnly;
        dashboard.search_query = Some("alpha.*".to_string());
        dashboard.last_output_height = 1;

        dashboard.recompute_search_matches();

        assert_eq!(
            dashboard.search_matches,
            vec![SearchMatch {
                session_id: "focus-12345678".to_string(),
                line_index: 0,
            }]
        );
        assert_eq!(
            dashboard.visible_output_text(),
            "Updated alpha.rs\nRenamed beta.rs to gamma.rs"
        );
    }

    #[test]
    fn cycle_output_time_filter_keeps_only_recent_lines() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line_minutes_ago(OutputStream::Stdout, "recent line", 5),
                test_output_line_minutes_ago(OutputStream::Stdout, "older line", 45),
                test_output_line_minutes_ago(OutputStream::Stdout, "stale line", 180),
            ],
        );

        dashboard.cycle_output_time_filter();

        assert_eq!(
            dashboard.output_time_filter,
            OutputTimeFilter::Last15Minutes
        );
        assert_eq!(dashboard.visible_output_text(), "recent line");
        assert_eq!(dashboard.output_title(), " Output last 15m ");
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("output time filter set to last 15m")
        );
    }

    #[test]
    fn search_matches_respect_time_filter() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "focus-12345678",
                "planner",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![
                test_output_line_minutes_ago(OutputStream::Stdout, "alpha recent", 10),
                test_output_line_minutes_ago(OutputStream::Stdout, "beta recent", 10),
                test_output_line_minutes_ago(OutputStream::Stdout, "alpha stale", 180),
            ],
        );
        dashboard.output_time_filter = OutputTimeFilter::Last15Minutes;
        dashboard.search_query = Some("alpha.*".to_string());
        dashboard.last_output_height = 1;

        dashboard.recompute_search_matches();

        assert_eq!(
            dashboard.search_matches,
            vec![SearchMatch {
                session_id: "focus-12345678".to_string(),
                line_index: 0,
            }]
        );
        assert_eq!(dashboard.visible_output_text(), "alpha recent\nbeta recent");
    }

    #[test]
    fn search_scope_all_sessions_matches_across_output_buffers() {
        let mut dashboard = test_dashboard(
            vec![
                sample_session(
                    "focus-12345678",
                    "planner",
                    SessionState::Running,
                    None,
                    1,
                    1,
                ),
                sample_session(
                    "review-87654321",
                    "reviewer",
                    SessionState::Running,
                    None,
                    1,
                    1,
                ),
            ],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![test_output_line(OutputStream::Stdout, "alpha local")],
        );
        dashboard.session_output_cache.insert(
            "review-87654321".to_string(),
            vec![test_output_line(OutputStream::Stdout, "alpha global")],
        );
        dashboard.search_query = Some("alpha.*".to_string());

        dashboard.toggle_search_scope();

        assert_eq!(dashboard.search_scope, SearchScope::AllSessions);
        assert_eq!(
            dashboard.search_matches,
            vec![
                SearchMatch {
                    session_id: "focus-12345678".to_string(),
                    line_index: 0,
                },
                SearchMatch {
                    session_id: "review-87654321".to_string(),
                    line_index: 0,
                },
            ]
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("search scope set to all sessions | 2 match(es)")
        );
        assert_eq!(
            dashboard.output_title(),
            " Output all sessions /alpha.* 1/2 "
        );
    }

    #[test]
    fn next_search_match_switches_selected_session_in_all_sessions_scope() {
        let mut dashboard = test_dashboard(
            vec![
                sample_session(
                    "focus-12345678",
                    "planner",
                    SessionState::Running,
                    None,
                    1,
                    1,
                ),
                sample_session(
                    "review-87654321",
                    "reviewer",
                    SessionState::Running,
                    None,
                    1,
                    1,
                ),
            ],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![test_output_line(OutputStream::Stdout, "alpha local")],
        );
        dashboard.session_output_cache.insert(
            "review-87654321".to_string(),
            vec![test_output_line(OutputStream::Stdout, "alpha global")],
        );
        dashboard.search_scope = SearchScope::AllSessions;
        dashboard.search_query = Some("alpha.*".to_string());
        dashboard.last_output_height = 1;
        dashboard.recompute_search_matches();

        dashboard.next_search_match();

        assert_eq!(dashboard.selected_session_id(), Some("review-87654321"));
        assert_eq!(dashboard.selected_search_match, 1);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("search /alpha.* match 2/2 | all sessions")
        );
    }

    #[test]
    fn search_agent_filter_selected_agent_type_limits_global_search() {
        let mut dashboard = test_dashboard(
            vec![
                sample_session(
                    "focus-12345678",
                    "planner",
                    SessionState::Running,
                    None,
                    1,
                    1,
                ),
                sample_session(
                    "planner-2222222",
                    "planner",
                    SessionState::Running,
                    None,
                    1,
                    1,
                ),
                sample_session(
                    "review-87654321",
                    "reviewer",
                    SessionState::Running,
                    None,
                    1,
                    1,
                ),
            ],
            0,
        );
        dashboard.session_output_cache.insert(
            "focus-12345678".to_string(),
            vec![test_output_line(OutputStream::Stdout, "alpha local")],
        );
        dashboard.session_output_cache.insert(
            "planner-2222222".to_string(),
            vec![test_output_line(OutputStream::Stdout, "alpha planner")],
        );
        dashboard.session_output_cache.insert(
            "review-87654321".to_string(),
            vec![test_output_line(OutputStream::Stdout, "alpha reviewer")],
        );
        dashboard.search_scope = SearchScope::AllSessions;
        dashboard.search_query = Some("alpha.*".to_string());
        dashboard.recompute_search_matches();

        dashboard.toggle_search_agent_filter();

        assert_eq!(
            dashboard.search_agent_filter,
            SearchAgentFilter::SelectedAgentType
        );
        assert_eq!(
            dashboard.search_matches,
            vec![
                SearchMatch {
                    session_id: "focus-12345678".to_string(),
                    line_index: 0,
                },
                SearchMatch {
                    session_id: "planner-2222222".to_string(),
                    line_index: 0,
                },
            ]
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("search agent filter set to agent planner | 2 match(es)")
        );
        assert_eq!(
            dashboard.output_title(),
            " Output all sessions agent planner /alpha.* 1/2 "
        );
    }

    #[tokio::test]
    async fn stop_selected_uses_session_manager_transition() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "running-1".to_string(),
            task: "stop me".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            state: SessionState::Running,
            working_dir: PathBuf::from("/tmp"),
            pid: Some(999_999),
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.stop_selected().await;

        let session = db
            .get_session("running-1")?
            .expect("session should exist after stop");
        assert_eq!(session.state, SessionState::Stopped);
        assert_eq!(session.pid, None);

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn resume_selected_requeues_failed_session() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "failed-1".to_string(),
            task: "resume me".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            state: SessionState::Failed,
            working_dir: PathBuf::from("/tmp/ecc2-resume"),
            pid: None,
            worktree: Some(WorktreeInfo {
                path: PathBuf::from("/tmp/ecc2-resume"),
                branch: "ecc/failed-1".to_string(),
                base_branch: "main".to_string(),
            }),
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.resume_selected().await;

        let session = db
            .get_session("failed-1")?
            .expect("session should exist after resume");
        assert_eq!(session.state, SessionState::Pending);
        assert_eq!(session.pid, None);

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn cleanup_selected_worktree_clears_session_metadata() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();
        let worktree_path = std::env::temp_dir().join(format!("ecc2-cleanup-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&worktree_path)?;

        db.insert_session(&Session {
            id: "stopped-1".to_string(),
            task: "cleanup me".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            state: SessionState::Stopped,
            working_dir: worktree_path.clone(),
            pid: None,
            worktree: Some(WorktreeInfo {
                path: worktree_path.clone(),
                branch: "ecc/stopped-1".to_string(),
                base_branch: "main".to_string(),
            }),
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.cleanup_selected_worktree().await;

        let session = db
            .get_session("stopped-1")?
            .expect("session should exist after cleanup");
        assert!(
            session.worktree.is_none(),
            "worktree metadata should be cleared"
        );

        let _ = std::fs::remove_dir_all(worktree_path);
        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn prune_inactive_worktrees_sets_operator_note_when_clear() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "running-1".to_string(),
            task: "keep alive".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.prune_inactive_worktrees().await;

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("no inactive worktrees to prune")
        );

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn prune_inactive_worktrees_reports_pruned_and_skipped_counts() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();
        let active_path = std::env::temp_dir().join(format!("ecc2-active-{}", Uuid::new_v4()));
        let stopped_path = std::env::temp_dir().join(format!("ecc2-stopped-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&active_path)?;
        std::fs::create_dir_all(&stopped_path)?;

        db.insert_session(&Session {
            id: "running-1".to_string(),
            task: "keep worktree".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: active_path.clone(),
            state: SessionState::Running,
            pid: None,
            worktree: Some(WorktreeInfo {
                path: active_path.clone(),
                branch: "ecc/running-1".to_string(),
                base_branch: "main".to_string(),
            }),
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;
        db.insert_session(&Session {
            id: "stopped-1".to_string(),
            task: "prune me".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: stopped_path.clone(),
            state: SessionState::Stopped,
            pid: None,
            worktree: Some(WorktreeInfo {
                path: stopped_path.clone(),
                branch: "ecc/stopped-1".to_string(),
                base_branch: "main".to_string(),
            }),
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.prune_inactive_worktrees().await;

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("pruned 1 inactive worktree(s); skipped 1 active session(s)")
        );
        assert!(db
            .get_session("stopped-1")?
            .expect("stopped session should exist")
            .worktree
            .is_none());
        assert!(db
            .get_session("running-1")?
            .expect("running session should exist")
            .worktree
            .is_some());

        let _ = std::fs::remove_dir_all(active_path);
        let _ = std::fs::remove_dir_all(stopped_path);
        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn prune_inactive_worktrees_reports_retained_sessions_within_retention() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();
        let retained_path = std::env::temp_dir().join(format!("ecc2-retained-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&retained_path)?;

        db.insert_session(&Session {
            id: "stopped-1".to_string(),
            task: "retain me".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: retained_path.clone(),
            state: SessionState::Stopped,
            pid: None,
            worktree: Some(WorktreeInfo {
                path: retained_path.clone(),
                branch: "ecc/stopped-1".to_string(),
                base_branch: "main".to_string(),
            }),
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let mut cfg = Config::default();
        cfg.db_path = db_path.clone();
        cfg.worktree_retention_secs = 3600;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, cfg);
        dashboard.prune_inactive_worktrees().await;

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("deferred 1 inactive worktree(s) within retention")
        );
        assert!(db
            .get_session("stopped-1")?
            .expect("stopped session should exist")
            .worktree
            .is_some());

        let _ = std::fs::remove_dir_all(retained_path);
        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn merge_selected_worktree_sets_operator_note_when_ready() -> Result<()> {
        let tempdir = std::env::temp_dir().join(format!("dashboard-merge-{}", Uuid::new_v4()));
        let repo_root = tempdir.join("repo");
        init_git_repo(&repo_root)?;

        let cfg = build_config(&tempdir);
        let db = StateStore::open(&cfg.db_path)?;
        let worktree = worktree::create_for_session_in_repo("merge1234", &cfg, &repo_root)?;
        let session_id = "merge1234".to_string();
        let now = Utc::now();
        db.insert_session(&Session {
            id: session_id.clone(),
            task: "merge via dashboard".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: worktree.path.clone(),
            state: SessionState::Completed,
            pid: None,
            worktree: Some(worktree.clone()),
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        std::fs::write(worktree.path.join("dashboard.txt"), "dashboard merge\n")?;
        Command::new("git")
            .arg("-C")
            .arg(&worktree.path)
            .args(["add", "dashboard.txt"])
            .status()?;
        Command::new("git")
            .arg("-C")
            .arg(&worktree.path)
            .args(["commit", "-qm", "dashboard work"])
            .status()?;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.sync_selection_by_id(Some(&session_id));
        dashboard.merge_selected_worktree().await;

        let note = dashboard
            .operator_note
            .clone()
            .context("operator note should be set")?;
        assert!(note.contains("merged ecc/merge1234 into"));
        assert!(note.contains(&format!("for {}", format_session_id(&session_id))));

        let session = dashboard
            .db
            .get_session(&session_id)?
            .context("merged session should still exist")?;
        assert!(
            session.worktree.is_none(),
            "worktree metadata should be cleared"
        );
        assert!(!worktree.path.exists(), "worktree path should be removed");
        assert_eq!(
            std::fs::read_to_string(repo_root.join("dashboard.txt"))?,
            "dashboard merge\n"
        );

        let _ = std::fs::remove_dir_all(&tempdir);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn merge_ready_worktrees_sets_operator_note_with_skip_summary() -> Result<()> {
        let tempdir =
            std::env::temp_dir().join(format!("dashboard-merge-ready-{}", Uuid::new_v4()));
        let repo_root = tempdir.join("repo");
        init_git_repo(&repo_root)?;

        let cfg = build_config(&tempdir);
        let db = StateStore::open(&cfg.db_path)?;
        let now = Utc::now();

        let merged_worktree =
            worktree::create_for_session_in_repo("merge-ready", &cfg, &repo_root)?;
        std::fs::write(
            merged_worktree.path.join("merged.txt"),
            "dashboard bulk merge\n",
        )?;
        Command::new("git")
            .arg("-C")
            .arg(&merged_worktree.path)
            .args(["add", "merged.txt"])
            .status()?;
        Command::new("git")
            .arg("-C")
            .arg(&merged_worktree.path)
            .args(["commit", "-qm", "dashboard bulk merge"])
            .status()?;
        db.insert_session(&Session {
            id: "merge-ready".to_string(),
            task: "merge via dashboard".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: merged_worktree.path.clone(),
            state: SessionState::Completed,
            pid: None,
            worktree: Some(merged_worktree.clone()),
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let active_worktree =
            worktree::create_for_session_in_repo("active-ready", &cfg, &repo_root)?;
        db.insert_session(&Session {
            id: "active-ready".to_string(),
            task: "still active".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: active_worktree.path.clone(),
            state: SessionState::Running,
            pid: Some(999),
            worktree: Some(active_worktree.clone()),
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let mut dashboard = Dashboard::new(db, cfg);
        dashboard.merge_ready_worktrees().await;

        let note = dashboard
            .operator_note
            .clone()
            .context("operator note should be set")?;
        assert!(note.contains("merged 1 ready worktree(s)"));
        assert!(note.contains("skipped 1 active"));
        assert!(dashboard
            .db
            .get_session("merge-ready")?
            .context("merged session should still exist")?
            .worktree
            .is_none());
        assert_eq!(
            std::fs::read_to_string(repo_root.join("merged.txt"))?,
            "dashboard bulk merge\n"
        );

        let _ = std::fs::remove_dir_all(&tempdir);
        Ok(())
    }

    #[tokio::test]
    async fn delete_selected_session_removes_inactive_session() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "done-1".to_string(),
            task: "delete me".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Completed,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.delete_selected_session().await;

        assert!(
            db.get_session("done-1")?.is_none(),
            "session should be deleted"
        );

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn auto_dispatch_backlog_sets_operator_note_when_clear() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "lead-1".to_string(),
            task: "coordinate".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.auto_dispatch_backlog().await;

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("no unread handoff backlog found")
        );

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn rebalance_selected_team_sets_operator_note_when_clear() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "lead-1".to_string(),
            task: "coordinate".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.rebalance_selected_team().await;

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("no delegate backlog needed rebalancing for lead-1")
        );

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn rebalance_all_teams_sets_operator_note_when_clear() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "lead-1".to_string(),
            task: "coordinate".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.rebalance_all_teams().await;

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("no delegate backlog needed global rebalancing")
        );

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn coordinate_backlog_sets_operator_note_when_clear() -> Result<()> {
        let db_path = std::env::temp_dir().join(format!("ecc2-dashboard-{}.db", Uuid::new_v4()));
        let db = StateStore::open(&db_path)?;
        let now = Utc::now();

        db.insert_session(&Session {
            id: "lead-1".to_string(),
            task: "coordinate".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            working_dir: PathBuf::from("/tmp"),
            state: SessionState::Running,
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics::default(),
        })?;

        let dashboard_store = StateStore::open(&db_path)?;
        let mut dashboard = Dashboard::new(dashboard_store, Config::default());
        dashboard.coordinate_backlog().await;

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("backlog already clear")
        );

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }

    #[test]
    fn grid_layout_renders_four_panes() {
        let mut dashboard = test_dashboard(
            vec![sample_session(
                "grid-1",
                "claude",
                SessionState::Running,
                None,
                1,
                1,
            )],
            0,
        );
        dashboard.cfg.pane_layout = PaneLayout::Grid;
        dashboard.pane_size_percent = DEFAULT_GRID_SIZE_PERCENT;

        let areas = dashboard.pane_areas(Rect::new(0, 0, 100, 40));
        let output_area = areas.output.expect("grid layout should include output");
        let metrics_area = areas.metrics.expect("grid layout should include metrics");
        let log_area = areas.log.expect("grid layout should include a log pane");

        assert!(output_area.x > areas.sessions.x);
        assert!(metrics_area.y > areas.sessions.y);
        assert!(log_area.x > metrics_area.x);
    }

    #[test]
    fn collapse_selected_pane_hides_metrics_and_moves_focus() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.selected_pane = Pane::Metrics;

        dashboard.collapse_selected_pane();

        assert_eq!(dashboard.selected_pane, Pane::Sessions);
        assert_eq!(
            dashboard.visible_panes(),
            vec![Pane::Sessions, Pane::Output]
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("collapsed metrics pane")
        );
    }

    #[test]
    fn collapse_selected_pane_rejects_sessions_and_last_detail_pane() {
        let mut dashboard = test_dashboard(Vec::new(), 0);

        dashboard.collapse_selected_pane();
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("cannot collapse sessions pane")
        );

        dashboard.selected_pane = Pane::Metrics;
        dashboard.collapse_selected_pane();
        dashboard.selected_pane = Pane::Output;
        dashboard.collapse_selected_pane();

        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("cannot collapse last detail pane")
        );
        assert_eq!(
            dashboard.visible_panes(),
            vec![Pane::Sessions, Pane::Output]
        );
    }

    #[test]
    fn restore_collapsed_panes_restores_hidden_tabs() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.selected_pane = Pane::Metrics;
        dashboard.collapse_selected_pane();

        dashboard.restore_collapsed_panes();

        assert_eq!(
            dashboard.visible_panes(),
            vec![Pane::Sessions, Pane::Output, Pane::Metrics]
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("restored 1 collapsed pane(s)")
        );
    }

    #[test]
    fn collapsed_grid_reflows_to_horizontal_detail_stack() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.pane_layout = PaneLayout::Grid;
        dashboard.pane_size_percent = DEFAULT_GRID_SIZE_PERCENT;
        dashboard.selected_pane = Pane::Log;
        dashboard.collapse_selected_pane();

        let areas = dashboard.pane_areas(Rect::new(0, 0, 100, 40));
        let output_area = areas.output.expect("output should stay visible");
        let metrics_area = areas.metrics.expect("metrics should stay visible");

        assert!(areas.log.is_none());
        assert_eq!(areas.sessions.height, 40);
        assert_eq!(output_area.width, metrics_area.width);
        assert!(metrics_area.y > output_area.y);
    }

    #[test]
    fn pane_resize_clamps_to_bounds() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.pane_layout = PaneLayout::Grid;
        dashboard.pane_size_percent = DEFAULT_GRID_SIZE_PERCENT;

        for _ in 0..20 {
            dashboard.adjust_pane_size_with_save(5, Path::new("/tmp/ecc2-noop.toml"), |_| Ok(()));
        }
        assert_eq!(dashboard.pane_size_percent, MAX_PANE_SIZE_PERCENT);

        for _ in 0..40 {
            dashboard.adjust_pane_size_with_save(-5, Path::new("/tmp/ecc2-noop.toml"), |_| Ok(()));
        }
        assert_eq!(dashboard.pane_size_percent, MIN_PANE_SIZE_PERCENT);
    }

    #[test]
    fn pane_navigation_skips_log_outside_grid_layouts() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.next_pane();
        dashboard.next_pane();
        dashboard.next_pane();
        assert_eq!(dashboard.selected_pane, Pane::Sessions);

        dashboard.cfg.pane_layout = PaneLayout::Grid;
        dashboard.pane_size_percent = DEFAULT_GRID_SIZE_PERCENT;
        dashboard.next_pane();
        dashboard.next_pane();
        dashboard.next_pane();
        assert_eq!(dashboard.selected_pane, Pane::Log);
    }

    #[test]
    fn focus_pane_number_selects_visible_panes_and_rejects_hidden_targets() {
        let mut dashboard = test_dashboard(Vec::new(), 0);

        dashboard.focus_pane_number(3);

        assert_eq!(dashboard.selected_pane, Pane::Metrics);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("focused metrics pane")
        );

        dashboard.focus_pane_number(4);

        assert_eq!(dashboard.selected_pane, Pane::Metrics);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("log pane is not visible")
        );
    }

    #[test]
    fn directional_pane_focus_uses_grid_neighbors() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.pane_layout = PaneLayout::Grid;
        dashboard.pane_size_percent = DEFAULT_GRID_SIZE_PERCENT;

        dashboard.focus_pane_right();
        assert_eq!(dashboard.selected_pane, Pane::Output);

        dashboard.focus_pane_down();
        assert_eq!(dashboard.selected_pane, Pane::Log);

        dashboard.focus_pane_left();
        assert_eq!(dashboard.selected_pane, Pane::Metrics);

        dashboard.focus_pane_up();
        assert_eq!(dashboard.selected_pane, Pane::Sessions);
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("focused sessions pane")
        );
    }

    #[test]
    fn configured_pane_navigation_keys_override_defaults() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.pane_navigation.focus_metrics = "e".to_string();
        dashboard.cfg.pane_navigation.move_left = "a".to_string();

        assert!(dashboard.handle_pane_navigation_key(KeyEvent::new(
            crossterm::event::KeyCode::Char('e'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(dashboard.selected_pane, Pane::Metrics);

        assert!(dashboard.handle_pane_navigation_key(KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(dashboard.selected_pane, Pane::Sessions);
    }

    #[test]
    fn pane_navigation_labels_use_configured_bindings() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.pane_navigation.focus_sessions = "q".to_string();
        dashboard.cfg.pane_navigation.focus_output = "w".to_string();
        dashboard.cfg.pane_navigation.focus_metrics = "e".to_string();
        dashboard.cfg.pane_navigation.focus_log = "r".to_string();
        dashboard.cfg.pane_navigation.move_left = "a".to_string();
        dashboard.cfg.pane_navigation.move_down = "s".to_string();
        dashboard.cfg.pane_navigation.move_up = "w".to_string();
        dashboard.cfg.pane_navigation.move_right = "d".to_string();

        assert_eq!(dashboard.pane_focus_shortcuts_label(), "q/w/e/r");
        assert_eq!(dashboard.pane_move_shortcuts_label(), "a/s/w/d");
    }

    #[test]
    fn pane_command_mode_handles_focus_and_cancel() {
        let mut dashboard = test_dashboard(Vec::new(), 0);

        dashboard.begin_pane_command_mode();
        assert!(dashboard.is_pane_command_mode());

        assert!(dashboard.handle_pane_command_key(KeyEvent::new(
            crossterm::event::KeyCode::Char('3'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(dashboard.selected_pane, Pane::Metrics);
        assert!(!dashboard.is_pane_command_mode());

        dashboard.begin_pane_command_mode();
        assert!(dashboard.handle_pane_command_key(KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some("pane command cancelled")
        );
        assert!(!dashboard.is_pane_command_mode());
    }

    #[test]
    fn pane_command_mode_sets_layout() {
        let tempdir = std::env::temp_dir().join(format!("ecc2-pane-command-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tempdir).unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &tempdir);

        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.pane_layout = PaneLayout::Horizontal;

        dashboard.begin_pane_command_mode();
        assert!(dashboard.handle_pane_command_key(KeyEvent::new(
            crossterm::event::KeyCode::Char('g'),
            crossterm::event::KeyModifiers::NONE,
        )));

        assert_eq!(dashboard.cfg.pane_layout, PaneLayout::Grid);
        assert!(dashboard
            .operator_note
            .as_deref()
            .is_some_and(|note| note.contains("pane layout set to grid | saved to ")));

        if let Some(home) = previous_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
        let _ = std::fs::remove_dir_all(tempdir);
    }

    #[test]
    fn cycle_pane_layout_rotates_and_hides_log_when_leaving_grid() {
        let tempdir = std::env::temp_dir().join(format!("ecc2-cycle-pane-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tempdir).unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &tempdir);

        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.pane_layout = PaneLayout::Grid;
        dashboard.cfg.linear_pane_size_percent = 44;
        dashboard.cfg.grid_pane_size_percent = 77;
        dashboard.pane_size_percent = 77;
        dashboard.selected_pane = Pane::Log;

        dashboard.cycle_pane_layout();

        assert_eq!(dashboard.cfg.pane_layout, PaneLayout::Horizontal);
        assert_eq!(dashboard.pane_size_percent, 44);
        assert_eq!(dashboard.selected_pane, Pane::Sessions);

        if let Some(home) = previous_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
        let _ = std::fs::remove_dir_all(tempdir);
    }

    #[test]
    fn cycle_pane_layout_persists_config() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        let tempdir = std::env::temp_dir().join(format!("ecc2-layout-policy-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tempdir).unwrap();
        let config_path = tempdir.join("ecc2.toml");

        dashboard.cycle_pane_layout_with_save(&config_path, |cfg| cfg.save_to_path(&config_path));

        assert_eq!(dashboard.cfg.pane_layout, PaneLayout::Vertical);
        let expected_note = format!(
            "pane layout set to vertical | saved to {}",
            config_path.display()
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some(expected_note.as_str())
        );

        let saved = std::fs::read_to_string(&config_path).unwrap();
        let loaded: Config = toml::from_str(&saved).unwrap();
        assert_eq!(loaded.pane_layout, PaneLayout::Vertical);
        let _ = std::fs::remove_dir_all(tempdir);
    }

    #[test]
    fn pane_resize_persists_linear_setting() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        let tempdir = std::env::temp_dir().join(format!("ecc2-pane-size-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tempdir).unwrap();
        let config_path = tempdir.join("ecc2.toml");

        dashboard.adjust_pane_size_with_save(5, &config_path, |cfg| cfg.save_to_path(&config_path));

        assert_eq!(dashboard.pane_size_percent, 40);
        assert_eq!(dashboard.cfg.linear_pane_size_percent, 40);
        let expected_note = format!(
            "pane size set to 40% for horizontal layout | saved to {}",
            config_path.display()
        );
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some(expected_note.as_str())
        );

        let saved = std::fs::read_to_string(&config_path).unwrap();
        let loaded: Config = toml::from_str(&saved).unwrap();
        assert_eq!(loaded.linear_pane_size_percent, 40);
        assert_eq!(loaded.grid_pane_size_percent, 50);
        let _ = std::fs::remove_dir_all(tempdir);
    }

    #[test]
    fn cycle_pane_layout_uses_persisted_grid_size() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.pane_layout = PaneLayout::Vertical;
        dashboard.cfg.linear_pane_size_percent = 41;
        dashboard.cfg.grid_pane_size_percent = 63;
        dashboard.pane_size_percent = 41;

        dashboard.cycle_pane_layout_with_save(Path::new("/tmp/ecc2-noop.toml"), |_| Ok(()));

        assert_eq!(dashboard.cfg.pane_layout, PaneLayout::Grid);
        assert_eq!(dashboard.pane_size_percent, 63);
    }

    #[test]
    fn auto_split_layout_after_spawn_prefers_vertical_for_two_live_sessions() {
        let mut dashboard = test_dashboard(
            vec![
                sample_session("running-1", "planner", SessionState::Running, None, 1, 1),
                sample_session("idle-1", "planner", SessionState::Idle, None, 1, 1),
            ],
            0,
        );

        let note = dashboard.auto_split_layout_after_spawn_with_save(
            2,
            Path::new("/tmp/ecc2-noop.toml"),
            |_| Ok(()),
        );

        assert_eq!(dashboard.cfg.pane_layout, PaneLayout::Vertical);
        assert_eq!(
            dashboard.pane_size_percent,
            dashboard.cfg.linear_pane_size_percent
        );
        assert_eq!(dashboard.selected_pane, Pane::Sessions);
        assert_eq!(
            note.as_deref(),
            Some("auto-split vertical layout for 2 live session(s)")
        );
    }

    #[test]
    fn auto_split_layout_after_spawn_prefers_grid_for_three_live_sessions() {
        let mut dashboard = test_dashboard(
            vec![
                sample_session("pending-1", "planner", SessionState::Pending, None, 1, 1),
                sample_session("running-1", "planner", SessionState::Running, None, 1, 1),
                sample_session("idle-1", "planner", SessionState::Idle, None, 1, 1),
            ],
            1,
        );
        dashboard.selected_pane = Pane::Output;

        let note = dashboard.auto_split_layout_after_spawn_with_save(
            2,
            Path::new("/tmp/ecc2-noop.toml"),
            |_| Ok(()),
        );

        assert_eq!(dashboard.cfg.pane_layout, PaneLayout::Grid);
        assert_eq!(
            dashboard.pane_size_percent,
            dashboard.cfg.grid_pane_size_percent
        );
        assert_eq!(dashboard.selected_pane, Pane::Sessions);
        assert_eq!(
            note.as_deref(),
            Some("auto-split grid layout for 3 live session(s)")
        );
    }

    #[test]
    fn auto_split_layout_after_spawn_focuses_sessions_when_layout_already_matches() {
        let mut dashboard = test_dashboard(
            vec![
                sample_session("pending-1", "planner", SessionState::Pending, None, 1, 1),
                sample_session("running-1", "planner", SessionState::Running, None, 1, 1),
                sample_session("idle-1", "planner", SessionState::Idle, None, 1, 1),
            ],
            1,
        );
        dashboard.cfg.pane_layout = PaneLayout::Grid;
        dashboard.selected_pane = Pane::Output;

        let note = dashboard.auto_split_layout_after_spawn_with_save(
            3,
            Path::new("/tmp/ecc2-noop.toml"),
            |_| Ok(()),
        );

        assert_eq!(dashboard.cfg.pane_layout, PaneLayout::Grid);
        assert_eq!(dashboard.selected_pane, Pane::Sessions);
        assert_eq!(
            note.as_deref(),
            Some("auto-focused sessions in grid layout for 3 live session(s)")
        );
    }

    #[test]
    fn post_spawn_selection_prefers_lead_for_multi_spawn() {
        let preferred = post_spawn_selection_id(
            Some("lead-12345678"),
            &["child-a".to_string(), "child-b".to_string()],
        );

        assert_eq!(preferred.as_deref(), Some("lead-12345678"));
    }

    #[test]
    fn post_spawn_selection_keeps_single_spawn_on_created_session() {
        let preferred = post_spawn_selection_id(Some("lead-12345678"), &["child-a".to_string()]);

        assert_eq!(preferred.as_deref(), Some("child-a"));
    }

    #[test]
    fn post_spawn_selection_falls_back_to_first_created_when_no_lead_exists() {
        let preferred =
            post_spawn_selection_id(None, &["child-a".to_string(), "child-b".to_string()]);

        assert_eq!(preferred.as_deref(), Some("child-a"));
    }

    #[test]
    fn toggle_theme_persists_config() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        let tempdir = std::env::temp_dir().join(format!("ecc2-theme-policy-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tempdir).unwrap();
        let config_path = tempdir.join("ecc2.toml");

        dashboard.toggle_theme_with_save(&config_path, |cfg| cfg.save_to_path(&config_path));

        assert_eq!(dashboard.cfg.theme, Theme::Light);
        let expected_note = format!("theme set to light | saved to {}", config_path.display());
        assert_eq!(
            dashboard.operator_note.as_deref(),
            Some(expected_note.as_str())
        );

        let saved = std::fs::read_to_string(&config_path).unwrap();
        let loaded: Config = toml::from_str(&saved).unwrap();
        assert_eq!(loaded.theme, Theme::Light);
        let _ = std::fs::remove_dir_all(tempdir);
    }

    #[test]
    fn light_theme_uses_light_palette_accent() {
        let mut dashboard = test_dashboard(Vec::new(), 0);
        dashboard.cfg.theme = Theme::Light;
        dashboard.selected_pane = Pane::Sessions;

        assert_eq!(
            dashboard.pane_border_style(Pane::Sessions),
            Style::default().fg(Color::Blue)
        );
        assert_eq!(dashboard.theme_palette().row_highlight_bg, Color::Gray);
    }

    fn test_output_line(stream: OutputStream, text: &str) -> OutputLine {
        OutputLine::new(stream, text, Utc::now().to_rfc3339())
    }

    fn test_output_line_minutes_ago(
        stream: OutputStream,
        text: &str,
        minutes_ago: i64,
    ) -> OutputLine {
        OutputLine::new(
            stream,
            text,
            (Utc::now() - chrono::Duration::minutes(minutes_ago)).to_rfc3339(),
        )
    }

    fn line_plain_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn text_plain_text(text: &Text<'_>) -> String {
        text.lines
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn test_dashboard(sessions: Vec<Session>, selected_session: usize) -> Dashboard {
        let selected_session = selected_session.min(sessions.len().saturating_sub(1));
        let cfg = Config::default();
        let notifier = DesktopNotifier::new(cfg.desktop_notifications.clone());
        let webhook_notifier = WebhookNotifier::new(cfg.webhook_notifications.clone());
        let last_session_states = sessions
            .iter()
            .map(|session| (session.id.clone(), session.state.clone()))
            .collect();
        let session_harnesses = sessions
            .iter()
            .map(|session| {
                (
                    session.id.clone(),
                    SessionHarnessInfo::detect(&session.agent_type, &session.working_dir)
                        .with_config_detection(&cfg, &session.working_dir),
                )
            })
            .collect();
        let output_store = SessionOutputStore::default();
        let output_rx = output_store.subscribe();
        let mut session_table_state = TableState::default();
        if !sessions.is_empty() {
            session_table_state.select(Some(selected_session));
        }

        Dashboard {
            db: StateStore::open(Path::new(":memory:")).expect("open test db"),
            pane_size_percent: configured_pane_size(&cfg, cfg.pane_layout),
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
            selected_session,
            show_help: false,
            operator_note: None,
            pane_command_mode: false,
            output_follow: true,
            output_scroll_offset: 0,
            last_output_height: 0,
            metrics_scroll_offset: 0,
            last_metrics_height: 0,
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
            last_cost_metrics_signature: None,
            last_tool_activity_signature: None,
            last_budget_alert_state: BudgetState::Normal,
            last_session_states,
            last_seen_approval_message_id: None,
        }
    }

    fn build_config(root: &Path) -> Config {
        Config {
            db_path: root.join("state.db"),
            worktree_root: root.join("worktrees"),
            worktree_branch_prefix: "ecc".to_string(),
            max_parallel_sessions: 4,
            max_parallel_worktrees: 4,
            worktree_retention_secs: 0,
            session_timeout_secs: 60,
            heartbeat_interval_secs: 5,
            auto_terminate_stale_sessions: false,
            default_agent: "claude".to_string(),
            default_agent_profile: None,
            harness_runners: Default::default(),
            agent_profiles: Default::default(),
            orchestration_templates: Default::default(),
            memory_connectors: Default::default(),
            computer_use_dispatch: crate::config::ComputerUseDispatchConfig::default(),
            auto_dispatch_unread_handoffs: false,
            auto_dispatch_limit_per_session: 5,
            auto_create_worktrees: true,
            auto_merge_ready_worktrees: false,
            desktop_notifications: crate::notifications::DesktopNotificationConfig::default(),
            webhook_notifications: crate::notifications::WebhookNotificationConfig::default(),
            completion_summary_notifications:
                crate::notifications::CompletionSummaryConfig::default(),
            cost_budget_usd: 10.0,
            token_budget: 500_000,
            budget_alert_thresholds: crate::config::Config::BUDGET_ALERT_THRESHOLDS,
            conflict_resolution: crate::config::ConflictResolutionConfig::default(),
            theme: Theme::Dark,
            pane_layout: PaneLayout::Horizontal,
            pane_navigation: Default::default(),
            linear_pane_size_percent: 35,
            grid_pane_size_percent: 50,
            risk_thresholds: Config::RISK_THRESHOLDS,
        }
    }

    fn init_git_repo(path: &Path) -> Result<()> {
        fs::create_dir_all(path)?;
        run_git(path, &["init", "-q"])?;
        run_git(path, &["config", "user.name", "ECC Tests"])?;
        run_git(path, &["config", "user.email", "ecc-tests@example.com"])?;
        fs::write(path.join("README.md"), "hello\n")?;
        run_git(path, &["add", "README.md"])?;
        run_git(path, &["commit", "-qm", "init"])?;
        Ok(())
    }

    fn run_git(path: &Path, args: &[&str]) -> Result<()> {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }

    fn git_stdout(path: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn sample_session(
        id: &str,
        agent_type: &str,
        state: SessionState,
        branch: Option<&str>,
        tokens_used: u64,
        duration_secs: u64,
    ) -> Session {
        Session {
            id: id.to_string(),
            task: "Render dashboard rows".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: agent_type.to_string(),
            state,
            working_dir: branch
                .map(|branch| PathBuf::from(format!("/tmp/{branch}")))
                .unwrap_or_else(|| PathBuf::from("/tmp")),
            pid: None,
            worktree: branch.map(|branch| WorktreeInfo {
                path: PathBuf::from(format!("/tmp/{branch}")),
                branch: branch.to_string(),
                base_branch: "main".to_string(),
            }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_heartbeat_at: Utc::now(),
            metrics: SessionMetrics {
                input_tokens: tokens_used.saturating_mul(3) / 4,
                output_tokens: tokens_used / 4,
                tokens_used,
                tool_calls: 4,
                files_changed: 2,
                duration_secs,
                cost_usd: 0.42,
            },
        }
    }

    fn budget_session(id: &str, tokens_used: u64, cost_usd: f64) -> Session {
        let now = Utc::now();
        Session {
            id: id.to_string(),
            task: "Budget tracking".to_string(),
            project: "workspace".to_string(),
            task_group: "general".to_string(),
            agent_type: "claude".to_string(),
            state: SessionState::Running,
            working_dir: PathBuf::from("/tmp"),
            pid: None,
            worktree: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: now,
            metrics: SessionMetrics {
                input_tokens: tokens_used.saturating_mul(3) / 4,
                output_tokens: tokens_used / 4,
                tokens_used,
                tool_calls: 0,
                files_changed: 0,
                duration_secs: 0,
                cost_usd,
            },
        }
    }

    fn render_dashboard_text(mut dashboard: Dashboard, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("create terminal");

        terminal
            .draw(|frame| dashboard.render(frame))
            .expect("render dashboard");

        let buffer = terminal.backend().buffer();
        buffer
            .content
            .chunks(buffer.area.width as usize)
            .map(|cells| cells.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
