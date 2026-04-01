# DBA Checklists

Use these checklists when executing real work. Keep the spoken/written summary short, but cover the items that materially affect safety.

## Read-Only Inspection

- Confirm profile, engine, database, and environment.
- List tables or inspect the target table/view first.
- Inspect schema before writing any SQL against it.
- Sample rows with a narrow `SELECT`.
- Get row counts or approximate scope when relevant.
- Run `EXPLAIN` on important queries.
- Record known unknowns: indexes, triggers, dependent jobs, app paths.

## Schema Change Review

- What objects change: table, columns, indexes, constraints, views.
- Is the change additive, rewriting, renaming, or destructive.
- Will it lock the table or block writes/reads.
- Does it require backfill or default population.
- Can old app versions still read and write during rollout.
- Are existing queries sensitive to nullability, defaults, renamed columns, or type changes.
- Can rollback happen safely, and what data would be lost.
- What should be verified immediately after execution.

## DML Change Review

- Show the exact predicate and intended row set.
- Inspect sample rows that match.
- Estimate affected-row count before execution.
- Check indexes supporting the predicate.
- Check for long transactions or hot-table contention risk.
- Decide whether batching is needed.
- State how to reverse or reconstruct changed rows if needed.
- Define post-write verification queries before running the change.

## Production Safety Checks

- Correct profile and policy for the current step.
- Maintenance window or rollout timing considered.
- Blast radius understood.
- Rollback/containment plan named explicitly.
- Old/new application compatibility checked.
- Unknowns surfaced instead of hand-waved.
- Destructive impact summarized before execution.

## Post-Change Verification

- Re-run schema inspection if DDL was involved.
- Re-run row count / sample queries.
- Compare expected vs actual affected rows.
- Re-run `EXPLAIN` for critical queries if indexes or predicates changed.
- Confirm whether more steps remain: backfill, cleanup, app deploy, monitoring.
