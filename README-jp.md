# PostgreSQL Semantic Gateway

[English](README.md)

`postgresem`は、AIエージェントとアプリケーションのためのPostgreSQLネイティブな
セマンティックゲートウェイです。厳格かつバージョン化されたLogical Semantic Query
（LSQ）を受け取り、immutableな公開済みSemantic Revisionに対して解決し、保護された
PostgreSQL境界を通じて、決定的に生成されたパラメータ化`SELECT`クエリを実行します。

最新の公開リリースは**0.3.0-beta.1**です。ローカル評価および統制された読み取り専用
pilotに適しており、本番環境への導入を意図したものではありません。

## postgresemはどのような問題を解決するのか？

PostgreSQLデータベースは、schema、table、column、型、key、foreign key、制約、
comment、権限、Row-Level Security policyなど、データについて多くの情報をすでに
保持しています。一方で、その構造が業務上何を意味するかまでは、通常は完全に表現
されていません。アプリケーションやAIエージェントには、どのtableが注文を表すのか、
どのtimestampが売上計上時点なのか、どのjoinが安全なのか、承認されたmetric定義は
どれか、特定の利用者がどのfieldを発見・queryできるか、といった情報も必要です。

統制されたsemantic layerがない場合、これらの情報はapplication code、BI tool、
prompt、documentation、YAML fileなどに重複して記述されがちです。それぞれの複製は
databaseや相互の定義から独立してdriftします。物理schema metadataしか参照できない
AIエージェントは、構文的には正しくても、誤ったgrainを使う、join fan-outを起こす、
一貫しないmetric定義を適用する、意図されたaccess pathを外れる、といったSQLを生成
する可能性があります。

`postgresem`は、不足しているsemantic contractをデータとともにPostgreSQL内へ保持
します。`pg_catalog`、`COMMENT`、PK/UNIQUE/FK、`CHECK`、GRANT、RLSといった
database-nativeなevidenceを、人が明示的にreviewしたmodel、field、relationship、
metric、term、policy bindingと組み合わせます。これらの定義はimmutableなrevision
として公開されます。エージェントはraw SQLを送信する代わりに、承認済みsemantic
nameをLSQでqueryします。決定的compilerは、上限付きのパラメータ化`SELECT`を生成
するか、曖昧または未対応のrequestを拒否します。

このcontractをPostgreSQL内へ置くことで、次の実用的な利点が得られます。

- **統制された単一の正本:** 物理metadata、業務上の意味、権限、revision履歴を、
  独立したmetadata serviceとの間で同期することなく、同じdatabase運用境界で管理
  できます。
- **database securityが最終的な正本:** PostgreSQLのGRANTとRLSが実行時のaccessを
  引き続き強制します。semantic layerは公開・query可能な範囲を狭められますが、
  databaseが拒否するaccessを許可することはできません。
- **意味をdata modelとともに変更:** semantic migration、公開、backup、restore、
  drift checkを、それらが説明するschemaの変更と連携できます。
- **より安全なAI access:** エージェントは承認されたconceptを発見し、型付きLSQを
  組み立てます。無制限のSQL実行interfaceを受け取ることも、table名やcolumn名だけ
  から業務上の意味を推測することもありません。
- **構築時に確定するlineageとaudit:** 各resultを、その生成に使用したsemantic
  revision、metric、relationship、source column、policy context、compiler version、
  SQL hashへ結び付けられます。
- **PostgreSQL中心のsystemで追加infraを削減:** core contractに外部catalog、vector
  database、policy engineは不要です。PostgreSQLが、データと統制された意味の両方に
  対するdurableなsystem of recordであり続けます。

この方式は意図的にPostgreSQL専用です。多くのdatabase dialectを横断する広い抽象化
よりも、PostgreSQLの型、catalog metadata、role、RLS、transaction、backup手順との
深い統合を優先します。

## 1.0までのロードマップ

現在の`0.3` betaは統制されたread-only systemです。M6は`1.0`ではなく`0.4`として
releaseし、上限付きinsertと明示的にmodel化された冪等upsertのための独立した型付き
mutation contractを追加する計画です。既存の`READ ONLY` query executorを弱めず、raw
SQL、任意DML、物理identifier、request-selected database roleは公開しません。
PostgreSQLのGRANT、RLS `WITH CHECK`、constraint、triggerを最終正本として維持します。

`0.4`では、cross buildしたarchiveやmulti-architecture image manifestだけでなく、
Linux amd64/arm64上で実際にruntime testを実行することをrelease要件にします。Mac
StudioとApple Containerはmaintainerのlocal reference環境として残しますが、唯一の
support targetではありません。

`0.4`以降の`0.5`から`0.9`では、Wren AI、Cube、Malloy、MetricFlow等の現行reference
implementationと再現可能な比較を行い、不足するauthoring、semantic、integration、
operations機能を選びます。feature数のparityは目的とせず、`1.0`までPostgreSQLを唯一の
execution engineかつsemantic source of truthとして維持します。

M6〜M12のgateは
[implementation plan](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN-jp.md)
を参照してください。

## はじめに

- [30分で試すApple Container quickstart](docs/quickstart.md)
- [Commerce sampleとstdio smoke client](examples/commerce/README.md)
- [ローカルCommerce Web demo](examples/web_demo/README.md)
- [運用ガイド](docs/operations.md)
- [エラーリファレンス](docs/error-reference.md)
- [互換性policyとsupport matrix](docs/compatibility.md)
- [Performance baselineと再現手順](docs/performance.md)
- [Developer preview exit checklist](docs/developer-preview-checklist.md)
- [M5 beta checklist](docs/beta-checklist.md)
- [M5 external evidence収集手順](docs/m5-external-evidence.md)
- [Backupとrestore](docs/backup-restore.md)
- [SLOとadoption reporting](docs/slo-and-adoption.md)
- [Incident runbook](docs/incident-runbook.md)
- [Beta security review checklist](docs/security-review-checklist.md)
- [M4 design feedback form](https://github.com/rioriost/postgresem/issues/new?template=m4_design_feedback.yml)
- [設定済みCI](.github/workflows/ci.yml)と
  [release automation](.github/workflows/release.yml)
- [Architecture Decision Record](docs/adr/)
- [Implementation plan](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN-jp.md)

## Betaで実装されているもの

- LSQ v1 validationと決定的compile
- PostgreSQLをbacking storeとするSemantic Snapshot/Schema v1
- canonical hashを持つimmutableな公開済みrevision
- row数とbyte数に上限を設けた保護された読み取り専用実行
- PostgreSQLのGRANTとRLSを強制する固定role mapping
- 必須query audit lifecycle record
- 改行区切りJSON-RPC stdio上のMCP `2024-11-05`
- semantic操作に限定した5つのtoolと3形式のresource URI
- breaking-change gateを持つ決定的Semantic Model互換性diff
- 100 modelのcompiler baselineと決定的な100 relation catalog check
- PostgreSQL 18を使用するローカルApple Container Compose開発stack

MCP toolは`list_semantic_models`、`describe_semantic_model`、
`validate_semantic_query`、`query_semantic_model`、
`explain_semantic_query`です。raw SQLまたはcompiler outputを返すMCP toolはなく、
MCP responseは生成SQLや物理lineageを公開しません。

## Security境界

runtimeおよびaudit credential、project、mapped database role、principal、execution
profileはprocess起動時のenvironmentで固定され、requestから上書きできません。実行には
durableな`started` audit rowが必要です。その後、`SET LOCAL ROLE`とtransaction-local
timeoutを設定した`READ ONLY` transactionを使用します。executorは、必要なrole
membershipを持たないrole、superuser、`BYPASSRLS` role、queryが使用するsource
relationのowner roleを拒否します。

Apple Containerでは、`/etc/hosts` fallbackのためにGatewayのCompose設定userをrootに
する必要があります。startup commandはidle processを直ちに`postgresem`へ降格し、
`make mcp`もMCPを明示的に`postgresem`としてexecします。container設定自体はnonroot
ではありませんが、application processは非特権です。

MCP diagnosticはstructured JSONとしてstderrへ出力され、request value、connection
data、SQL、result row、private name、principal dataを含みません。hiddenなsemantic
objectと未知のsemantic objectには、同じ公開用「not available」errorを返します。

## Betaの制限事項

- PostgreSQL connectionには明示的な`sslmode`が必要です。remote connectionでは
  `sslmode=require`を使用してください。`sslmode=disable`は、ローカルまたは別途
  保護されたconnectionとして明示的に選択した場合だけ受け入れます。
- MCPはstdio専用です。HTTP listenerやremote authentication layerはありません。
- MCPのconcurrent cancellationは未実装です。PostgreSQL statement timeoutが現在の
  cancellation境界です。
- N-1および同名restore pathはfixtureでtestされていますが、本番backup、RPO/RTO、
  disaster recovery、down migrationはoperatorの責任です。
- `v0.3.0-beta.1`のchecksumとimmutable container image digestは、GitHub release
  workflowによりkeyless署名されています。
- PostgreSQL 18が検証済みのローカル開発targetです。PostgreSQL 16、17、18はDocker
  CIのmigration、integration、recovery matrixを通過しています。正確な境界は
  [compatibility matrix](docs/compatibility.md)を参照してください。
- 現在もLinux amd64 CIとmulti-architecture release artifactはありますが、M6でLinux
  amd64/arm64両方の実行evidenceをrelease-blockingにします。
- governed writeは`0.4`で計画しており、現在のreleaseはmutation requestを受け付けません。

## Packaging状況

tagをtriggerとするautomationは、4種類のnative archiveをbuildし、`SHA256SUMS`を生成
して、image SBOMおよびprovenance付きのmulti-architecture GHCR imageを公開します。
[`scripts/install.sh`](scripts/install.sh)はCosignを必須とし、署名済み
`SHA256SUMS`を正確なrelease workflow/tag identityに対して認証してから、対応する
archiveのchecksumを検証します。

[`v0.3.0-beta.1` pre-release](https://github.com/rioriost/postgresem/releases/tag/v0.3.0-beta.1)
には、amd64およびarm64向けのLinux/macOS archive、`SHA256SUMS`、Sigstore signature、
certificateが含まれます。公開imageは
`ghcr.io/rioriost/postgresem:0.3.0-beta.1`です。checksumとimmutable image digestは
GitHub OIDCでkeyless署名されています。検証時には、想定するworkflow identityと
issuerを制約する必要があります。
[artifact matrix](docs/compatibility.md#artifact-release-and-runtime-matrix)も参照して
ください。

## 開発

```sh
make doctor
make test
make check
```

preview全体のgateは次のcommandで実行します。

```sh
make preview-check
```

M4のcompatibilityおよびperformance surfaceは、次のcommandで直接実行できます。

```sh
postgresem model diff --from BEFORE.json --to AFTER.json --fail-on-breaking
postgresem benchmark compiler \
  --models 100 --warmup 100 --iterations 1000 --threshold-ms 50
make test-performance
```

どちらのCLI commandもstructured JSONを出力します。benchmarkはp95がthresholdを
厳密に下回らない場合にnonzeroで終了します。model diffは
`--fail-on-breaking`が指定され、breaking diffがある場合にだけnonzeroで終了します。
対象範囲とreference measurementは
[performance.md](docs/performance.md)を参照してください。

変更またはreportを提出する前に、[CONTRIBUTING.md](CONTRIBUTING.md)と
[SECURITY.md](SECURITY.md)を確認してください。
