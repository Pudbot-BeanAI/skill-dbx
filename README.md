# dbx

`dbx` is a Rust CLI for direct database operations against MySQL, PostgreSQL, and SQLite. It keeps the CLI small, but now separates two safety concerns clearly:

- write confirmation: mutating SQL still requires `--write`
- permission policy: each profile can allow or deny explicit operation classes

That makes production behavior unambiguous even when shorthand phrases like "only allow DDL" are internally contradictory.

## Features

- Rust CLI built with `clap`, `serde`, `toml`, and `sqlx`
- Profile-based config loaded from an external TOML file
- Drivers for MySQL, PostgreSQL, and SQLite
- Commands: `query`, `exec`, `tables`, `schema` (`desc` alias), and `explain`
- Output formats: `table` and `json`
- Explicit policy model: `read`, `dml_write`, `schema_inspect`, `schema_change`, `explain`
- Built-in restrictive policies for readonly and production-like profiles
- Conservative mutating-statement confirmation via `--write`

## Build

```bash
cargo build
```

## CI And Releases

GitHub Actions handles both validation and release packaging:

- `CI` runs on branch pushes and pull requests
- it checks `cargo fmt --check`, `cargo test --locked`, and `cargo build --locked --release --bin dbx`
- `Release` runs only when a tag matching `v*` is pushed
- release packaging is built natively on `ubuntu-latest`, `macos-latest`, and `windows-latest`
- release artifact names include the runner OS label plus architecture
- Linux and macOS artifacts are published as `.tar.gz`
- Windows artifacts are published as `.zip` and include `dbx.exe`

Release packages include:

- the compiled `dbx` binary (`dbx.exe` on Windows)
- `README.md`
- `LICENSE`

### Release Rule

Packaging is intended to happen only for tags created from `main`.

GitHub Actions cannot infer "tag was created from main" directly from the event payload, so the release workflow enforces a practical safeguard instead:

- it fetches `origin/main`
- it verifies the tagged commit is contained in `origin/main`
- if that check fails, the workflow stops before any release artifacts are built or published

That means a tag pushed from a feature branch commit will fail the release workflow. It also means an older commit that is still reachable from `main` will pass the check, which is an intentional approximation of the policy.

### Maintainer Release Steps

1. Merge the intended release commit into `main`.
2. Create an annotated tag such as `v0.1.0` on that `main` commit.
3. Push the tag to GitHub.
4. Wait for the `Release` workflow to build and publish the Linux, macOS, and Windows archives.

## Config

By default, `dbx` looks for config in this order:

1. `--config /path/to/config.toml`
2. `./dbx.toml`
3. `~/.config/dbx/config.toml`

See [`examples/dbx.example.toml`](/root/go/src/github.com/sql/dbx/examples/dbx.example.toml) for a fuller example.

Minimal profile example:

```toml
default_profile = "local_pg"

[output]
default_format = "table"

[profiles.local_pg]
kind = "postgres"
url = "postgres://app:secret@127.0.0.1:5432/appdb"

[profiles.local_mysql]
kind = "mysql"
url = "mysql://app:secret@127.0.0.1:3306/appdb"

[profiles.local_sqlite]
kind = "sqlite"
url = "sqlite://./dev.db"
```

Example with policies:

```toml
default_profile = "prod"

[policies.reporting]
allow = ["read", "schema_inspect", "explain"]

[profiles.local_dev]
kind = "postgres"
url = "postgres://app:secret@127.0.0.1:5432/appdb"
policy = "all"

[profiles.prod]
kind = "postgres"
url = "postgres://readonly@10.0.0.12:5432/appdb"
policy = "prod_safe"

[profiles.reporting]
kind = "mysql"
url = "mysql://reporter:secret@127.0.0.1:3306/appdb"
policy = "reporting"
```

Credentials are expected to be written manually by the user in the config file. There is no secrets manager integration in this repository.

## Built-In Policies

`dbx` ships with these built-in policies:

- `all`: allows `read`, `dml_write`, `schema_inspect`, `schema_change`, `explain`
- `readonly`: allows `read`, `schema_inspect`, `explain`
- `prod_safe`: allows `read`, `schema_inspect`, `explain`
- `migration_only`: allows `schema_change`

Profiles default to `all` when `policy` is omitted.

Config-defined policies live under `[policies.<name>]` and use `allow = [...]`.

## Usage

Run against the default profile:

```bash
dbx query --sql "select now() as current_time"
dbx query --file ./queries/report.sql
dbx tables
dbx tables --schema public
dbx schema users
dbx desc users
dbx explain --sql "select * from users where id = 42"
dbx --format json query --sql "select 1 as ok"
```

Choose a profile explicitly:

```bash
dbx --profile local_mysql query --sql "select database() as current_db"
dbx --profile local_sqlite tables
```

Mutating statements still require explicit confirmation even when the profile policy allows them:

```bash
dbx exec --sql "update users set active = false"
# error: `exec` requires --write for dml_write statements

dbx exec --write --sql "update users set active = false"
```

Policies are enforced before execution:

```bash
dbx --profile prod exec --write --sql "delete from users where id = 42"
# error: policy `prod_safe` does not allow dml_write operations for `exec`

dbx --profile prod exec --write --sql "alter table users add column note text"
# error: policy `prod_safe` does not allow schema_change operations for `exec`
```

## Permission Model

`dbx` classifies statements into explicit operation classes:

- `read`: non-mutating row reads such as `SELECT` and `VALUES`
- `dml_write`: row-changing statements such as `INSERT`, `UPDATE`, `DELETE`, `MERGE`, `REPLACE`
- `schema_inspect`: metadata/introspection statements such as `SHOW`, `DESCRIBE`, `DESC`
- `schema_change`: schema-changing or admin-like statements such as `CREATE`, `ALTER`, `DROP`, `TRUNCATE`, `RENAME`
- `explain`: `EXPLAIN ...`

Command-to-policy behavior is explicit:

- `tables` and `schema` require `schema_inspect`
- `explain` requires `explain`
- `query` and `exec` classify the provided SQL and then apply policy checks

## Production Semantics

The original instruction "线上库只能执行ddl，不可执行修改表等操作" is contradictory because DDL normally includes table-changing statements such as `CREATE TABLE` and `ALTER TABLE`.

`dbx` resolves that ambiguity by documenting and implementing explicit operation classes instead of relying on the phrase "DDL" alone.

Safe production recommendations:

- `readonly`: `read + schema_inspect + explain`
- `prod_safe`: `read + schema_inspect + explain`
- `migration_only`: `schema_change` only

If a team truly wants "migration-only" access, that should be configured as `migration_only`, not described as "DDL except schema changes".

## Classification Heuristic

`dbx` does not implement a full SQL parser. Statement classification is heuristic:

- comments and quoted literals are stripped before token inspection
- the leading significant keyword decides most classifications
- `WITH ...` statements are scanned for the contained mutating keyword

This is designed to be conservative and testable, not dialect-complete. Statements with unsupported leading keywords are rejected instead of guessed.

## Command Notes

- `query`: fetches rows and renders them as a table or JSON
- `exec`: executes a statement and reports affected rows
- `tables`: lists tables/views for the current schema or namespace
- `schema`: describes a table. `desc` is an alias
- `explain`: runs `EXPLAIN` for MySQL/PostgreSQL and `EXPLAIN QUERY PLAN` for SQLite

## Current Limitations

- SQL files are treated as raw statements; multi-statement execution is not guaranteed across drivers
- Empty result sets do not currently preserve column metadata in table output
- Classification is heuristic and does not fully parse every vendor-specific statement form
- Some vendor-specific commands may be rejected if their leading keyword is not supported by the classifier
