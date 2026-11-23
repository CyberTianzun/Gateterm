//! Shared server-side agent integration operations (plan preview, install, uninstall, purge).

use std::path::{Path, PathBuf};

use crate::agent::apply::{apply, default_backup_dir, ApplyError, ApplyOpts, Outcome};
use crate::agent::edit::{ConfigEdit, EditOp, InstallCtx};
use crate::agent::{plan_purge, AgentAdapter};

const DEFAULT_BACKUP_KEEP: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanGroup {
    pub path: PathBuf,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPlanSummary {
    pub install: Vec<PlanGroup>,
    pub uninstall: Vec<PlanGroup>,
    pub include_blocking: bool,
}

#[derive(Debug)]
pub struct MutateResult {
    pub include_blocking: bool,
    pub outcomes: Vec<Outcome>,
}

/// Returns `true` when installing blocking hooks has no foreign blocking conflicts.
pub fn wants_blocking(adapter: &dyn AgentAdapter, home: &Path, port: u16) -> bool {
    let ctx = InstallCtx { home: home.to_path_buf(), port, include_blocking: true };
    adapter.integration(&ctx).1.is_empty()
}

/// Projects a [`ConfigEdit`] into `(action, event_label, optional_spec)`.
pub fn describe_edit(edit: &ConfigEdit) -> (&'static str, String, Option<serde_json::Value>) {
    match &edit.op {
        EditOp::EnsureHook { event, group } => ("ensure", event.clone(), Some(group.clone())),
        EditOp::RemoveOurs { event } => ("remove", event.clone(), None),
        EditOp::PurgeOurs => ("purge", "*".into(), None),
        EditOp::WriteOwnedFile { .. } => ("ensure", "(extension file)".into(), None),
        EditOp::RemoveOwnedFile => ("remove", "(extension file)".into(), None),
        EditOp::SetOwnedKey { key, value } => {
            let events: Vec<&str> =
                value.as_object().into_iter().flat_map(|o| o.keys()).map(String::as_str).collect();
            ("ensure", format!("{key}: {}", events.join(", ")), Some(value.clone()))
        }
        EditOp::RemoveOwnedKey { key } => ("remove", format!("{key}: *"), None),
    }
}

/// Groups config edits by target file path while preserving first-seen path and event order.
pub fn summarize_edits(edits: &[ConfigEdit]) -> Vec<PlanGroup> {
    let mut groups: Vec<PlanGroup> = Vec::new();
    for edit in edits {
        let (_, event, _) = describe_edit(edit);
        if let Some(group) = groups.iter_mut().find(|g| g.path == edit.path) {
            group.events.push(event);
        } else {
            groups.push(PlanGroup { path: edit.path.clone(), events: vec![event] });
        }
    }
    groups
}

/// Previews install and uninstall plans for `adapter` without modifying disk.
pub fn preview_plan(
    adapter: &dyn AgentAdapter,
    home: &Path,
    port: u16,
) -> anyhow::Result<AgentPlanSummary> {
    let include_blocking = wants_blocking(adapter, home, port);
    let install_ctx = InstallCtx { home: home.to_path_buf(), port, include_blocking };
    let uninstall_ctx = InstallCtx { home: home.to_path_buf(), port, include_blocking: true };
    let install = summarize_edits(&adapter.plan_install(&install_ctx)?);
    let uninstall = summarize_edits(&adapter.plan_uninstall(&uninstall_ctx)?);
    Ok(AgentPlanSummary { install, uninstall, include_blocking })
}

fn plan_err(home: &Path, err: anyhow::Error) -> ApplyError {
    ApplyError::Io { path: home.to_path_buf(), detail: err.to_string() }
}

fn default_opts(home: &Path, enabled: bool) -> Result<ApplyOpts, ApplyError> {
    let backup_dir = default_backup_dir().ok_or_else(|| ApplyError::Io {
        path: home.to_path_buf(),
        detail: "cannot resolve backup dir".into(),
    })?;
    Ok(ApplyOpts { enabled, backup_dir, backup_keep: DEFAULT_BACKUP_KEEP })
}

/// Installs hooks/config for `adapter` using explicit [`ApplyOpts`].
pub fn install_with_opts(
    adapter: &dyn AgentAdapter,
    home: &Path,
    port: u16,
    opts: &ApplyOpts,
) -> Result<MutateResult, ApplyError> {
    if !opts.enabled {
        return Err(ApplyError::Disabled);
    }
    let include_blocking = wants_blocking(adapter, home, port);
    let ctx = InstallCtx { home: home.to_path_buf(), port, include_blocking };
    let plan = adapter.plan_install(&ctx).map_err(|e| plan_err(home, e))?;
    let outcomes = apply(&plan, opts)?;
    Ok(MutateResult { include_blocking, outcomes })
}

/// Uninstalls hooks/config for `adapter` using explicit [`ApplyOpts`].
pub fn uninstall_with_opts(
    adapter: &dyn AgentAdapter,
    home: &Path,
    port: u16,
    opts: &ApplyOpts,
) -> Result<MutateResult, ApplyError> {
    if !opts.enabled {
        return Err(ApplyError::Disabled);
    }
    let ctx = InstallCtx { home: home.to_path_buf(), port, include_blocking: true };
    let plan = adapter.plan_uninstall(&ctx).map_err(|e| plan_err(home, e))?;
    let outcomes = apply(&plan, opts)?;
    Ok(MutateResult { include_blocking: true, outcomes })
}

/// Purges all gateterm-owned hooks/files for `adapter` regardless of `install_enabled`.
pub fn purge_with_opts(
    adapter: &dyn AgentAdapter,
    home: &Path,
    backup_dir: PathBuf,
    backup_keep: usize,
) -> Result<Vec<Outcome>, ApplyError> {
    let ctx = InstallCtx { home: home.to_path_buf(), port: 0, include_blocking: true };
    let plan = plan_purge(adapter, &ctx).map_err(|e| plan_err(home, e))?;
    apply(&plan, &ApplyOpts { enabled: true, backup_dir, backup_keep })
}

/// Installs hooks/config for `adapter` using the default backup directory.
pub fn install(
    adapter: &dyn AgentAdapter,
    home: &Path,
    port: u16,
    enabled: bool,
) -> Result<MutateResult, ApplyError> {
    if !enabled {
        return Err(ApplyError::Disabled);
    }
    let opts = default_opts(home, enabled)?;
    install_with_opts(adapter, home, port, &opts)
}

/// Uninstalls hooks/config for `adapter` using the default backup directory.
pub fn uninstall(
    adapter: &dyn AgentAdapter,
    home: &Path,
    port: u16,
    enabled: bool,
) -> Result<MutateResult, ApplyError> {
    if !enabled {
        return Err(ApplyError::Disabled);
    }
    let opts = default_opts(home, enabled)?;
    uninstall_with_opts(adapter, home, port, &opts)
}

/// Purges all gateterm-owned hooks/files for `adapter` using the default backup directory.
pub fn purge(adapter: &dyn AgentAdapter, home: &Path) -> Result<Vec<Outcome>, ApplyError> {
    let opts = default_opts(home, true)?;
    purge_with_opts(adapter, home, opts.backup_dir, opts.backup_keep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::edit::OWNED_MARKER;
    use serde_json::json;

    #[test]
    fn summarize_edits_groups_by_path_preserving_order() {
        let path_a = PathBuf::from("/tmp/a.json");
        let path_b = PathBuf::from("/tmp/b.ts");
        let edits = vec![
            ConfigEdit {
                path: path_a.clone(),
                op: EditOp::EnsureHook { event: "Stop".into(), group: json!({}) },
            },
            ConfigEdit {
                path: path_b.clone(),
                op: EditOp::WriteOwnedFile { content: format!("// {OWNED_MARKER}\n") },
            },
            ConfigEdit {
                path: path_a.clone(),
                op: EditOp::EnsureHook { event: "PreToolUse".into(), group: json!({}) },
            },
        ];
        assert_eq!(
            summarize_edits(&edits),
            vec![
                PlanGroup { path: path_a, events: vec!["Stop".into(), "PreToolUse".into()] },
                PlanGroup { path: path_b, events: vec!["(extension file)".into()] },
            ]
        );
    }

    #[test]
    fn wants_blocking_respects_foreign_blocking_conflicts() {
        let adapter = crate::agent::registry()[0];
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        assert!(wants_blocking(adapter, home, 7683));

        // Write a foreign blocking hook on the same event at the adapter's install target path.
        let plan = adapter
            .plan_install(&InstallCtx { home: home.to_path_buf(), port: 7683, include_blocking: true })
            .unwrap();
        let target = &plan[0].path;
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(
            target,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PermissionRequest": [
                        {
                            "matcher": "",
                            "hooks": [
                                { "type": "http", "url": "http://127.0.0.1:9999/other-tool" }
                            ]
                        }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(!wants_blocking(adapter, home, 7683));
    }

    #[test]
    fn install_uninstall_and_purge_with_opts_are_idempotent_and_create_backups() {
        let adapter = crate::agent::registry()[0];
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let backup_dir = home.join("backups");

        let disabled_opts =
            ApplyOpts { enabled: false, backup_dir: backup_dir.clone(), backup_keep: 5 };
        assert!(matches!(
            install_with_opts(adapter, home, 7683, &disabled_opts),
            Err(ApplyError::Disabled)
        ));
        assert!(matches!(
            uninstall_with_opts(adapter, home, 7683, &disabled_opts),
            Err(ApplyError::Disabled)
        ));

        let opts = ApplyOpts { enabled: true, backup_dir: backup_dir.clone(), backup_keep: 5 };
        let first = install_with_opts(adapter, home, 7683, &opts).unwrap();
        assert!(first.include_blocking);
        assert!(first.outcomes.iter().any(|o| o.changed));

        let second = install_with_opts(adapter, home, 7683, &opts).unwrap();
        assert!(!second.outcomes.iter().any(|o| o.changed));

        let uninst = uninstall_with_opts(adapter, home, 7683, &opts).unwrap();
        assert!(uninst.include_blocking);
        assert!(uninst.outcomes.iter().any(|o| o.changed));
        assert!(uninst.outcomes.iter().any(|o| o.backup.is_some()));

        // Re-install with stale residual hook, then purge (always works regardless of enabled).
        install_with_opts(adapter, home, 7684, &opts).unwrap();
        let purged = purge_with_opts(adapter, home, backup_dir, 5).unwrap();
        assert!(purged.iter().any(|o| o.changed));
        assert!(purged.iter().any(|o| o.backup.is_some()));
        let (state, _) = adapter.integration(&InstallCtx {
            home: home.to_path_buf(),
            port: 7683,
            include_blocking: true,
        });
        assert_eq!(state, crate::agent::Integration::NotInstalled);
    }
}
