# PostgreSQL Semantic Gateway

[English](README.md)

`postgresem`は、AIエージェントとアプリケーションのためのPostgreSQLネイティブな
セマンティックゲートウェイです。厳格かつバージョン化されたLogical Semantic Query
（LSQ）とLogical Semantic Mutation（LSM）を受け取り、immutableな公開済みSemantic
Revisionに対して解決し、分離されたquery/mutation PostgreSQL境界を通じて決定的な
パラメータ化operationを実行します。

現在のsource versionは**0.9.0 release candidate**、最新の公開releaseは
**0.7.0**です。M11ではcandidate contractをfreezeし、previous-binary rollback、
query/ingestion operator gate、support/governance/deprecation policyを追加しました。
production readinessやSLAを保証するものではありません。

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

現在のsourceはrepository管理下のM11 scopeを`1.0`ではなく`0.9`として実装しています。
M6の独立した型付き
mutation contractは、上限付きinsertと明示的にmodel化された冪等upsertに限定したまま
です。M7ではcatalog-boundなApache Ossie `0.1.1` candidate importと
authorization-awareなcatalog driftを追加しました。既存の`READ ONLY` query executor
を弱めず、raw SQL、任意DML、物理identifier、request-selected database roleは公開
しません。PostgreSQLのGRANT、RLS `WITH CHECK`、constraint、triggerを最終正本として
維持します。

`0.4`では、cross buildしたarchiveやmulti-architecture image manifestを実行evidence
とはせず、packaged binaryとruntime imageをLinux amd64/arm64上でnative実行するgateを
追加しました。Mac StudioとApple Containerはmaintainerのlocal reference環境として
残しますが、唯一のsupport targetではありません。

M7では、固定したWren AI、Cube、Malloy、MetricFlowのOSS runtimeを同一PostgreSQL 18
datasetに対して実行しました。全referenceが同じ期待aggregateを返すことを確認しつつ、
異なるtrust boundaryを明示しています。M8ではこのevidenceを基に、明示的なmetric
aggregation anchorと、要求されたaggregateを適用する前に宣言済みroot entity grainで
duplicate child rowを除去する二段階PostgreSQL planを追加しました。M9ではidentityと
authorizationをPostgreSQLから移動させず、stateless MCP `2026-07-28` HTTP resource
serverを追加しました。M10では計測されたcatalog N+1 bottleneckを解消し、
catalog-boundなlarge-model scaffoldとoperations/upgrade surfaceを追加しました。
guarded executionは計測上のbottleneckではなかったため、persisted accelerationは
deferしています。M11ではこれらのcontractをfreezeし、release-candidate operationと
rollback gateを追加しました。独立external security reviewと2件の28日間non-fixture
pilotは[issue #4](https://github.com/rioriost/postgresem/issues/4)で未完了です。
feature数のparityは目的とせず、`1.0`まで
PostgreSQLを唯一のexecution engineかつsemantic source of truthとして維持します。

M6〜M12のgateは
[implementation plan](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN-jp.md)
を参照してください。

## はじめに

- [30分で試すApple Container quickstart](docs/quickstart.md)
- [Linux Docker ComposeとPodman Quadlet](docs/linux-containers.md)
- [Commerce sampleとstdio smoke client](examples/commerce/README.md)
- [認証済みMCP HTTP deploymentとSDK guidance](docs/mcp-http.md)
- [ローカルCommerce Web demo](examples/web_demo/README.md)
- [運用ガイド](docs/operations.md)
- [M11 release-candidate checklist](docs/m11-release-candidate-checklist.md)
- [Release-candidate operator workflow](docs/rc-operator-workflow.md)
- [Support policy](SUPPORT.md)
- [Governance](GOVERNANCE.md)
- [Deprecation policy](docs/deprecation-policy.md)
- [エラーリファレンス](docs/error-reference.md)
- [互換性policyとsupport matrix](docs/compatibility.md)
- [M10 reference比較](docs/reference-comparison/2026-09-03.md)
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

## 0.9で実装されているもの

- LSQ v1 validationと決定的compile
- PostgreSQLをbacking storeとするSemantic Snapshot/Schema v2。Snapshot v1の読み込みと
  canonical hash互換性を維持
- 明示的なmetric additivityとroot entity-key aggregation anchor
- 承認済みdirect one-to-many dimension/filterに対する決定的な二段階aggregation。
  duplicate childとmulti-branch fan-outを保護
- canonical hashを持つimmutableな公開済みrevision
- row数とbyte数に上限を設けた保護された読み取り専用実行
- LSM v1 validationと、上限付きinsert/承認済みupsertの決定的compile
- queryとは分離したwriter credential、mapped writer role、mutation transaction、
  冪等replay、reconciliation
- PostgreSQLのGRANTとRLSを強制する固定role mapping
- 必須query/mutation audit lifecycle record
- 改行区切りJSON-RPC stdio上のMCP `2024-11-05`
- RFC 9728 metadataとlocal asymmetric JWT検証を備えた、loopback
  Streamable HTTP上の認証済みstateless MCP `2026-07-28`
- verified subjectからquery/writer roleへの完全一致mapping、authority単位のlimit、
  private discovery、request SSE、切断からPostgreSQLへのcancellation
- mutation有効時に8つのsemantic限定toolと4形式のresource URI
- breaking-change gateを持つ決定的Semantic Model互換性diff
- fingerprint付きPostgreSQL catalog drift。GRANT、RLS、role authorization、
  完全なrole graph evidence、security-definer view owner authority、
  正規化object ACL、function/window/aggregate実行evidence、
  relation ownership、constraint、型の変更をbreaking evidenceとして扱う
- PostgreSQL catalog evidenceと照合する、review可能かつquery-onlyなcandidateへの
  Apache Ossie `0.1.1`一方向import
- 同一PostgreSQL 18 taskに対するWren AI、Cube、Malloy、MetricFlowの固定runtime比較と
  machine-readable evidence
- 1,000 modelのcompiler、決定的な1,000 relation catalog scan、
  guarded execution result hashを含むM10 scale baseline
- 1,000 relationを1秒未満に保つregression gate付きset-based catalog scan
- 最大1,000のreview-only modelを生成する決定的catalog-bound scaffold
- fixedかつprivacy-preservingなM10 operational dashboard
- verified backupをgateとするlocal Apple Container upgrade automation
- 決定的なfrozen release-candidate contract inventory
- isolated same-name restore後のprevious-release binary実行
- guarded query、governed ingestion、replay、auditを結合したworkflow gate
- PostgreSQL 18を使用するローカルApple Container Compose開発stack
- Linux Docker Composeとrootless Podman Quadlet deployment path

MCP toolは`list_semantic_models`、`describe_semantic_model`、
`validate_semantic_query`、`query_semantic_model`、
`explain_semantic_query`に加え、mutation設定時の
`validate_semantic_mutation`、`mutate_semantic_model`、
`reconcile_semantic_mutation`です。raw SQLまたはcompiler outputを返すMCP toolはなく、
MCP responseは生成SQLや物理lineageを公開しません。

## Security境界

runtimeおよびaudit credential、project、mapped database role、principal、execution
profileはprocess起動時のenvironmentで固定され、requestから上書きできません。実行には
durableな`started` audit rowが必要です。その後、`SET LOCAL ROLE`とtransaction-local
timeoutを設定した`READ ONLY` transactionを使用します。executorは、必要なrole
membershipを持たないrole、superuser、`BYPASSRLS` role、queryが使用するsource
relationのowner roleを拒否します。

mutationは独立したlogin、mapped writer role、compiler、executor、idempotency store、
audit lifecycleを使用します。business DML、committed idempotency result、terminalな
committed audit stateは同一transactionで確定します。PostgreSQLのcolumn GRANT、RLS
`USING`/`WITH CHECK`、constraint、triggerが最終authorityです。

HTTP adapterはOAuth resource serverとしてのみ動作します。local read-only fileから
strict authority document、JWKS、principal HMAC keyを読み、検証済みJWT subjectを
事前設定済みroleへ完全一致でmappingし、同居HTTPS reverse proxyの背後でloopbackだけ
にbindします。token発行、remote key fetch、forwarded identity headerの信頼、
request-selected roleは行いません。remote mutationはoperator gate、verified scope、
mapped writer role、既存PostgreSQL mutation境界がすべて有効な場合だけ公開されます。

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
- 認証済みHTTP listenerはTLSを終端せず、非loopback addressへbindできません。同居
  HTTPS reverse proxyでpublic Hostを保持し、SSE bufferingを無効化し、切断を伝播する
  必要があります。
- HTTP authority/JWKS reload、runtime OIDC discovery、distributed rate-limit state、
  resumable session、GET event stream、connection poolingは未実装です。
- N-1および同名restore pathはfixtureでtestされていますが、本番backup、RPO/RTO、
  disaster recovery、down migrationはoperatorの責任です。
- M10 operational reportはmaterialized view stateを観測しますが、materialized
  view/pre-aggregationの作成、refresh、query routingは行いません。
- `v0.7.0`のchecksumとimmutable container image digestは、GitHub release
  workflowによりkeyless署名されています。
- PostgreSQL 18が検証済みのローカル開発targetです。PostgreSQL 16、17、18はDocker
  CIのmigration、integration、recovery matrixを通過しています。正確な境界は
  [compatibility matrix](docs/compatibility.md)を参照してください。
- native Linux amd64/arm64 CI gateはruntime imageをPostgreSQL 18に対して実行します。
  tagged releaseではpackaged binaryとarchitecture別imageも公開前にgateします。
- governed writeは公開済みinsert/upsert projectionに限定されます。update、delete、
  merge、copy、call、DDL、raw SQL、caller-selected conflict target/returning fieldは
  未対応です。
- fan-out-safe aggregationは、単一root model、direct one-to-many relationship、
  共通のroot entity-key anchor、root-localなmetric input/filterに限定されます。
  group間のfact allocation、multi-fact、bridge、reverse、multi-hop planningは未対応です。
- Ossie importは意図的に一方向で、direct ANSI field、single-column key-backed
  relationship、承認済みsingle-field aggregateだけを扱います。未対応またはlossyな
  semanticsはfail closedします。

## Packaging状況

tagをtriggerとするautomationは、4種類のnative archiveをbuildし、`SHA256SUMS`を生成
して、image SBOMおよびprovenance付きのmulti-architecture GHCR imageを公開します。
[`scripts/install.sh`](scripts/install.sh)はCosignを必須とし、署名済み
`SHA256SUMS`を正確なrelease workflow/tag identityに対して認証してから、対応する
archiveのchecksumを検証します。

[`v0.7.0` release](https://github.com/rioriost/postgresem/releases/tag/v0.7.0)
には、amd64およびarm64向けのLinux/macOS archive、Linux binary/imageのnative
runtime evidence、`SHA256SUMS`、Sigstore signature、certificateが含まれます。
公開imageは`ghcr.io/rioriost/postgresem:0.7.0`です。checksumとimmutable image
digestはGitHub OIDCでkeyless署名されています。検証時には、想定するworkflow
identityとissuerを制約する必要があります。
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
