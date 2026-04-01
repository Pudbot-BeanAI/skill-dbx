# dbx DBA 技能包

这个仓库现在首先分发的是 `dbx-dba`：一个面向 DBA 工作流的技能包，用于安全地做数据库检查、查询评审、变更评审和受控执行。Rust 实现的 `dbx` CLI 仍然保留在仓库里，但它现在更像技能包内部使用的执行引擎，而不是仓库对外的主产品。

正式发布产物是按平台区分的 `.skill` 包；每个包里都会内置目标平台对应的原生 `dbx` 二进制。这样安装技能时不需要再额外依赖宿主机单独安装 CLI。

返回英文文档：[`README.md`](../README.md)

## 仓库实际分发什么

- 一个可分发技能根目录：[`skill/`](../skill)
- 技能内容包括 `SKILL.md`、`references/`、`agents/` 和 `assets/`
- 一个被技能调用的 Rust `dbx` CLI，用于 MySQL、PostgreSQL 和 SQLite
- 按平台区分的发布产物：
  - `dbx-dba-linux-amd64.skill`
  - `dbx-dba-macos-amd64.skill`
  - `dbx-dba-windows-amd64.skill`
- 技能内置 CLI 继续使用明确的安全模型：
  - 修改性 SQL 仍然必须显式传入 `--write`
  - 每个 profile 都可以单独允许或拒绝某些操作类型

现在更合适的理解方式是：先选择并安装目标平台对应的 `dbx-dba` 技能包，再通过技能调用其中内置的 `dbx`。直接把仓库理解成“一个裸 CLI 项目”已经不是主要定位。

## 技能包的安装与使用

1. 下载与技能实际运行平台匹配的 `.skill` 发布产物。
2. 在支持技能包的工具或运行环境中安装/导入这个 `.skill`。
3. 运行时应当相对于“已安装技能根目录”解析内置的 `dbx`，而不是相对于当前项目目录。
4. 实际使用时优先调用安装好的 `dbx-dba` 技能，让它去执行内置的二进制，而不是假设宿主机已经单独安装了 `dbx`。

安装后的技能包内部，内置二进制路径约定为：

- Linux 和 macOS：`assets/bin/dbx`
- Windows：`assets/bin/dbx.exe`

如果安装了错误平台的 `.skill`，`assets/bin/` 下的可执行文件格式也会不匹配；如果这个二进制不存在，说明技能包构建本身有问题。

## 技能打包与发布

仓库只有一个规范的技能根目录：[`skill/`](../skill)。发布时会先对这个目录做 staging，再把当前平台编译出的 `dbx` 二进制注入到 `assets/bin/`，最后产出对应平台的 `.skill` 文件。归档根目录就是技能本身，不会再额外套一层 `dbx-dba/`。

## 构建

```bash
cargo build
```

## CI 与发布

GitHub Actions 负责校验和发布打包：

- `CI` 会在分支 push 和 pull request 时运行
- 它会检查 `cargo fmt --check`、`cargo test --locked`、`cargo build --locked --release --bin dbx`，以及 `.skill` 打包冒烟验证
- `Release` 只会在推送符合 `v*` 模式的 tag 时运行
- 发布包会在 `ubuntu-latest`、`macos-latest` 和 `windows-latest` 原生构建
- 发布产物是明确的平台技能包：
  - `dbx-dba-linux-amd64.skill`
  - `dbx-dba-macos-amd64.skill`
  - `dbx-dba-windows-amd64.skill`

`.skill` 打包由 [`scripts/package_skill.py`](../scripts/package_skill.py) 负责。该脚本只使用 Python 标准库，会先对 [`skill/`](../skill) 做临时 staging，再把当前平台编译出的二进制注入到 `assets/bin/`，最后生成确定性的 `.skill` zip 归档。

本地打包示例：

```bash
cargo build --locked --release --bin dbx
python scripts/package_skill.py \
  --binary target/release/dbx \
  --target-os linux \
  --target-arch amd64 \
  --output-dir dist
```

### 如何选择发布产物

请按技能实际运行的平台选择对应的 `.skill` 文件：

- Linux x86_64 / amd64: `dbx-dba-linux-amd64.skill`
- macOS Intel / amd64: `dbx-dba-macos-amd64.skill`
- Windows x86_64 / amd64: `dbx-dba-windows-amd64.skill`

之所以要区分平台，是因为每个 `.skill` 内都内置了对应平台的原生 `dbx` 二进制；如果选错，`assets/bin/` 里的可执行文件格式也会不匹配。

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
4. 等待 `Release` 工作流构建并发布 Linux、macOS 和 Windows 的 `.skill` 产物。

## CLI 补充说明

仓库里仍然保留了独立的 `dbx` CLI 源码。下面这些章节主要用于维护者、本地开发，或者需要理解技能包里内置二进制具体行为的使用者。

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
