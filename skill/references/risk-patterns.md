# Risk Patterns

Use this file to pressure-test a plan before execution.

## Schema Risks

- **Lock risk**: `ALTER TABLE`, index builds/drops, constraint validation, or type changes may block reads/writes or hold long metadata locks.
- **Index risk**: dropping or changing an index can turn a bounded query into a table scan; adding an index may create long build time or write amplification.
- **Null/default risk**: adding `NOT NULL`, changing defaults, or backfilling can break old writes and old readers that assume previous behavior.
- **Backward compatibility risk**: renaming/dropping columns, changing types, or tightening constraints can break older application versions, scripts, ETL, reports, or ad hoc queries.
- **Rewrite risk**: some engines rewrite whole tables for type/default/column order changes; this increases runtime, lock time, and rollback pain.
- **Rollback risk**: destructive DDL is often not instantly reversible; rollback may require another migration or restore.

## DML Risks

- **Predicate risk**: missing or broad predicates update/delete too many rows.
- **Plan risk**: an unindexed predicate or bad plan causes large scans and long locks.
- **Hot-row risk**: touching frequently updated rows increases contention and deadlock risk.
- **Batching risk**: one large statement may exceed lock, WAL/binlog, or replication tolerance; batches reduce blast radius.
- **Idempotency risk**: retries can duplicate effects unless the statement is designed to be safely repeatable.
- **Reconstruction risk**: destructive updates/deletes may be irreversible without snapshots, exports, or logs.

## Compatibility Review Prompts

Ask these before changing schema or data:

- What old code still reads this field or table?
- What old code still writes it?
- Are null, default, or enum/value assumptions changing?
- Are there jobs, ETL, reports, or dashboards outside the main app path?
- Can old and new application versions coexist during rollout?
- If we stop halfway, what state is the database in?

## Safer Patterns

- Add before remove.
- Backfill before enforcing stricter constraints.
- Dual-read or dual-write during transitions when needed.
- Batch large DML with verification between batches.
- Inspect first, then execute the smallest safe change.
