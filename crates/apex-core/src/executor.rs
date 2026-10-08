//! Transactional application of an optimization plan, with verification,
//! automatic rollback on failure, per-change / per-batch undo and crash recovery.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::actions::{Action, BackendError, SystemBackend};
use crate::ledger::{BatchStatus, ChangeStatus, Ledger, LedgerError};

#[derive(Debug, Error)]
pub enum ExecError {
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error("{0}")]
    Invalid(String),
    #[error("the setting was changed by something else since APEX applied it (now {current}); undo would overwrite that change")]
    Conflict { current: String },
    #[error(transparent)]
    Backend(#[from] BackendError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", content = "message", rename_all = "camelCase")]
pub enum Outcome {
    Applied,
    Skipped(String),
    Failed(String),
    RolledBack,
    NotAttempted(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub change_id: Option<String>,
    pub description: String,
    pub reversible: bool,
    pub outcome: Outcome,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanReport {
    pub batch_id: String,
    pub status: BatchStatus,
    pub results: Vec<ActionResult>,
    /// Plain-language summary of exactly what is now different on the system.
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    pub change_id: String,
    pub description: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub change_id: String,
    pub description: String,
    pub resolved_status: ChangeStatus,
}

pub struct Executor<'a> {
    pub backend: &'a dyn SystemBackend,
    pub ledger: &'a Ledger,
}

impl<'a> Executor<'a> {
    pub fn new(backend: &'a dyn SystemBackend, ledger: &'a Ledger) -> Self {
        Self { backend, ledger }
    }

    /// Apply a set of actions as one batch.
    ///
    /// Reversible actions run first, in order. If any of them fails, every change
    /// already applied in this batch is restored (in reverse order) and the
    /// non-reversible actions are not run at all. Non-reversible actions only run
    /// once all reversible ones succeeded.
    pub fn apply_plan(&self, label: &str, actions: &[Action]) -> Result<PlanReport, ExecError> {
        if actions.is_empty() {
            return Err(ExecError::Invalid("no actions selected".into()));
        }
        let batch_id = self.ledger.create_batch(label)?;
        let (reversible, irreversible): (Vec<&Action>, Vec<&Action>) =
            actions.iter().partition(|a| a.is_reversible());

        let mut results: Vec<ActionResult> = Vec::new();
        let mut applied: Vec<(usize, String)> = Vec::new(); // (index in results, change id)
        let mut failed = false;
        let mut unrestored = false;
        let mut seq = 0i64;

        for action in &reversible {
            seq += 1;
            let (setting, target) = action.target().expect("partitioned as reversible");
            let desc = action.describe();

            let before = match self.backend.read(&setting) {
                Ok(v) => v,
                Err(e) => {
                    let id = self.ledger.record_change(
                        &batch_id,
                        seq,
                        action,
                        Some(&setting),
                        None,
                        Some(&target),
                        ChangeStatus::Failed,
                    )?;
                    self.ledger.update_change(
                        &id,
                        ChangeStatus::Failed,
                        Some(&e.to_string()),
                        Some("could not read current value; nothing was changed"),
                    )?;
                    results.push(ActionResult {
                        change_id: Some(id),
                        description: desc,
                        reversible: true,
                        outcome: Outcome::Failed(e.to_string()),
                        detail: Some("Nothing was changed.".into()),
                    });
                    failed = true;
                    break;
                }
            };

            if before == target {
                let id = self.ledger.record_change(
                    &batch_id,
                    seq,
                    action,
                    Some(&setting),
                    Some(&before),
                    Some(&target),
                    ChangeStatus::Skipped,
                )?;
                results.push(ActionResult {
                    change_id: Some(id),
                    description: desc,
                    reversible: true,
                    outcome: Outcome::Skipped("already set".into()),
                    detail: None,
                });
                continue;
            }

            // Record intent before touching the system (crash safety).
            let id = self.ledger.record_change(
                &batch_id,
                seq,
                action,
                Some(&setting),
                Some(&before),
                Some(&target),
                ChangeStatus::Pending,
            )?;

            let write_result = self.backend.write(&setting, &target);
            let verified = match &write_result {
                Ok(()) => match self.backend.read(&setting) {
                    Ok(v) if v == target => Ok(()),
                    Ok(v) => Err(format!("verification failed: value is {v:?} after write")),
                    Err(e) => Err(format!("verification read failed: {e}")),
                },
                Err(e) => Err(e.to_string()),
            };

            match verified {
                Ok(()) => {
                    self.ledger
                        .update_change(&id, ChangeStatus::Applied, None, None)?;
                    results.push(ActionResult {
                        change_id: Some(id.clone()),
                        description: desc,
                        reversible: true,
                        outcome: Outcome::Applied,
                        detail: None,
                    });
                    applied.push((results.len() - 1, id));
                }
                Err(msg) => {
                    // The write may have partially happened; put the original back.
                    let restore = self.restore_value(&setting, &before);
                    let detail = match &restore {
                        Ok(()) => "Original value confirmed in place.".to_string(),
                        Err(e) => {
                            format!("WARNING: could not confirm original value was restored: {e}")
                        }
                    };
                    let status = if restore.is_ok() {
                        ChangeStatus::Failed
                    } else {
                        unrestored = true;
                        ChangeStatus::NeedsAttention
                    };
                    self.ledger
                        .update_change(&id, status, Some(&msg), Some(&detail))?;
                    results.push(ActionResult {
                        change_id: Some(id),
                        description: desc,
                        reversible: true,
                        outcome: Outcome::Failed(msg),
                        detail: Some(detail),
                    });
                    failed = true;
                    break;
                }
            }
        }

        if failed {
            // Roll back everything this batch applied, newest first.
            for (idx, change_id) in applied.iter().rev() {
                let rec = self.ledger.get_change(change_id)?;
                let (Some(setting), Some(before)) = (rec.setting.as_ref(), rec.before.as_ref())
                else {
                    continue;
                };
                match self.restore_value(setting, before) {
                    Ok(()) => {
                        self.ledger.update_change(
                            change_id,
                            ChangeStatus::RolledBack,
                            None,
                            Some("restored because a later step failed"),
                        )?;
                        results[*idx].outcome = Outcome::RolledBack;
                    }
                    Err(e) => {
                        unrestored = true;
                        let msg = format!("rollback failed: {e}");
                        self.ledger.update_change(
                            change_id,
                            ChangeStatus::NeedsAttention,
                            Some(&msg),
                            None,
                        )?;
                        results[*idx].outcome = Outcome::Failed(msg);
                    }
                }
            }
            // Mark remaining reversible actions as not attempted.
            for action in reversible.iter().skip(results.len()) {
                results.push(ActionResult {
                    change_id: None,
                    description: action.describe(),
                    reversible: true,
                    outcome: Outcome::NotAttempted("an earlier step failed".into()),
                    detail: None,
                });
            }
            for action in &irreversible {
                results.push(ActionResult {
                    change_id: None,
                    description: action.describe(),
                    reversible: false,
                    outcome: Outcome::NotAttempted("an earlier step failed".into()),
                    detail: None,
                });
            }
            let all_restored = !unrestored;
            let status = if all_restored {
                BatchStatus::RolledBack
            } else {
                BatchStatus::Failed
            };
            self.ledger.set_batch_status(&batch_id, status)?;
            let summary = if all_restored {
                "A step failed, so APEX restored every setting it had changed in this batch. Your system is as it was.".to_string()
            } else {
                "A step failed and at least one setting could not be restored automatically. See the items marked failed.".to_string()
            };
            return Ok(PlanReport {
                batch_id,
                status,
                results,
                summary,
            });
        }

        // Irreversible actions (currently: temp cleanup).
        let mut irreversible_failed = false;
        for action in &irreversible {
            seq += 1;
            let id = self.ledger.record_change(
                &batch_id,
                seq,
                action,
                None,
                None,
                None,
                ChangeStatus::Pending,
            )?;
            let res = match action {
                Action::CleanTempFiles { older_than_days } => self
                    .backend
                    .clean_temp(*older_than_days)
                    .map(|r| r.summary()),
                other => Err(BackendError::Unsupported(format!(
                    "{other:?} is not an irreversible action"
                ))),
            };
            match res {
                Ok(detail) => {
                    self.ledger
                        .update_change(&id, ChangeStatus::Applied, None, Some(&detail))?;
                    results.push(ActionResult {
                        change_id: Some(id),
                        description: action.describe(),
                        reversible: false,
                        outcome: Outcome::Applied,
                        detail: Some(detail),
                    });
                }
                Err(e) => {
                    irreversible_failed = true;
                    self.ledger.update_change(
                        &id,
                        ChangeStatus::Failed,
                        Some(&e.to_string()),
                        None,
                    )?;
                    results.push(ActionResult {
                        change_id: Some(id),
                        description: action.describe(),
                        reversible: false,
                        outcome: Outcome::Failed(e.to_string()),
                        detail: None,
                    });
                }
            }
        }

        let applied_count = results
            .iter()
            .filter(|r| r.outcome == Outcome::Applied)
            .count();
        let status = if irreversible_failed {
            BatchStatus::PartiallyApplied
        } else {
            BatchStatus::Applied
        };
        self.ledger.set_batch_status(&batch_id, status)?;
        let summary = format!(
            "{applied_count} change(s) applied and verified, {} already in place{}.",
            results
                .iter()
                .filter(|r| matches!(r.outcome, Outcome::Skipped(_)))
                .count(),
            if irreversible_failed {
                "; the cleanup step failed (nothing else was affected)"
            } else {
                ""
            }
        );
        Ok(PlanReport {
            batch_id,
            status,
            results,
            summary,
        })
    }

    fn restore_value(
        &self,
        setting: &crate::actions::Setting,
        value: &crate::actions::SettingValue,
    ) -> Result<(), BackendError> {
        // If the original value is still in place (e.g. the write was rejected), don't write at all.
        if self.backend.read(setting).ok().as_ref() == Some(value) {
            return Ok(());
        }
        self.backend.write(setting, value)?;
        let now = self.backend.read(setting)?;
        if &now == value {
            Ok(())
        } else {
            Err(BackendError::Os(format!(
                "value after restore is {now:?}, expected {value:?}"
            )))
        }
    }

    /// Undo one applied change. Refuses if something else has changed the setting
    /// since, unless `force` is set.
    pub fn undo_change(&self, change_id: &str, force: bool) -> Result<UndoReport, ExecError> {
        let rec = self.ledger.get_change(change_id)?;
        let description = rec.action.describe();
        if !rec.reversible {
            return Err(ExecError::Invalid(format!(
                "\"{description}\" cannot be undone (files were deleted)"
            )));
        }
        if rec.status != ChangeStatus::Applied {
            return Err(ExecError::Invalid(format!(
                "change is {:?}, only applied changes can be undone",
                rec.status
            )));
        }
        let (Some(setting), Some(before), Some(after)) = (rec.setting, rec.before, rec.after)
        else {
            return Err(ExecError::Invalid("change record is incomplete".into()));
        };
        let current = self.backend.read(&setting)?;
        if current == before {
            self.ledger.update_change(
                change_id,
                ChangeStatus::Reverted,
                None,
                Some("original value was already in place"),
            )?;
            return Ok(UndoReport {
                change_id: change_id.into(),
                description,
                ok: true,
                message: "Already at the original value.".into(),
            });
        }
        if current != after && !force {
            return Err(ExecError::Conflict {
                current: format!("{current:?}"),
            });
        }
        match self.restore_value(&setting, &before) {
            Ok(()) => {
                self.ledger
                    .update_change(change_id, ChangeStatus::Reverted, None, None)?;
                self.refresh_batch_status(&rec.batch_id)?;
                Ok(UndoReport {
                    change_id: change_id.into(),
                    description,
                    ok: true,
                    message: "Original value restored and verified.".into(),
                })
            }
            Err(e) => Ok(UndoReport {
                change_id: change_id.into(),
                description,
                ok: false,
                message: e.to_string(),
            }),
        }
    }

    /// Undo every applied reversible change in a batch, newest first.
    pub fn undo_batch(&self, batch_id: &str, force: bool) -> Result<Vec<UndoReport>, ExecError> {
        let batch = self.ledger.get_batch(batch_id)?;
        let mut out = Vec::new();
        for rec in batch.changes.iter().rev() {
            if rec.reversible && rec.status == ChangeStatus::Applied {
                match self.undo_change(&rec.id, force) {
                    Ok(r) => out.push(r),
                    Err(e) => out.push(UndoReport {
                        change_id: rec.id.clone(),
                        description: rec.action.describe(),
                        ok: false,
                        message: e.to_string(),
                    }),
                }
            }
        }
        self.refresh_batch_status(batch_id)?;
        Ok(out)
    }

    fn refresh_batch_status(&self, batch_id: &str) -> Result<(), ExecError> {
        let batch = self.ledger.get_batch(batch_id)?;
        let any_applied = batch
            .changes
            .iter()
            .any(|c| c.reversible && c.status == ChangeStatus::Applied);
        let any_reverted = batch
            .changes
            .iter()
            .any(|c| c.status == ChangeStatus::Reverted);
        if !any_applied && any_reverted {
            self.ledger
                .set_batch_status(batch_id, BatchStatus::Reverted)?;
        } else if any_applied && any_reverted {
            self.ledger
                .set_batch_status(batch_id, BatchStatus::PartiallyApplied)?;
        }
        Ok(())
    }

    /// Resolve changes left `pending` by a crash. Call once at startup.
    pub fn recover(&self) -> Result<Vec<RecoveryReport>, ExecError> {
        let mut out = Vec::new();
        for rec in self.ledger.changes_with_status(ChangeStatus::Pending)? {
            let description = rec.action.describe();
            let resolved = match (&rec.setting, &rec.before, &rec.after) {
                (Some(setting), Some(before), Some(after)) => match self.backend.read(setting) {
                    Ok(v) if &v == after => ChangeStatus::Applied,
                    Ok(v) if &v == before => ChangeStatus::Failed,
                    _ => ChangeStatus::NeedsAttention,
                },
                // Irreversible step interrupted: we can't know how far it got.
                _ => ChangeStatus::NeedsAttention,
            };
            let note = match resolved {
                ChangeStatus::Applied => "recovered after interruption: change was in place",
                ChangeStatus::Failed => "recovered after interruption: change never took effect",
                _ => "interrupted; current state could not be matched to the original or target value",
            };
            self.ledger
                .update_change(&rec.id, resolved, None, Some(note))?;
            out.push(RecoveryReport {
                change_id: rec.id,
                description,
                resolved_status: resolved,
            });
        }
        // Batches stuck in progress get re-derived from their changes.
        for batch in self.ledger.history(10_000)? {
            if batch.status == BatchStatus::InProgress {
                let any_applied = batch
                    .changes
                    .iter()
                    .any(|c| c.status == ChangeStatus::Applied);
                self.ledger.set_batch_status(
                    &batch.id,
                    if any_applied {
                        BatchStatus::PartiallyApplied
                    } else {
                        BatchStatus::Failed
                    },
                )?;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{Setting, SettingValue};
    use crate::storage::CleanupReport;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// In-memory backend used only by tests to exercise transaction logic.
    #[derive(Default)]
    struct MemBackend {
        values: Mutex<HashMap<String, SettingValue>>,
        fail_write_for: Mutex<Option<String>>,
        /// Write "succeeds" but stores a different value (simulates a silently ignored write).
        lie_for: Mutex<Option<String>>,
    }

    fn key(s: &Setting) -> String {
        serde_json::to_string(s).unwrap()
    }

    impl MemBackend {
        fn set(&self, s: &Setting, v: SettingValue) {
            self.values.lock().insert(key(s), v);
        }
        fn get(&self, s: &Setting) -> SettingValue {
            self.values
                .lock()
                .get(&key(s))
                .cloned()
                .unwrap_or(SettingValue::Absent)
        }
    }

    impl SystemBackend for MemBackend {
        fn read(&self, s: &Setting) -> Result<SettingValue, BackendError> {
            Ok(self.get(s))
        }
        fn write(&self, s: &Setting, v: &SettingValue) -> Result<(), BackendError> {
            if self.fail_write_for.lock().as_deref() == Some(key(s).as_str()) {
                return Err(BackendError::Os("access denied".into()));
            }
            if self.lie_for.lock().as_deref() == Some(key(s).as_str()) {
                return Ok(());
            }
            self.set(s, v.clone());
            Ok(())
        }
        fn temp_roots(&self) -> Vec<PathBuf> {
            vec![]
        }
        fn clean_temp(&self, _d: u32) -> Result<CleanupReport, BackendError> {
            Ok(CleanupReport::default())
        }
    }

    fn power(guid: &str) -> Action {
        Action::SetPowerScheme {
            guid: guid.into(),
            name: guid.into(),
        }
    }
    fn startup(id: &str, enabled: bool) -> Action {
        Action::SetStartupEntryEnabled {
            entry_id: id.into(),
            name: id.into(),
            enabled,
        }
    }

    fn setup() -> (MemBackend, Ledger) {
        let b = MemBackend::default();
        b.set(
            &Setting::ActivePowerScheme,
            SettingValue::Text("balanced".into()),
        );
        b.set(
            &Setting::StartupEntryEnabled {
                entry_id: "a".into(),
            },
            SettingValue::Bool(true),
        );
        b.set(
            &Setting::StartupEntryEnabled {
                entry_id: "b".into(),
            },
            SettingValue::Bool(true),
        );
        b.set(&Setting::GameMode, SettingValue::Bool(false));
        (b, Ledger::open_in_memory().unwrap())
    }

    #[test]
    fn applies_verifies_and_undoes_batch() {
        let (b, l) = setup();
        let ex = Executor::new(&b, &l);
        let rep = ex
            .apply_plan(
                "t",
                &[
                    power("high"),
                    startup("a", false),
                    Action::SetGameMode { enabled: true },
                ],
            )
            .unwrap();
        assert_eq!(rep.status, BatchStatus::Applied);
        assert!(rep.results.iter().all(|r| r.outcome == Outcome::Applied));
        assert_eq!(
            b.get(&Setting::ActivePowerScheme),
            SettingValue::Text("high".into())
        );

        let undo = ex.undo_batch(&rep.batch_id, false).unwrap();
        assert_eq!(undo.len(), 3);
        assert!(undo.iter().all(|u| u.ok));
        assert_eq!(
            b.get(&Setting::ActivePowerScheme),
            SettingValue::Text("balanced".into())
        );
        assert_eq!(
            b.get(&Setting::StartupEntryEnabled {
                entry_id: "a".into()
            }),
            SettingValue::Bool(true)
        );
        assert_eq!(b.get(&Setting::GameMode), SettingValue::Bool(false));
        assert_eq!(
            l.get_batch(&rep.batch_id).unwrap().status,
            BatchStatus::Reverted
        );
    }

    #[test]
    fn failure_rolls_back_earlier_changes_and_skips_irreversible() {
        let (b, l) = setup();
        *b.fail_write_for.lock() = Some(key(&Setting::StartupEntryEnabled {
            entry_id: "b".into(),
        }));
        let ex = Executor::new(&b, &l);
        let rep = ex
            .apply_plan(
                "t",
                &[
                    power("high"),
                    startup("a", false),
                    startup("b", false),
                    Action::CleanTempFiles { older_than_days: 7 },
                ],
            )
            .unwrap();
        assert_eq!(rep.status, BatchStatus::RolledBack);
        assert_eq!(rep.results[0].outcome, Outcome::RolledBack);
        assert_eq!(rep.results[1].outcome, Outcome::RolledBack);
        assert!(matches!(rep.results[2].outcome, Outcome::Failed(_)));
        assert!(matches!(rep.results[3].outcome, Outcome::NotAttempted(_)));
        // System is exactly as before.
        assert_eq!(
            b.get(&Setting::ActivePowerScheme),
            SettingValue::Text("balanced".into())
        );
        assert_eq!(
            b.get(&Setting::StartupEntryEnabled {
                entry_id: "a".into()
            }),
            SettingValue::Bool(true)
        );
    }

    #[test]
    fn silent_write_failure_is_caught_by_verification() {
        let (b, l) = setup();
        *b.lie_for.lock() = Some(key(&Setting::GameMode));
        let ex = Executor::new(&b, &l);
        let rep = ex
            .apply_plan("t", &[power("high"), Action::SetGameMode { enabled: true }])
            .unwrap();
        assert_eq!(rep.status, BatchStatus::RolledBack);
        assert!(
            matches!(&rep.results[1].outcome, Outcome::Failed(m) if m.contains("verification"))
        );
        assert_eq!(
            b.get(&Setting::ActivePowerScheme),
            SettingValue::Text("balanced".into())
        );
    }

    #[test]
    fn already_set_is_skipped_not_logged_as_change() {
        let (b, l) = setup();
        let ex = Executor::new(&b, &l);
        let rep = ex.apply_plan("t", &[power("balanced")]).unwrap();
        assert!(matches!(rep.results[0].outcome, Outcome::Skipped(_)));
        assert!(ex.undo_batch(&rep.batch_id, false).unwrap().is_empty());
    }

    #[test]
    fn undo_refuses_when_setting_changed_externally() {
        let (b, l) = setup();
        let ex = Executor::new(&b, &l);
        let rep = ex.apply_plan("t", &[power("high")]).unwrap();
        b.set(
            &Setting::ActivePowerScheme,
            SettingValue::Text("saver".into()),
        );
        let id = rep.results[0].change_id.clone().unwrap();
        assert!(matches!(
            ex.undo_change(&id, false),
            Err(ExecError::Conflict { .. })
        ));
        assert!(ex.undo_change(&id, true).unwrap().ok);
        assert_eq!(
            b.get(&Setting::ActivePowerScheme),
            SettingValue::Text("balanced".into())
        );
    }

    #[test]
    fn absent_values_are_restored_as_absent() {
        let (b, l) = setup();
        let s = Setting::StartupEntryEnabled {
            entry_id: "never-approved".into(),
        };
        let ex = Executor::new(&b, &l);
        let rep = ex
            .apply_plan("t", &[startup("never-approved", false)])
            .unwrap();
        assert_eq!(b.get(&s), SettingValue::Bool(false));
        ex.undo_batch(&rep.batch_id, false).unwrap();
        assert_eq!(b.get(&s), SettingValue::Absent);
    }

    #[test]
    fn crash_recovery_resolves_pending_changes() {
        let (b, l) = setup();
        let batch = l.create_batch("crashed").unwrap();
        let a = power("high");
        let (s, t) = a.target().unwrap();
        let id1 = l
            .record_change(
                &batch,
                1,
                &a,
                Some(&s),
                Some(&SettingValue::Text("balanced".into())),
                Some(&t),
                ChangeStatus::Pending,
            )
            .unwrap();
        b.set(&s, t.clone()); // the write landed before the crash
        let g = Action::SetGameMode { enabled: true };
        let (gs, gt) = g.target().unwrap();
        let id2 = l
            .record_change(
                &batch,
                2,
                &g,
                Some(&gs),
                Some(&SettingValue::Bool(false)),
                Some(&gt),
                ChangeStatus::Pending,
            )
            .unwrap();

        let ex = Executor::new(&b, &l);
        let rep = ex.recover().unwrap();
        assert_eq!(rep.len(), 2);
        assert_eq!(l.get_change(&id1).unwrap().status, ChangeStatus::Applied);
        assert_eq!(l.get_change(&id2).unwrap().status, ChangeStatus::Failed);
        assert_eq!(
            l.get_batch(&batch).unwrap().status,
            BatchStatus::PartiallyApplied
        );
        // and the recovered change can still be undone
        assert!(ex.undo_change(&id1, false).unwrap().ok);
    }

    #[test]
    fn irreversible_actions_cannot_be_undone() {
        let (b, l) = setup();
        let ex = Executor::new(&b, &l);
        let rep = ex
            .apply_plan("t", &[Action::CleanTempFiles { older_than_days: 7 }])
            .unwrap();
        let id = rep.results[0].change_id.clone().unwrap();
        assert!(matches!(
            ex.undo_change(&id, false),
            Err(ExecError::Invalid(_))
        ));
    }
}
