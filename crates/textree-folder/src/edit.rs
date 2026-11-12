//! Planning and applying changes to a folder, with the outcome as the lines a person or an agent
//! reads. Every front end (the command line, a binding) goes through these, so they say the same
//! thing for the same input.

use textree::{
    build_graph, plan, recheck, validate_paths, ChangeSet, CommitError, Graph, Op, Plan, PlanError,
    Policy, RecheckError, Snapshot,
};

use serde_json::Value;

use crate::FolderStore;

/// What kind of failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// The request itself is wrong (exit code 2 on the command line).
    Usage,
    /// The change would break an invariant, or a plan does not match its operations.
    Refused,
    /// The folder changed since the plan was made.
    Stale,
    /// Writing failed part-way and was undone.
    Failed,
}

/// A failure, as lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub lines: Vec<String>,
}

impl Failure {
    pub fn usage(message: impl std::fmt::Display) -> Self {
        Failure {
            kind: FailureKind::Usage,
            lines: vec![format!("error: {message}")],
        }
    }

    /// The exit code of the command line for it.
    pub fn exit_code(&self) -> u8 {
        match self.kind {
            FailureKind::Usage => 2,
            _ => 1,
        }
    }

    /// The lines, each ending with a line break.
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| format!("{l}\n")).collect()
    }
}

impl From<CommitError> for Failure {
    fn from(e: CommitError) -> Self {
        match e {
            CommitError::Invalid(why) => Failure::usage(why),
            CommitError::Stale(paths) => {
                let mut lines: Vec<String> = paths.iter().map(|p| format!("stale: {p}")).collect();
                lines.push(
                    "the folder changed since the plan was made; nothing was written".to_string(),
                );
                Failure {
                    kind: FailureKind::Stale,
                    lines,
                }
            }
            CommitError::Conflict(paths) => Failure {
                kind: FailureKind::Refused,
                lines: paths
                    .iter()
                    .map(|p| format!("refused [I5] {p}: already exists"))
                    .collect(),
            },
            CommitError::Failed { path, message } => Failure {
                kind: FailureKind::Failed,
                lines: vec![if path.is_empty() {
                    format!("error: {message}")
                } else {
                    format!("failed at {path}: {message}")
                }],
            },
        }
    }
}

/// Plans one operation on the folder as it is now. The plan is also checked against the disk
/// (a destination behind a symbolic link is refused).
pub fn plan_change(store: &FolderStore, op: &Op, policy: &Policy) -> Result<Plan, Failure> {
    let snapshot = Snapshot::read(store);
    let planned = match plan(&snapshot, op, policy) {
        Ok(planned) => planned,
        Err(PlanError::Usage(message)) => return Err(Failure::usage(message)),
        Err(PlanError::Refused(violations)) => {
            return Err(Failure {
                kind: FailureKind::Refused,
                lines: violations.iter().map(ToString::to_string).collect(),
            })
        }
    };
    if let Err(CommitError::Invalid(why)) = store.preflight(&planned.changes) {
        return Err(Failure {
            kind: FailureKind::Refused,
            lines: vec![format!("refused [I5] {why}")],
        });
    }
    Ok(planned)
}

/// Writes a plan that was just made. After writing, and before it is final, the folder has to be
/// what the plan said it would be; if it is not, everything is put back.
pub fn commit_planned(store: &FolderStore, planned: &Plan) -> Result<(), Failure> {
    let verify = |s: &FolderStore| -> Result<(), String> {
        if same_links(&planned.expected, &build_graph(s)) {
            Ok(())
        } else {
            Err("the folder after writing does not match the plan".to_string())
        }
    };
    store
        .commit_checked(&planned.changes, &verify)
        .map(|_| ())
        .map_err(Failure::from)
}

/// Writes a plan that came from outside (a saved file, an agent) as the JSON it came in: only
/// when it is well formed, not stale, and exactly what its operations give on the folder as it
/// is now — the same JSON value, with no field added, left out or changed. `what` names the plan
/// in a message that says it is not one.
pub fn apply_outside(store: &FolderStore, received: &Value, what: &str) -> Result<Plan, Failure> {
    let changes: ChangeSet = serde_json::from_value(received.clone())
        .map_err(|e| Failure::usage(format!("{what}: {e}")))?;
    let changes = &changes;
    validate_paths(changes).map_err(Failure::usage)?;
    store.preflight(changes).map_err(Failure::from)?;
    let planned = match recheck(&Snapshot::read(store), changes) {
        Ok(planned) => planned,
        Err(RecheckError::Stale(paths)) => return Err(CommitError::Stale(paths).into()),
        Err(e) => {
            return Err(Failure {
                kind: FailureKind::Refused,
                lines: vec![e.to_string()],
            })
        }
    };
    if serde_json::to_value(&planned.changes).as_ref().ok() != Some(received) {
        return Err(Failure {
            kind: FailureKind::Refused,
            lines: vec![RecheckError::NotMatching.to_string()],
        });
    }
    commit_planned(store, &planned)?;
    Ok(planned)
}

fn same_links(a: &Graph, b: &Graph) -> bool {
    let project = |g: &Graph| -> Vec<(String, String, Option<String>)> {
        let mut v: Vec<_> = g
            .links
            .iter()
            .map(|l| (l.from.clone(), l.link.clone(), l.to.clone()))
            .collect();
        v.sort();
        v
    };
    project(a) == project(b) && a.errors.is_empty() == b.errors.is_empty()
}

/// The line that says what was written.
pub fn applied_line(changes: &ChangeSet) -> String {
    let mut parts = Vec::new();
    let n = changes.moves.len();
    if n > 0 {
        parts.push(format!("{n} {}", if n == 1 { "move" } else { "moves" }));
    }
    let n = changes.creates.len();
    if n > 0 {
        parts.push(format!("{n} created"));
    }
    let n = changes.rewrites.len();
    if n > 0 {
        parts.push(format!(
            "{n} {} rewritten",
            if n == 1 { "link" } else { "links" }
        ));
    }
    let n = changes.deletes.len();
    if n > 0 {
        parts.push(format!("{n} deleted"));
    }
    if parts.is_empty() && !changes.writes.is_empty() {
        let n = changes.writes.len();
        parts.push(format!(
            "{n} {} written",
            if n == 1 { "document" } else { "documents" }
        ));
    }
    format!("Applied: {}.", parts.join(", "))
}
