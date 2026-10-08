//! Access and approval decisions after collecting structural safety issues.
use crate::{
    classify, tables, AccessLevel, GuardPolicy, Issue, Severity, StatementAnalysis, StatementKind,
    Verdict,
};

pub(crate) fn apply(
    analysis: &mut StatementAnalysis,
    level: AccessLevel,
    policy: &GuardPolicy,
    locking: bool,
) -> Verdict {
    for table in &analysis.tables {
        if tables::sensitive(table)
            || (classify::is_write(analysis.kind) && table.starts_with("performance_schema."))
        {
            analysis.issues.push(Issue::new(Severity::Block, "sensitive_catalog", format!("Relation {table} contains credentials or sensitive authorization data. Query a non-sensitive catalog instead.")));
        }
        if policy
            .blocked_tables
            .iter()
            .any(|pattern| tables::matches(pattern, table))
        {
            analysis.issues.push(Issue::new(Severity::Block, "blocked_table", format!("Relation {table} is blocked by this cluster's policy. Use an allowed relation or ask a cluster administrator to review the policy.")));
        }
    }
    let mut verdict = match analysis.kind {
        StatementKind::Select | StatementKind::Explain | StatementKind::Show => Verdict::Allow,
        kind if classify::is_write(kind) => {
            if level == AccessLevel::Read {
                block(
                    analysis,
                    "insufficient_access",
                    "Writes require Write or Admin access. Request a write grant.",
                );
            }
            if !policy.allow_writes {
                block(analysis, "writes_disabled", "Writes are disabled by this cluster's policy. Ask an administrator to review the policy.");
            }
            if policy.require_approval_for_writes {
                Verdict::RequiresApproval
            } else {
                Verdict::Allow
            }
        }
        StatementKind::Ddl => {
            if level != AccessLevel::Admin {
                block(
                    analysis,
                    "insufficient_access",
                    "Schema changes require Admin access. Request an administrator's review.",
                );
            }
            if !policy.allow_ddl {
                block(analysis, "ddl_disabled", "Schema changes are disabled by this cluster's policy. Ask an administrator to review the policy.");
            }
            Verdict::RequiresApproval
        }
        _ => {
            block(analysis, "statement_not_allowed", "This statement can change session, transaction, or server state and is not allowed through the gateway. Use a supported query or an administrator's maintenance workflow.");
            Verdict::Deny
        }
    };
    if locking {
        if level == AccessLevel::Read {
            block(analysis, "locking_read", "This SELECT acquires row locks and can block production writes. Remove the locking clause or request Write access and approval.");
        } else {
            analysis.issues.push(Issue::new(Severity::Warning, "locking_read", "This SELECT acquires row locks and may block other transactions. Approval is required; keep the query narrowly filtered."));
            verdict = worst(verdict, Verdict::RequiresApproval);
        }
    }
    if analysis
        .issues
        .iter()
        .any(|i| i.severity == Severity::Block)
    {
        Verdict::Deny
    } else {
        verdict
    }
}

fn block(analysis: &mut StatementAnalysis, code: &str, message: &str) {
    analysis
        .issues
        .push(Issue::new(Severity::Block, code, message));
}

pub(crate) fn worst(left: Verdict, right: Verdict) -> Verdict {
    match (left, right) {
        (Verdict::Deny, _) | (_, Verdict::Deny) => Verdict::Deny,
        (Verdict::RequiresApproval, _) | (_, Verdict::RequiresApproval) => {
            Verdict::RequiresApproval
        }
        _ => Verdict::Allow,
    }
}
