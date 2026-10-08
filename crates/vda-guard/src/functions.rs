//! Function policy shared by scalar and table-valued calls.
use std::{collections::HashSet, sync::LazyLock};

use crate::{Dialect, Issue, Severity};

const POSTGRES_BLOCKED: &[&str] = &[
    "pg_sleep",
    "pg_sleep_for",
    "pg_sleep_until",
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_promote",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_stat_file",
    "pg_ls_logdir",
    "pg_ls_waldir",
    "pg_ls_tmpdir",
    "pg_ls_archive_statusdir",
    "lo_import",
    "lo_export",
    "lo_unlink",
    "lo_from_bytea",
    "lo_put",
    "dblink",
    "dblink_exec",
    "dblink_connect",
    "dblink_send_query",
    "set_config",
    "pg_advisory_lock",
    "pg_advisory_xact_lock",
    "pg_advisory_lock_shared",
    "pg_try_advisory_lock",
    "pg_create_restore_point",
    "pg_switch_wal",
    "pg_start_backup",
    "pg_stop_backup",
    "pg_backup_start",
    "pg_backup_stop",
    "pg_create_logical_replication_slot",
    "pg_create_physical_replication_slot",
    "pg_drop_replication_slot",
    "pg_logical_emit_message",
    "pg_file_write",
    "pg_file_unlink",
    "pg_file_rename",
    "query_to_xml",
    "query_to_xml_and_xmlschema",
    "cursor_to_xml",
    "table_to_xml",
    "database_to_xml",
    "schema_to_xml",
    "txid_current",
    "pg_current_xact_id",
    "nextval",
    "setval",
    "pg_notify",
    "pg_import_system_collations",
];
const MYSQL_BLOCKED: &[&str] = &[
    "sleep",
    "benchmark",
    "load_file",
    "get_lock",
    "release_lock",
    "release_all_locks",
    "master_pos_wait",
    "source_pos_wait",
    "wait_for_executed_gtid_set",
    "sys_exec",
    "sys_eval",
];
const POSTGRES_WARNED: &[&str] = &["generate_series"];
const MYSQL_WARNED: &[&str] = &["uuid_short"];
static PG_BLOCK: LazyLock<HashSet<&str>> =
    LazyLock::new(|| POSTGRES_BLOCKED.iter().copied().collect());
static MY_BLOCK: LazyLock<HashSet<&str>> =
    LazyLock::new(|| MYSQL_BLOCKED.iter().copied().collect());
static PG_WARN: LazyLock<HashSet<&str>> =
    LazyLock::new(|| POSTGRES_WARNED.iter().copied().collect());
static MY_WARN: LazyLock<HashSet<&str>> = LazyLock::new(|| MYSQL_WARNED.iter().copied().collect());

pub(crate) fn inspect(name: &str, dialect: Dialect) -> Option<Issue> {
    let last = name.rsplit('.').next().unwrap_or(name);
    let (blocked, warned) = match dialect {
        Dialect::Postgres => (&*PG_BLOCK, &*PG_WARN),
        Dialect::MySql => (&*MY_BLOCK, &*MY_WARN),
    };
    if blocked.contains(last) || (dialect == Dialect::MySql && name.starts_with("sys.")) {
        Some(Issue::new(Severity::Block, "dangerous_function", format!("Function {name} can change database state, access files, acquire locks, or disrupt production. Remove this call.")))
    } else if warned.contains(last) {
        let (code, message) = if last == "generate_series" {
            ("unbounded_generator", format!("Function {name} can generate a large number of rows. Bound its arguments and use a LIMIT."))
        } else {
            ("stateful_function", format!("Function {name} allocates identifiers and changes server state. Use a deterministic expression if possible."))
        };
        Some(Issue::new(Severity::Warning, code, message))
    } else {
        None
    }
}

pub(crate) fn is_aggregate(name: &str) -> bool {
    matches!(
        name.rsplit('.').next(),
        Some(
            "count"
                | "sum"
                | "avg"
                | "min"
                | "max"
                | "bool_and"
                | "bool_or"
                | "every"
                | "array_agg"
                | "string_agg"
                | "json_agg"
                | "jsonb_agg"
                | "group_concat"
        )
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    #[test]
    fn every_listed_function_is_enforced() {
        for (dialect, names) in [
            (Dialect::Postgres, POSTGRES_BLOCKED),
            (Dialect::MySql, MYSQL_BLOCKED),
        ] {
            for name in names {
                let sql = format!("SELECT {name}()");
                let policy = crate::GuardPolicy {
                    max_rows: 10,
                    allow_writes: true,
                    require_approval_for_writes: false,
                    allow_ddl: true,
                    blocked_tables: vec![],
                };
                let a = crate::analyze(&sql, dialect, crate::AccessLevel::Admin, &policy);
                assert_eq!(a.verdict, crate::Verdict::Deny, "{name}: {a:?}");
                assert!(
                    a.issues.iter().any(|i| i.code == "dangerous_function"),
                    "{name}: {a:?}"
                );
            }
        }
    }
}
