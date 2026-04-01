---
name: dbx-dba
description: Database operations and DBA-style safe change workflow using the bundled dbx CLI for MySQL, PostgreSQL, and SQLite. This skill package embeds a platform-specific dbx binary at ./assets/bin/dbx on Linux and macOS, or ./assets/bin/dbx.exe on Windows.
---
# dbx DBA

Use this skill for safe DBA-style work with `dbx`: schema inspection, query review, DML/DDL review, production readiness checks, controlled execution, and post-change verification.

## Embedded Binary

Use the bundled `dbx` executable from the installed skill instead of assuming `dbx` is present on the host machine.

- Linux and macOS: `./assets/bin/dbx`
- Windows: `./assets/bin/dbx.exe`

Resolve that path relative to the installed skill root, not the current project directory. If the binary is missing, the `.skill` package was built incorrectly.

Load extra detail only when needed:

- `references/dbx-usage.md` for config, policy selection, and command patterns
- `references/checklists.md` for step-by-step execution checklists
- `references/risk-patterns.md` for migration and DML risk patterns to call out explicitly

## Operating Rules

1. Inspect first, change second.
2. Before any schema or data modification, review compatibility with old logic:
   existing queries, ORM expectations, old column names/types/nullability/defaults, old write paths, downstream jobs, and rollback feasibility.
3. Identify affected queries, services, cron jobs, reports, ETL, and code paths if known. If unknown, say so as a risk.
4. Summarize risks before destructive or write operations. At minimum call out:
   lock risk, index risk, null/default risk, backward compatibility risk, and blast radius.
5. Prefer `readonly` or `prod_safe` profiles for inspection. Use `migration_only` only for approved schema changes. Do not use broader write-capable profiles unless the task explicitly requires it.
6. Prefer dry-run thinking even when the tool has no literal dry-run flag:
   inspect current state, sample candidate rows, run `EXPLAIN`, narrow predicates, stage rollout, and verify row counts before writing.
7. Do not treat "DDL" as automatically safe. Review whether it rewrites tables, blocks writes, invalidates old code, or breaks rollback.
8. Treat `dbx` SQL classification as heuristic, not a full parser. If a statement is ambiguous or vendor-specific, say so and review it manually before execution.

## Policy Selection

- `readonly`: default for inspection, troubleshooting, schema discovery, and query review.
- `prod_safe`: same effective permissions as `readonly`; use when the environment is production-like and you want the policy intent to be explicit.
- `migration_only`: use only for approved schema changes that do not require reads through the same profile.
- `all` or custom write-capable policies: only when a reviewed DML task truly requires writes. If used, still require explicit risk summary and `--write`.

If a task mixes inspection and schema change, inspect with `readonly`/`prod_safe` first, then switch to `migration_only` only for the execution step.

## Default Workflow

### 1. Read-Only Inspection

Use this for incident triage, schema discovery, and pre-change review.

1. Confirm target profile, engine, environment, and intended scope.
2. Inspect objects before reasoning about changes:
   `dbx tables`, `dbx schema <table>`, targeted `dbx query`, and `dbx explain`.
3. Capture the current state that matters:
   table shape, indexes if visible through schema/introspection queries, row counts, candidate rows, and the current query plan.
4. State what is known vs unknown before proposing a change.

### 2. Schema Change Review

Do this before any `ALTER`, `CREATE`, `DROP`, `RENAME`, `TRUNCATE`, or similar change.

1. Inspect the current table, dependent objects, and access patterns first.
2. Review compatibility with old logic:
   old readers, old writers, old defaults, old null assumptions, old column names/types, and deployment order between app and DB.
3. Call out concrete risks:
   table rewrite/lock risk, index build/drop risk, nullability/default backfill risk, rename/drop breakage, and rollback difficulty.
4. Prefer additive and staged changes:
   add nullable column first, dual-write/backfill, add index safely, switch reads later, drop old objects last.
5. If execution is approved, use a `migration_only` profile where possible.

### 3. DML Change Review

Do this before `UPDATE`, `DELETE`, `INSERT`, `MERGE`, `REPLACE`, or data backfills.

1. Inspect target rows first with read-only queries.
2. Validate the predicate with sample rows and row counts.
3. Review compatibility with old logic:
   readers expecting current values, writes racing with the change, triggers/jobs depending on current state, and replay/idempotency concerns.
4. Call out risks:
   unintended row scope, long-running locks, write amplification, index misses causing scans, and rollback complexity.
5. Prefer staged execution:
   smaller batches, bounded predicates, ordered processing, verification between batches.
6. Before execution, give a short risk summary and expected affected-row range.

### 4. Production Safety Checks

Before any production or production-like change:

1. Verify you are on the intended profile and that the policy matches the step.
2. Confirm there is a rollback or containment plan.
3. Check whether the statement can lock hot tables, rebuild indexes, scan large ranges, or violate SLAs.
4. Check whether old app versions can still read/write correctly during rollout.
5. Prefer maintenance windows, staged rollout, or small batches when risk is non-trivial.
6. If risk remains unclear, stop at review and surface the unknowns rather than executing.

### 5. Rollback / Backup Considerations

Always discuss rollback before executing non-trivial writes.

- For schema changes, state whether rollback is:
  immediate, delayed, lossy, or effectively impossible without restore.
- For DML, state whether rows can be reconstructed, reversed by predicate, or require backup/snapshot/binlog/PITR support.
- If a change drops or overwrites information, explicitly say so before execution.
- Prefer reversible sequences:
  additive schema, shadow columns/tables, precomputed restore predicates, exported key lists, or backups/snapshots.

### 6. Post-Change Verification

After approved execution:

1. Re-run targeted inspection queries.
2. Verify schema shape, row counts, spot-check rows, and query plans relevant to the change.
3. Compare expected vs actual affected rows and call out any mismatch.
4. Note follow-up actions still required:
   backfill completion, index validation, app rollout, cleanup of old columns, or further monitoring.

## Execution Guidance

- Use the embedded binary path above when invoking `dbx` from the installed skill package.
- Use `dbx query` for reads and sampling.
- Use `dbx explain` before risky reads/writes when plan quality matters.
- Use `dbx exec --write` only after review. Never skip the risk summary for destructive or mutating SQL.
- If the task asks for execution, provide a concise preflight summary first:
  target profile, statement class, expected blast radius, main risks, rollback notes, and verification plan.
- If the request is underspecified, stop after inspection and review. Do not guess predicates or migration sequencing.
