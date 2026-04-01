# dbx 中文文档

`dbx` 是一个使用 Rust 编写的数据库命令行工具，直接连接 MySQL、PostgreSQL 和 SQLite。它保持 CLI 本身尽量简单，同时把两个安全控制点明确拆开：

- 写操作确认：会修改数据或结构的 SQL 仍然必须显式传入 `--write`
- 权限策略：每个 profile 都可以单独允许或拒绝某些操作类型

这样可以避免把“只允许 DDL”这类语义含糊的说法直接映射成危险行为。

返回英文文档：[`README.md`](../README.md)

## 功能特性

- 基于 Rust，使用 `clap`、`serde`、`toml` 和 `sqlx`
- 通过外部 TOML 文件管理 profile
- 支持 MySQL、PostgreSQL、SQLite
- 提供 `query`、`exec`、`tables`、`schema`（`desc` 别名）、`explain`
- 输出格式支持 `table` 和 `json`
- 显式权限模型：`read`、`dml_write`、`schema_inspect`、`schema_change`、`explain`
- 内置更保守的策略，适合只读或生产场景
- 对修改性语句保持保守确认，必须显式传入 `--write`

## 构建

```bash
cargo build
```

## CI 与发布

GitHub Actions 负责校验和发布打包：

- `CI` 会在分支 push 和 pull request 时运行
- 它会检查 `cargo fmt --check`、`cargo test --locked` 和 `cargo build --locked --release --bin dbx`
- `Release` 只会在推送符合 `v*` 模式的 tag 时运行
- 发布包会在 `ubuntu-latest`、`macos-latest` 和 `windows-latest` 原生构建
- 发布产物名称会包含 runner 的操作系统标签和架构
- Linux 和 macOS 产物使用 `.tar.gz`
- Windows 产物使用 `.zip`，并包含 `dbx.exe`

发布包包含：

- 编译后的 `dbx` 可执行文件（Windows 下为 `dbx.exe`）
- `README.md`
- `LICENSE`

### 发布规则

打包的预期规则是：只有从 `main` 创建的 tag 才应该触发正式发布。

GitHub Actions 不能直接从事件载荷里判断 “这个 tag 是否从 `main` 创建”，所以当前工作流实现的是一个务实的保护：

- 拉取 `origin/main`
- 检查被打 tag 的提交是否包含在 `origin/main` 中
- 如果不包含，工作流会在构建和发布产物之前停止

这意味着，如果 tag 指向 feature branch 上的提交，发布工作流会失败。也意味着如果 tag 指向一个仍然可从 `main` 到达的旧提交，它依然会通过检查；这是当前规则下有意接受的近似实现。

### 维护者发布步骤

1. 先把准备发布的提交合并到 `main`。
2. 在该 `main` 提交上创建一个带注释的 tag，例如 `v0.1.0`。
3. 把 tag 推送到 GitHub。
4. 等待 `Release` 工作流构建并发布 Linux、macOS 和 Windows 的压缩包。

## 配置文件

默认情况下，`dbx` 会按下面顺序查找配置文件：

1. `--config /path/to/config.toml`
2. `./dbx.toml`
3. `~/.dbx/config.toml`
4. `~/.config/dbx/config.toml`

这意味着你既可以使用更直接的 `~/.dbx/config.toml`，也可以继续使用 XDG 风格的 `~/.config/dbx/config.toml`。
如果某个默认位置不存在，`dbx` 会继续按顺序查找下一个存在的配置文件。

完整示例见 [`examples/dbx.example.toml`](../examples/dbx.example.toml)。

最小配置示例：

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

带策略的示例：

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

本仓库不会集成 secrets manager，配置中的数据库凭据需要由使用者自己写入并自行管理。

## 内置策略

`dbx` 内置了这些策略：

- `all`：允许 `read`、`dml_write`、`schema_inspect`、`schema_change`、`explain`
- `readonly`：允许 `read`、`schema_inspect`、`explain`
- `prod_safe`：允许 `read`、`schema_inspect`、`explain`
- `migration_only`：只允许 `schema_change`

如果 profile 没有写 `policy`，默认使用 `all`。

自定义策略放在 `[policies.<name>]` 下，使用 `allow = [...]` 指定允许的操作。

## 使用方式

使用默认 profile：

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

显式选择 profile：

```bash
dbx --profile local_mysql query --sql "select database() as current_db"
dbx --profile local_sqlite tables
```

即使策略允许，修改性语句仍然必须显式确认：

```bash
dbx exec --sql "update users set active = false"
# error: `exec` requires --write for dml_write statements

dbx exec --write --sql "update users set active = false"
```

策略会在执行前先检查：

```bash
dbx --profile prod exec --write --sql "delete from users where id = 42"
# error: policy `prod_safe` does not allow dml_write operations for `exec`

dbx --profile prod exec --write --sql "alter table users add column note text"
# error: policy `prod_safe` does not allow schema_change operations for `exec`
```

## 权限模型

`dbx` 会把 SQL 归类到明确的操作类型中：

- `read`：不修改数据的读取语句，例如 `SELECT`、`VALUES`
- `dml_write`：会改动行数据的语句，例如 `INSERT`、`UPDATE`、`DELETE`、`MERGE`、`REPLACE`
- `schema_inspect`：元数据或结构查看语句，例如 `SHOW`、`DESCRIBE`、`DESC`
- `schema_change`：结构修改或偏管理类语句，例如 `CREATE`、`ALTER`、`DROP`、`TRUNCATE`、`RENAME`
- `explain`：`EXPLAIN ...`

命令和权限的关系是显式的：

- `tables` 和 `schema` 需要 `schema_inspect`
- `explain` 需要 `explain`
- `query` 和 `exec` 会先对 SQL 分类，再应用策略检查

## 生产环境语义

“线上库只能执行 ddl，不可执行修改表等操作” 这种说法本身是矛盾的，因为 DDL 通常就包含 `CREATE TABLE`、`ALTER TABLE` 这类改表操作。

`dbx` 的处理方式是直接使用明确的操作类型，而不是依赖模糊的 “DDL” 说法。

更稳妥的生产环境建议：

- `readonly`：`read + schema_inspect + explain`
- `prod_safe`：`read + schema_inspect + explain`
- `migration_only`：只允许 `schema_change`

如果团队真正需要“只允许迁移”，应该配置成 `migration_only`，而不是描述成“允许 DDL 但不允许改表”。

## 分类规则

`dbx` 没有实现完整 SQL 解析器，当前分类规则是启发式的：

- 先去掉注释和字符串字面量，再检查 token
- 大多数情况下使用首个有效关键字决定分类
- 对 `WITH ...` 语句会继续扫描内部是否包含修改性关键字

目标是保守、可测试，而不是覆盖所有方言细节。无法识别的前导关键字会被拒绝，而不是猜测执行。

## 命令说明

- `query`：执行查询并把结果渲染成表格或 JSON
- `exec`：执行语句并返回影响行数
- `tables`：列出当前 schema 或命名空间下的表和视图
- `schema`：查看表结构，`desc` 是它的别名
- `explain`：执行 `EXPLAIN` 并输出计划结果

## 当前限制

- SQL 文件会被当作原始语句处理；跨驱动的多语句执行当前不保证一致
- 空结果集在表格输出里暂时不会保留列元数据
- SQL 分类是启发式的，不能完整解析所有厂商方言
- 一些厂商特有命令如果前导关键字不在支持列表中，可能会被直接拒绝
