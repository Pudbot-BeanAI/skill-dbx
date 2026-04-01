# dbx Usage

This skill assumes the local `dbx` CLI semantics from the `sql/dbx` project.

## Built-In Policies

- `readonly`: `read`, `schema_inspect`, `explain`
- `prod_safe`: `read`, `schema_inspect`, `explain`
- `migration_only`: `schema_change`
- `all`: `read`, `dml_write`, `schema_inspect`, `schema_change`, `explain`

Policy choice is separate from write confirmation. Mutating SQL still needs `--write` when `dbx` classifies it as mutating.

## When To Use Which Policy

- Use `readonly` for normal inspection and troubleshooting.
- Use `prod_safe` for inspection in production-like environments when you want the intent to be explicit.
- Use `migration_only` for reviewed schema execution steps only.
- Use `all` or a custom write policy only for reviewed DML tasks that genuinely need writes.

Typical pattern:

1. Inspect with `readonly` or `prod_safe`
2. Review risks and compatibility
3. Execute approved schema change with `migration_only`, or approved DML with a write-capable profile plus `--write`
4. Verify with `readonly` or `prod_safe`

## Command Patterns

Inspection:

```bash
dbx --profile prod_safe_pg tables
dbx --profile prod_safe_pg schema users
dbx --profile prod_safe_pg query --sql "select count(*) from users where deleted_at is null"
dbx --profile prod_safe_pg explain --sql "select * from users where email = 'x@example.com'"
```

Reviewed DML:

```bash
dbx --profile ops_write query --sql "select id, status from orders where status = 'queued' limit 20"
dbx --profile ops_write exec --write --sql "update orders set status = 'ready' where status = 'queued' and id between 1000 and 1999"
```

Reviewed schema change:

```bash
dbx --profile migration_pg exec --write --sql "alter table users add column last_seen_at timestamptz null"
```

## Notes

- `tables` and `schema` require `schema_inspect`.
- `explain` requires `explain`.
- `query` and `exec` classify SQL heuristically; ambiguous vendor-specific statements deserve manual review.
- `migration_only` allows schema change only. It is not an inspection profile.
