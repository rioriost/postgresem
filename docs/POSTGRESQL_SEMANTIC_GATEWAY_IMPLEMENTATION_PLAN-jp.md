# PostgreSQL Semantic Gateway 実装計画

- プロジェクト名: `postgresem` / PostgreSQL Semantic Gateway
- 文書ステータス: 継続更新する実装・リリース計画
- 作成日: 2026-08-31
- 最終改訂日: 2026-09-02
- 対象環境: 必須runtimeとしてLinux amd64/arm64、開発・native archiveとしてmacOS amd64/arm64、maintainer reference環境としてApple silicon Mac Studio + Apple Container、PostgreSQL 16–18
- 翻訳: [English](POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md)

## 1. エグゼクティブサマリー

PostgreSQL Semantic Gatewayは、PostgreSQL自身を業務データだけでなく「データの意味の正本（Semantic Source of Truth）」として扱い、AIエージェントやアプリケーションに、統制された意味・指標・関係・権限・来歴をMCP経由で公開するOSSを目指す。

中心となる価値はText-to-SQLではない。LLMにはSQLを書かせず、バージョン化されたLogical Semantic Query（LSQ）を組み立てさせる。GatewayはLSQを厳格に検証し、承認済みのSemantic ModelとPostgreSQLの権限を使って、決定的かつパラメータ化されたSQLへコンパイルする。

```text
AI Agent / Application
        │ MCP: discovery / validate / query / explain
        ▼
┌────────────────────────────────────────────────────┐
│ PostgreSQL Semantic Gateway（単一プロセス）       │
│ MCP Adapter → AuthN/AuthZ → Semantic Catalog       │
│ → LSQ Validator → Planner → SQL Compiler           │
│ → Guarded Executor → Lineage / Audit / Telemetry   │
└───────────────────────┬────────────────────────────┘
                        │ PostgreSQL protocol
                        ▼
┌────────────────────────────────────────────────────┐
│ PostgreSQL                                         │
│ business schemas │ semantic schema │ pg_catalog    │
│ COMMENT / FK / CHECK / GRANT / RLS                 │
└────────────────────────────────────────────────────┘
```

MVPはPostgreSQL専用、分析系の読み取り専用クエリ、単一Gateway、単一データベース、明示的に認証された利用者に限定する。多データソース、自然言語応答UI、キャッシュ、事前集計、pgvector、自由SQL、書き込み操作はMVPに含めない。

Beta後も対象databaseはPostgreSQLだけに保ちつつ、統制contractをデータ抽出だけでなく
投入へ拡張する。M6は`1.0`ではなく`0.4`とし、統制されたデータ投入用の独立した型付き
mutation contractと、Linux amd64/arm64の必須runtime evidenceを導入する。`0.4`以降は、
Wren AI、Cube、Malloy、MetricFlow等のreference implementationを各段階で再評価し、
PostgreSQL-nativeというpositionに適合する不足機能だけをevidenceに基づいて選び、
複数の互換性段階を経て`1.0`へ進む。

## 2. 目的と成功条件

### 2.1 目的

1. PostgreSQL内に、概念・ディメンション・メトリクス・関係・同義語・ポリシー参照・来歴を保持できる、明示的で移行可能なSemantic Schemaを提供する。
2. `pg_catalog`、`COMMENT`、PK/UNIQUE/FK、`CHECK`、GRANT、RLSから、有用な構造と意味の候補を安全に取り込む。
3. LSQ v1を、同一入力・同一Semantic Revision・同一Compiler Versionから同一の正規化SQLとパラメータ列へ変換する。
4. LLMに物理スキーマや自由SQLを直接操作させず、MCPで発見・検証・実行・説明を提供する。
5. すべての実行結果を、利用したSemantic Revision、メトリクス、ソース列、ポリシー、生成SQLハッシュまで追跡可能にする。
6. Apple Containerを使うMac Studio上の開発環境と、Linux amd64/arm64で検証されたproduction/runtime pathを提供する。
7. raw SQLや任意DMLを公開せず、PostgreSQLのGRANT、RLS `WITH CHECK`、constraint、trigger、transaction semanticsを最終正本として維持した統制データ投入を追加する。
8. PostgreSQL以外のdialectへ広げるのではなく、維持されているreference implementationとの比較から不足機能の優先順位を決める。

### 2.2 MVP成功条件

- 既存PostgreSQLのサンプルスキーマを取り込み、候補モデルを生成できる。
- 人がレビューして公開したモデルに対して、20件以上の代表LSQを正しいパラメータ化SQLへコンパイルできる。
- 主要な粒度・join fan-out・曖昧なjoin pathを検出し、誤答せず明示的に拒否できる。
- RLSを有効にしたテナント分離テストで、別テナントの行を取得できない。
- MCPクライアントがモデル発見、検証、実行、説明を完結でき、自由SQL入力口が存在しない。
- 全クエリについて、semantic revision、query hash、SQL hash、source lineage、policy context、実行時間、行数を監査できる。
- PostgreSQL 16、17、18のCIマトリクスを通過する。
- 初見の開発者がApple Container環境を30分以内に起動し、統合テストを実行できる。

### 2.3 0.4から1.0への方向

- `0.4`では、上限付き`insert`と明示的にmodel化された冪等`upsert`のために、
  versioned Logical Semantic Mutation（LSM）contractを追加する。callerからtable名、
  column名、conflict SQL、predicate、expression、stored procedure名は受け取らない。
- 書き込み可能model/fieldはquery visibilityとは独立して明示的にpublishする。
  server管理column、generated column、immutable field、許可conflict key、batch上限、
  返却可能fieldをpublished revisionへ含める。
- read/write credential、mapped role、transaction mode、audit record、rate limit、MCP
  capabilityを分離する。mutationを有効にしてもread-only query pathをwrite可能にしない。
- PostgreSQLのpermission、`WITH CHECK`を含むRLS、constraint、triggerを最終正本とする。
  Gatewayは許可mutationを狭められるが、databaseが拒否するmutationを成功させない。
- Linux amd64/arm64をrelease-blockingなruntime targetにする。cross compileまたは
  multi-architecture manifest生成だけではevidenceとせず、binary/imageを実際に起動し、
  architecture別smoke/contract testを通す。
- `0.5`から`0.9`は比較駆動の互換性段階とする。計測した利用者需要とreference
  implementationとの差から機能を選び、PostgreSQL専用かつno-raw-SQLの境界を保てる
  場合だけ採用する。

### 2.4 非目的

- 汎用Text-to-SQL製品、チャットUI、可視化ツール、BI製品を作ること。
- PostgreSQL以外のwarehouseやfederated queryに対応すること。
- dbt、ETL/ELT、データカタログ、MDMを置き換えること。
- 任意SQL、DDL、DML、ストアドプロシージャ実行をMCPへ公開すること。
- 統制mutation contractをbulk ETL/ELT、replication、CDC、database administrationの代替にすること。
- PostgreSQLのGRANT/RLSをGateway独自の認可で置き換えること。
- MVPでキャッシュ、事前集計、ベクトル検索、学習型join推論を実装すること。
- 複雑なmany-to-manyや非加法メトリクスを、意味が不明なまま自動補正すること。
- `1.0`以前または`1.0`でPostgreSQL以外のexecution engineへ対応すること。

## 3. 設計原則

1. **PostgreSQL-native**: 正式なSemantic Metadataは対象PostgreSQL内に置き、同じバックアップ、トランザクション、権限、移行手順で管理する。
2. **Database security is authoritative**: GatewayはGRANT/RLSを弱めない。実行ロールは非owner・非superuser・非`BYPASSRLS`とする。
3. **LLM proposes; deterministic code decides**: LLMはLSQ候補を作るだけで、解決、認可、計画、SQL生成、制限適用は決定的コードが担う。
4. **Fail closed**: 未知のフィールド、曖昧な関係、未承認のメトリクス、型不一致、過大コストは推測せず拒否する。
5. **Semantic contract is versioned**: Semantic ModelとLSQ Schemaをバージョン化し、実行時には公開済みrevisionを固定する。
6. **No raw SQL in the public contract**: 公開APIの式は型付きASTまたは承認済みシンボル参照とし、任意SQL文字列を受け付けない。
7. **Lineage by construction**: SQLを後から解析して推測するのではなく、名前解決と計画中にlineage edgeを構築する。
8. **Small modular monolith first**: MVPは単一バイナリと単一DBで始め、コンパイラの純粋ライブラリ境界だけを先に分離する。
9. **Explicit beats inferred**: catalogからの推論は候補であり、公開には人の承認を必要とする。
10. **Correctness before coverage**: サポート範囲を限定して正答または安全な拒否を保証し、曖昧な自動対応を増やさない。

## 4. スコープと主要ユースケース

### 4.1 MVPユースケース

- データエンジニアが既存DBをscanし、物理テーブル、列、コメント、制約、関係、RLSの候補を確認する。
- データオーナーが概念名、説明、公開列、メトリクス、許可join、粒度を登録してrevisionを公開する。
- AIエージェントがMCPで利用可能なモデルとフィールドを発見する。
- AIエージェントがLSQを検証し、読み取り専用クエリを実行する。
- 監査担当者が、結果がどの定義、列、関係、ポリシーに依存したか確認する。
- CIがSemantic Modelのmigration、golden SQL、既知回答、RLS境界、後方互換性を検証する。

### 4.2 MVPの意味論上の制約

- 1クエリのanchorは1つのfact modelとする。
- factからdimensionへのjoinは、原則many-to-oneまたはone-to-oneだけを許可する。
- many-to-many、bridge、複数factの同時集計、symmetric aggregateはv1では拒否する。
- メトリクスは`count`、`count_distinct`、`sum`、`min`、`max`、`avg`と、それらの安全な算術合成から始める。
- window function、任意subquery、user-defined functionはMVP対象外とする。
- dimension filterとmetric post-aggregate filter（HAVING相当）を区別する。
- 時間粒度は`day`、`week`、`month`、`quarter`、`year`。timezoneはmodelまたはrequestで許可済み値から選ぶ。

## 5. PostgreSQL-native Semantic Schema

### 5.1 配置と所有権

- 専用schema名は既定で`semantic`とし、設定で変更可能にする。
- schema ownerはmigration専用ロール`postgresem_owner`とする。
- Gateway接続ロール`postgresem_runtime`は`LOGIN NOINHERIT`を基本とし、直接の業務データ権限を持たせない。許可された`NOLOGIN`実行roleへtransaction内だけで切り替える。
- catalog scanは管理用`postgresem_introspector`、監査書き込みは`postgresem_auditor`へ分離する。通常queryのconnectionをread-writeにしない。
- モデル編集は`postgresem_editor`、公開は`postgresem_publisher`へ分離する。
- 業務テーブル所有者とGateway実行ロールを同一にしない。
- metadata tableにもRLSを適用し、利用者から見えない物理オブジェクトや説明をcatalog APIに出さない。

### 5.2 中核テーブル案

| テーブル | 役割 | MVP |
|---|---|---:|
| `semantic.project` | 対象DB内のSemantic Project | 必須 |
| `semantic.revision` | draft/published/retired状態、親revision、canonical hash | 必須 |
| `semantic.model` | business model、anchor relation、grain、公開状態 | 必須 |
| `semantic.field` | dimension/entity key/time dimension、型、物理列参照 | 必須 |
| `semantic.relationship` | join cardinality、列対応、許可方向、優先度 | 必須 |
| `semantic.metric` | aggregation、型付きexpression AST、filter、additivity | 必須 |
| `semantic.term` | 表示名、同義語、説明、locale | 必須 |
| `semantic.policy_binding` | DB role/RLS/semantic visibilityへの参照。RLS式の複製ではない | 必須 |
| `semantic.source_snapshot` | catalog scan時の物理object fingerprint | 必須 |
| `semantic.import_run` / `import_issue` | import履歴、候補、警告、drift | 必須 |
| `semantic.lineage_edge` | model/field/metric/source間の設計時lineage | 必須 |
| `semantic.query_audit` | 実行時lineage、hash、所要時間、結果サイズ | 必須 |
| `semantic.example_query` | 承認済みLSQと既知結果条件 | Phase 2 |
| `semantic.embedding` | term/exampleのembedding | pgvector導入後 |

### 5.3 モデルの識別と参照

- 公開APIでは連番IDではなく、revision内で一意な安定`semantic_name`とUUIDを使う。
- 物理参照は`database/schema/relation/column`の正規化名を正本とし、OIDはscan時の補助値に限定する。OIDはdump/restoreや再作成で変わり得るため永続IDにしない。
- identifierはGatewayがcatalogから取得した値だけを適切にquoteする。requestから受け取った文字列をSQL identifierへ直接挿入しない。
- draftの更新はimmutable revisionの新規作成として扱い、published revisionを上書きしない。
- `canonical_hash`は、順序を正規化したモデルJSON、schema version、compiler semantic versionから算出する。

### 5.4 Expression AST

`metric.expression`と計算fieldはversion付きJSONB ASTで保持する。SQL断片は保持しない。

```json
{
  "version": "1",
  "op": "aggregate",
  "function": "sum",
  "arg": { "op": "field_ref", "field": "order_amount" },
  "filter": {
    "op": "eq",
    "left": { "op": "field_ref", "field": "status" },
    "right": { "op": "literal", "type": "text", "value": "paid" }
  }
}
```

- JSONBの外形はDBの`CHECK`で最低限検証し、完全なJSON Schemaと型検証はGatewayおよびCIで行う。
- 使用可能な演算子・関数をallowlist化する。
- 関数のvolatility、入出力型、NULL意味論をcompiler側のregistryで固定する。
- ASTの新versionはmigrationとcompiler capabilityで明示し、未知versionは拒否する。

### 5.5 Semantic Schemaの整合性

- 同一revision内の名前一意性、参照整合性、状態遷移はPK/UNIQUE/FK/CHECKで保証する。
- `published`への遷移はGateway/CLIのtransaction内で全検証に合格した場合だけ許可する。
- `relationship.cardinality`、`metric.aggregation`、`revision.status`はenum相当のCHECKを使う。PostgreSQL enum型は追加変更が重いためMVPでは使わない。
- 任意のJSONBへ重要な意味を詰め込むEAV設計は避ける。JSONBはversioned AST、互換性のある補助属性、監査payloadに限定する。

## 6. `pg_catalog` / `COMMENT` / FK / CHECK / RLSの取り込み

### 6.1 取り込みパイプライン

```text
catalog scan (read-only, repeatable read)
  → normalize physical objects
  → fingerprint
  → infer candidates + confidence + evidence
  → compare with current published revision
  → import report / drift report
  → human review
  → new draft revision
  → validate and publish
```

scanは冪等とし、自動的にpublished modelを書き換えない。明示的な`postgresem catalog scan`と管理APIから、専用introspector credentialで起動し、MVPではevent triggerを要求しない。通常のGateway runtimeにscan権限を与えない。

### 6.2 読み取る主なcatalog

- relation/schema: `pg_class`, `pg_namespace`
- column/type/default: `pg_attribute`, `pg_type`, `pg_attrdef`
- key/constraint: `pg_constraint`, `pg_index`
- comments: `pg_description`、`obj_description`、`col_description`
- functions used by views/expressions: `pg_proc`、`pg_get_functiondef`は必要最小限
- views: `pg_rewrite`を直接解釈せず、`pg_get_viewdef`
- privilege: `has_schema_privilege`、`has_table_privilege`、`has_column_privilege`等
- RLS: `pg_policy`、`pg_class.relrowsecurity`、`relforcerowsecurity`、`pg_get_expr`
- dependencies/lineage補助: `pg_depend`、`pg_rewrite`

catalogへ直接更新は行わない。対応PostgreSQLバージョンごとにfixtureを持ち、公開されたcatalog列への依存差分をCIで検出する。

### 6.3 取り込みルール

| 入力 | 生成候補 | 扱い |
|---|---|---|
| table/view名 | model名・用語 | 命名規約で正規化、必ずレビュー |
| table/column `COMMENT` | description・business term | plain text。構造化DSLとして解釈しない |
| PK/UNIQUE | entity key・grain候補 | 高confidence。ただし業務粒度は要承認 |
| FK | relationship・join列・cardinality候補 | 高confidence。複合FKを順序保持して扱う |
| NOT NULL | nullable情報 | そのまま取り込む |
| CHECK | domain/value range/enum候補 | parserで安全に理解できる形だけhint化 |
| view definition | source lineage候補 | PostgreSQL parser/依存情報を利用。文字列正規表現でSQL解析しない |
| GRANT | model/field visibility候補 | 実行時のDB権限確認が正本 |
| RLS policy | policy存在・対象role・command・mode | 発見/説明用。Gateway独自式へ翻訳しない |

`COMMENT`は接続ユーザーから閲覧可能で、機密情報の保管場所ではない。Gatewayもコメントを監査ログへ無制限に複製せず、Semantic Catalogで公開許可された説明だけをMCPへ返す。

### 6.4 CHECKの扱い

- `col IN (...)`、range、単純比較、NULL条件など、parserが型付きASTへ安全に変換できるものだけ候補化する。
- 他行や外部関数に依存するCHECKを意味定義として信用しない。
- CHECKは「許容値のヒント」であり、メトリクス定義や認可条件には昇格させない。
- parser未対応の式はraw SQLをsemantic schemaにコピーせず、hashと人向け警告だけを記録する。

### 6.5 RLSとprincipal伝播

- Gatewayの認証済みprincipalから、事前登録された`NOLOGIN` DB roleまたは承認済みsession contextへの静的mappingを行う。request指定のrole名やGUC値は受け入れない。
- connection poolから取得後、transactionを開始し、`SET LOCAL ROLE <mapped_role>`および必要な`set_config(..., true)`を設定してからqueryを実行する。roleはcatalog由来allowlistからquoteし、任意文字列を連結しない。
- `postgresem_runtime`には、必要なmapped roleだけを`NOINHERIT`かつ`SET ROLE`可能なmembershipとして付与する。principal数がrole運用上限を超える場合は、固定role + RLS用session context方式をADR-005で選ぶ。
- 実行roleはtable owner、superuser、`BYPASSRLS`にしない。必要なtableでは`FORCE ROW LEVEL SECURITY`を推奨する。
- transaction終了時にcontextが確実に破棄されることをintegration testで検証する。
- RLS式をcompilerが再実装しない。DBでRLSを強制し、Gatewayはmodel/field visibilityとクエリ能力の縮小を追加する。
- referential integrityがRLSを迂回することや、RLS subqueryの競合による情報漏えいをsecurity review項目に含める。

## 7. Semantic Gateway構成

MVPはネットワーク分散しないモジュール化モノリスとする。

| モジュール | 責務 |
|---|---|
| MCP transport | stdio、後段でstreamable HTTP。protocol framingとpagination |
| Identity | token検証、principal→DB role mapping、request context |
| Catalog | 公開revisionのload/cache、権限でfilterした発見API |
| Importer | catalog scan、candidate生成、drift検出 |
| LSQ validator | JSON Schema、semantic name/type/capability検証 |
| Planner | anchor、join graph、grain、aggregate、policy binding、cost guard |
| Compiler | typed relational IRからPostgreSQL AST/SQLとbind parametersを生成 |
| Executor | read-only transaction、timeout、row/byte limit、cancel |
| Mutation compiler/executor | M6: typed insert/upsert plan、分離writer role、idempotency、rollback |
| Lineage/Audit | design-time/query-time/mutation-time edge、hash、監査event |
| Telemetry | structured logs、metrics、traces、health/readiness |
| Admin CLI | migrate、scan、validate、publish、diff、doctor |

### 7.1 技術スタック方針

- 第一候補はRustの単一workspaceとする。理由は、型付きIR、決定的compiler、低い配布依存、単一バイナリ、非同期PostgreSQL接続を一貫して実装しやすいため。
- Web/MCP transport、PostgreSQL driver、JSON Schema validator、SQL parser/AST rendererは、採用前にlicense、maintenance、PostgreSQL 16–18対応、fuzz実績をADRで評価する。
- SQL parserは入力SQLの許可に使わず、生成後の構文再parse、view lineage補助、golden testに使う。
- MCP SDKの成熟度に依存しすぎないよう、内部application serviceとMCP adapterを分離する。
- 別サービス化は負荷、独立release、権限境界の必要性が計測されてから判断する。

### 7.2 設定

- 設定優先順位はCLI flag > environment > TOML file > default。
- secretは環境変数または外部secret storeから受け取り、設定file、Semantic Schema、COMMENT、logへ保存しない。
- principal mapping、statement timeout、result上限、許可revision、監査保持期間を設定可能にする。
- 起動時にDB version、migration version、必須privilege、RLS安全条件を`doctor`相当で確認し、危険な実行roleならfailする。

### 7.3 統制Mutation境界（M6 target）

- queryとmutationのplanningはimmutable semantic snapshotとtyped scalar semanticsを
  共有するが、request type、compiler entry point、credential、role、budget、audit
  record、executorを分離する。
- 現在のquery executorはtransaction-level `READ ONLY`のまま維持する。このinvariantを
  弱めたりparameter化したりしてmutationを実装しない。
- 専用loginは、明示的に許可された非owner・非superuser・非`BYPASSRLS` writer role
  だけを引き受ける。requestからconnection、role、project、conflict policy、
  transaction isolation levelを選択できない。
- M6 compilerはpublished writable modelに対する単一のparameterized `INSERT`または
  承認済み`INSERT ... ON CONFLICT`だけを生成する。任意`UPDATE`、`DELETE`、`MERGE`、
  `COPY`、`CALL`、DDL、expression、multi-statement inputは`0.4`では拒否する。
- idempotency key、最大row/byte、statement/lock timeout、atomicなaudit start/finish、
  明示的affected-row expectationを必須にする。
- compiler crateにはdatabase、transport、logging、audit I/Oを入れない。DBによる拒否を
  stable mutation errorとして公開し、success-shaped responseへ変換しない。

## 8. MCP API

### 8.1 API原則

- MCPはsemantic objectを操作する。raw SQL toolは提供しない。
- discovery結果は、認証principalが利用可能な公開modelだけにfilterする。
- tool input/outputはversion付きJSON Schemaで定義し、`additionalProperties: false`を基本とする。
- 大きなcatalog/resultはcursor paginationを使い、行数・bytes・execution timeに上限を設ける。
- validation errorは機械可読なcode、JSON Pointer、候補名を返すが、非公開objectの存在を漏らさない。

MVPのstdio transportでは、principalとscopeはGatewayの起動設定から固定し、MCP requestの自己申告値を信用しない。後段のHTTP transportでは、reverse proxyまたはGatewayでOIDC/JWTを検証し、requestごとにprincipalを確立する。stdioとHTTPで同じ内部authorization contextを使う。

### 8.2 MVP tools

| tool | 用途 | 実行 |
|---|---|---:|
| `list_semantic_models` | 利用可能なmodelをページング取得 | なし |
| `describe_semantic_model` | field、metric、relationship、grain、制限を取得 | なし |
| `validate_semantic_query` | LSQのschema/semantic/security/cost前検証 | なし |
| `query_semantic_model` | validate→compile→execute→lineage付き結果 | あり |
| `explain_semantic_query` | 正規化LSQ、join plan、source lineage、制限を説明 | 原則なし |

管理用scan/publishはMCPの一般利用者へ公開せず、CLIまたは別の管理scopeに限定する。`compile`で完全な物理SQLを返す機能も、MVPの一般scopeでは無効にし、開発者向けdebug scopeだけで提供する。

### 8.3 Resources

- `semantic://projects/{project}/revisions/current`
- `semantic://projects/{project}/models/{model}`
- `semantic://schemas/lsq/v1`

resourceもtoolと同じ認可filterを通す。prompt templateや自然言語回答生成はcore scopeに含めない。

### 8.4 Query response

```json
{
  "schema_version": "1",
  "query_id": "uuid",
  "semantic_revision": "sha256:...",
  "columns": [{"name": "month", "type": "date"}, {"name": "revenue", "type": "numeric"}],
  "rows": [["2026-08-01", "12345.67"]],
  "truncated": false,
  "lineage": {
    "models": ["orders"],
    "metrics": ["revenue"],
    "source_columns": ["sales.orders.amount"]
  },
  "warnings": []
}
```

numeric、timestamp、date、intervalなどはJSONでの精度・timezone規約を明文化する。MVPではnumericを文字列、timestampをRFC 3339、dateをISO 8601として返す。

### 8.5 Post-MVP Mutation API

M6のmutationは独立してversion化されたcapabilityだけで追加する。CLI/MCP operationの
候補は`validate_semantic_mutation`と`mutate_semantic_model`とし、最終的な名称とschemaは
ADRで確定する。writer profileが設定されていないprocessはmutation toolをadvertiseも
acceptもしない。mutation requestはpublished semantic model/field名だけを使い、生成SQLを
返さず、物理identifierを受け取らない。requestからprincipal、role、project、credential、
idempotency storage、conflict expressionを指定することを拒否する。

既存の5つのread-only MCP toolの意味は維持する。mutation capability negotiation、
audit taxonomy、response shape、replay semantics、compatibility ruleを独立version化し、
read-only deploymentがclient requestだけでwrite可能にならないようにする。

## 9. Logical Semantic Query JSON Schema方針

### 9.1 LSQ v1の外形

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "schema_version": "1",
  "model": "orders",
  "dimensions": [
    {"field": "ordered_at", "time_grain": "month"},
    {"field": "customer_region"}
  ],
  "metrics": [{"metric": "revenue"}],
  "filters": {
    "op": "and",
    "args": [
      {"op": "gte", "field": "ordered_at", "value": {"type": "date", "value": "2026-01-01"}},
      {"op": "in", "field": "status", "values": [{"type": "text", "value": "paid"}]}
    ]
  },
  "order_by": [{"ref": "revenue", "direction": "desc"}],
  "limit": 100
}
```

### 9.2 Schema設計規約

- JSON Schema draft 2020-12を採用し、`schema_version`を必須にする。
- top-levelおよび各nodeは`additionalProperties: false`。
- SQL、table名、column名、expression文字列、function名の自由入力を禁止し、公開semantic symbolだけを参照する。
- literalは`type`付きとし、compilerの型体系で検証してbind parameter化する。
- filterは深さ、node数、`in`要素数に上限を置く。
- `limit`は必須またはserver defaultを適用し、hard maximumを超えられない。
- dimensionとmetric output aliasはserverが決定し、任意identifierを受け付けない。
- join pathはplannerが宣言済みrelationshipから一意に選ぶ。複数候補が同順位ならエラーにする。
- JSON Schemaは構文検証だけを担う。参照存在、型、権限、grain、aggregate compatibility、costはsemantic validationで担う。

### 9.3 互換性

- patch: error messageやdescriptionの変更。入力意味論は不変。
- minor: optional field/operator追加。既存queryは同じ意味。
- major: 意味論が変わる場合に新`schema_version`を追加し、並行サポート期間を置く。
- golden LSQごとにnormalized IR、SQL、parameter、lineageを固定し、compiler更新時の意味差分をreviewする。

## 10. Deterministic SQL Compiler

### 10.1 パイプライン

```text
LSQ JSON
  1. JSON Schema validation
  2. principal-filtered symbol resolution
  3. type / grain / aggregation validation
  4. relationship graph planning
  5. typed relational IR generation
  6. policy and resource guard annotation
  7. PostgreSQL AST generation
  8. stable SQL rendering + bind parameter ordering
  9. generated SQL re-parse / structural assertion
 10. optional EXPLAIN cost guard
 11. read-only execution
```

### 10.2 決定性の定義

次が同じなら、normalized IR、SQL text、parameter type/order、lineage、query hashが同じであることを保証する。

- LSQの意味（object keyの並び順は無視）
- published Semantic Revision
- compiler semantic version
- capability/config profile
- principalの権限集合とpolicy context

安定alias、join順序、projection順序、parameter番号、predicate正規化規則を仕様化する。PostgreSQL plannerの実行計画そのものは統計やversionで変わるため、決定性の対象外とする。

### 10.3 SQL生成の安全条件

- 値はすべてbind parameter。文字列連結でliteralを生成しない。
- identifierはcatalog由来の検証済み値をquoteする。
- 単一の`SELECT`または読み取り専用CTEだけを生成する。
- semicolonによる複数statement、DDL/DML、copy、call、volatile function、unapproved UDFを生成しない。
- `SET TRANSACTION READ ONLY`、`statement_timeout`、`lock_timeout`、`idle_in_transaction_session_timeout`をtransaction localで設定する。
- hard row limitに加え、結果byte上限とcancelを実装する。
- optional `EXPLAIN (FORMAT JSON)`で推定cost/rowsを検査するが、EXPLAIN結果は正しさの根拠にしない。

### 10.4 joinと集計の正しさ

- relationshipに`one_to_one`、`many_to_one`、`one_to_many`、`many_to_many`とjoin keyを明示する。
- MVP compilerはfact→many-to-one/one-to-oneだけを自動選択する。
- M8では、projectされた全metricが共通のdirect root entity-key aggregation anchorを
  宣言する場合だけ、明示的にbindされたdirect one-to-many dimension/filterを許可する。
  宣言済みouter aggregateの前にdimension-plus-anchor grainでduplicate child rowを除去する。
- 複数fact、many-to-many、reverse/multi-hop routing、joined metric input/filter、
  anchor欠落・混在、semi-additive fan-outは引き続き拒否する。
- metricごとにadditivity（時間、entity、全dimension）を保持し、非加法軸での集計を拒否または警告する。
- `count_distinct`は明示されたentity keyだけで許可する。
- NULL join semantics、timezone、week start、currency/unitをmodel contractに含める。

### 10.5 compiler API境界

compiler coreはI/Oを持たない純粋関数に近づける。

```text
compile(
  normalized_lsq,
  immutable_semantic_snapshot,
  principal_capabilities,
  compiler_options
) -> { sql, typed_parameters, output_schema, lineage, warnings, hash }
```

DB接続、MCP、logging、実行は外側に置く。この境界をproperty test、fuzz test、golden testの中心にする。

### 10.6 決定的Mutation Compiler（M6 target）

```text
compile_mutation(
  normalized_lsm,
  immutable_semantic_snapshot,
  principal_capabilities,
  mutation_options
) -> { statement, typed_parameters, affected_model, write_lineage, hash }
```

M6 mutation compilerは上限付きinsertと、明示的にmodel化された冪等upsertだけを扱う。
writable visibility、required/default field、PostgreSQL scalar type、nullability、
generated/identity column、immutable field、許可conflict key、batch上限、return
visibilityを検証する。同じinput、revision、compiler semantic version、capability
profileから同じstatement text、parameter順、lineage、hashを生成しなければならない。
未知または曖昧なfield、client指定の物理名、不完全なconflict key、unsafe default、
未対応expressionはfail closedにする。

## 11. Security

### 11.1 脅威モデル

- prompt injectionにより、非公開modelの発見、raw SQL、制限回避を試みる。
- SQL injectionまたはidentifier injection。
- RLS/GRANTを迂回するconnection roleやpool context漏れ。
- writable-field policy、RLS `WITH CHECK`、constraint、idempotency、affected-row
  expectationを迂回するmutation。
- 高cost query、巨大`IN`、Cartesian join、巨大resultによるDoS。
- catalog/comment/error/logを通じた機密情報漏えい。
- Semantic Modelの悪意ある変更またはsupply-chain混入。
- 古いrevisionやcompiler差分による監査不能。

### 11.2 防御策

- stdioでは起動主体と固定設定からprincipal/scopeを確立する。HTTPではOIDC/JWTの署名、issuer、audience、expiry、scopeを検証する。いずれもrequest body内のprincipal自己申告を信用せず、anonymous remote accessを許可しない。
- least privilegeの分離role、RLS強制、principal mapping allowlist。
- LSQ schema、semantic validation、typed IR、parameterized SQLの多層防御。
- model/field/metric単位のvisibilityとcapability。DB権限は最終防衛線。
- query complexity、join数、filter node、time range、limit、timeout、concurrency、result bytesのbudget。
- errorをpublic codeと内部詳細に分離し、非公開object名やSQLを一般scopeへ返さない。
- logはquery/resultの実値を既定で記録せず、hash、型、件数、timingを記録する。
- source query用read-only pool、introspection、audit writerをcredentialとpool単位で分離する。audit writerは`semantic.query_audit`へのappend/update以外を許可しない。
- mutationは専用writer credentialと分離されたallowlist済みmapped roleを使う。
  read-only credentialには業務データwrite権限を与えず、writer credentialにはpublished
  mutation contractに必要なmodel/table/column operationだけを与える。
- migration/model publishは署名済みreleaseまたはreview必須のCI経由にする。
- dependency audit、SBOM、container image scan、secret scanをrelease gateにする。

### 11.3 必須security test

- tenant Aのprincipalでtenant Bの行が0件または権限errorになる。
- pool reuse後に前requestの`SET LOCAL ROLE`/GUCが残らない。
- table owner、superuser、`BYPASSRLS`接続を起動時に拒否する。
- hidden model/field名をguessしても存在有無を区別できない。
- malicious literal、Unicode identifier、深いfilter、巨大IN、NaN/Infinity、timezone edgeを拒否または安全に処理する。
- cancel/timeout後にtransactionがabort/rollbackされ、接続が安全な状態でpoolへ戻る。
- stdioの固定principalとHTTPのrequest principalが同じauthorization fixtureで同じ可視性になる。
- source execution roleからSemantic Schemaやaudit tableへ書き込めず、audit writerから業務データを読めない。
- read-only deploymentはmutation capabilityをadvertiseせず、request fieldからwrite可能に
  変更できない。
- mutation testはcross-tenant insert、RLS `WITH CHECK`、generated/immutable field、
  duplicate idempotency key、partial batch、constraint/trigger failure、timeout/cancel
  rollback、affected-row mismatch、audit failureを含む。拒否されたmutationを成功として
  報告してはならない。

## 12. Semantic Lineageと監査

### 12.1 三種類のlineage

1. **Design-time lineage**: metric→field→physical column、model→view/table、relationship→join columns。
2. **Query-time lineage**: query→revision→metrics/dimensions→relationships→physical objects→policy context→SQL hash。
3. **Mutation-time lineage**: mutation→revision→writable model/field→physical target
   column→policy context→statement hash→affected-row outcome。

### 12.2 記録項目

- `query_id`、request/correlation ID、timestamp
- principalの不可逆IDまたは監査用subject ID。tokenやsecretは保存しない
- LSQ schema version、canonical LSQ hash
- semantic revision/hash、compiler version、config profile
- resolved model/field/metric/relationship IDとdefinition hash
- source relation/column、policy binding ID、DB role ID
- generated SQL hash。SQL本文の保存はdebug/監査scopeで明示opt-in
- parameter typeと個数。値は既定で保存しない
- validation/compile/queue/DB/result serializationの各時間
- status、error code、row count、byte count、truncated/cancelled

実行前に専用audit connectionで`started` eventをappendし、記録に失敗した場合はqueryを開始しない。完了時に同じ`query_id`へterminal event/statusを書き、process crash等でterminal eventがない`started` recordは監視対象にする。これにより、read-only source transactionを崩さず「実行された可能性のあるqueryに監査記録がない」状態を避ける。

mutation auditは別record typeとidentifierを使う。typed field名、row数、payload byte数、
idempotency-key hash、statement hash、policy context、terminal outcomeを記録し、field値は
既定で記録しない。source transaction開始前にaudit startをdurableにし、terminal statusで
committed、rejected、rolled back、indeterminate、reconciledを区別する。

### 12.3 drift

- physical schema fingerprintとpublished revisionのsource snapshotを比較する。
- column drop/type change、constraint/FK/RLS変更はseverity付きissueにする。
- breaking driftがあるmodelは新規queryをfail closedにできる設定を設ける。
- event triggerやlogical decodingによる即時検知は、必要権限と運用負荷が大きいため後段。MVPは明示scanと定期CIで十分とする。

## 13. pgvectorは後段

pgvectorはMVP依存にしない。semantic nameと明示的なdescription/synonymで十分な発見精度をまず計測する。

導入候補は次に限定する。

- business term、description、承認済みexample queryの類似検索
- 大規模catalogでMCPへ返す候補のranking
- 用語の重複・semantic drift候補の検出

embeddingはqueryの正しさ、権限、join選択、SQL生成に使わない。embedding model/version、source text hash、localeを保存し、再生成可能にする。導入判断は、非vector baselineに対してdiscovery recallが明確に改善し、運用コストと漏えいリスクを上回ることを計測して行う。

## 14. コンテナ開発環境

### 14.1 ローカル前提

maintainer referenceのMac Studioでは`container-compose`を使う。macOS quickstartはApple
Containerを継続利用し、Linux文書とCIは対応OCI/Compose runtimeを使う。portable contractは
単一host runtimeではなくrepository設定とrelease artifactである。文書化された各開発pathで
未起動状態からbootstrapできなければならない。

### 14.2 Composeサービス

| service | 内容 | 常時 |
|---|---|---:|
| `postgres` | PostgreSQL 18既定、named volume、healthcheck、fixture | 必須 |
| `gateway` | postgresem binary、read-only source mountまたはbuild image | 必須 |
| `migrate` | one-shot migration | 必須 |
| `test` | unit/integration/contract test runner | profile |
| `otel-collector` | local trace/metric確認 | observability profile |
| `prometheus` / `grafana` | dashboard開発 | observability profile、後段 |

Compose fileはApple ContainerとLinux CIで共通利用できる機能の交差部分に限定する。特殊なDocker socket、privileged container、暗黙host networkには依存しない。PostgreSQLデータはbind mountではなくnamed volumeを既定とし、権限差を避ける。

M8開始前に、Docker/Podman向けのbyte-equivalentな`Dockerfile`、UID/GID `10001`を
維持するLinux Compose override、rootless Quadlet unitも提供する。CIではDockerと
Podmanの両方でimageをbuild/runし、Linux Compose stackを起動し、Quadlet生成を検証する。
run
[`33575772186`](https://github.com/rioriost/postgresem/actions/runs/33575772186)
はnative Linux amd64/arm64とPodman 4.9でこれらのgateを通過した。

### 14.3 開発コマンドの目標

```text
make doctor       # Apple Container、container-compose、version、portを確認
make dev-up       # migrate後にPostgreSQLとGatewayを起動
make test         # unit + fast integration
make test-all     # PG 16–18、security、golden、migration
make dev-down     # container停止。volumeは保持
make clean-data   # 明示確認付きで開発volumeだけを削除
```

`.env.example`は非secretだけを含める。fixtureは架空データに限定し、実データdumpをrepositoryへ入れない。

### 14.4 PostgreSQL対応

- 初期対応: PostgreSQL 16、17、18。開発defaultは18。
- extensionなしでcore機能が動くことを必須にする。
- pgvector、pg_stat_statements等はoptional capabilityとして検出する。
- version固有catalog差分はadapterで吸収し、support matrixを文書化する。

### 14.5 OS・architecture対応

- M6必須: release binaryとOCI imageがLinux amd64/arm64で実行できる。
- 維持する開発/archive target: macOS amd64/arm64。
- CIで両Linux architecture上のbinaryまたはimageを実行する。起動せずmanifestをbuild
  しただけではsupport gateを満たさない。
- 最低でもCLI contract、TLS初期化、migration互換、catalog load、guarded query実行、
  governed mutationの拒否/smoke、installer検証を両architectureで実行する。
- PostgreSQL 16–18のbehavior matrixとCPU architecture matrixを分け、cross-productを
  縮小する場合は安全な理由をrelease gateへ記録する。
- architecture固有native dependency、OpenSSL/TLS behavior、endianness、filesystem
  assumption、archive namingをrelease testで扱う。

## 15. リポジトリ構成

```text
postgresem/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── AGENTS.md
├── Makefile
├── compose.yaml
├── compose.linux.yaml
├── Containerfile
├── Dockerfile
├── deploy/quadlet/
├── crates/
│   ├── postgresem/              # gateway binary、CLI、wiring
│   └── postgresem-compiler/     # pure LSQ/IR/planner/compiler core
├── migrations/                  # semantic schema、forward-only
├── schemas/
│   ├── lsq/v1.schema.json
│   ├── mcp/
│   └── semantic-expression/
├── fixtures/
│   ├── commerce/
│   ├── rls-multitenant/
│   └── catalog-compat/
├── tests/
│   ├── golden/
│   ├── integration/
│   ├── security/
│   ├── compatibility/
│   └── evals/
├── docs/
│   ├── POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md
│   ├── architecture/
│   ├── adr/
│   ├── threat-model.md
│   ├── lsq-v1.md
│   └── operations/
└── .github/workflows/           # 選定CIに合わせて変更可
```

MVPでcrateを細分化しすぎない。compiler coreだけを分離し、それ以外は`postgresem`内のRust moduleとする。独立releaseや明確な依存逆転が必要になった時点でcrateを増やす。

## 16. テスト戦略

### 16.1 Test pyramid

| 層 | 対象 | 例 |
|---|---|---|
| Unit | AST、型、名前解決、正規化、budget | 同じ意味のJSON key順で同hash |
| Property/Fuzz | LSQ parser、filter AST、renderer | panicなし、未知node拒否、literal非混入 |
| Golden compiler | LSQ→IR/SQL/params/lineage | review可能な差分 |
| Integration | 実PostgreSQL 16–18 | catalog scan、migration、query result |
| Security | GRANT/RLS/pool/timeout | cross-tenant漏えいなし |
| Mutation security | writer role/RLS/idempotency/rollback | unauthorized writeまたは部分成功の誤報なし |
| Contract | MCP/JSON Schema | client fixtureとの互換性 |
| Migration | fresh + N-1 upgrade | published revision保持、rollback方針確認 |
| Known-answer eval | semantic correctness | 代表質問の期待値・期待拒否 |
| Performance | compile/execute/catalog | regression budget |
| Platform | Linux amd64/arm64 binary/image | install、start、query、mutation smoke、TLS初期化 |

### 16.2 正しさのoracle

- 各fixtureに、手書きの信頼できるSQLと期待結果を保持する。
- LSQ結果とoracleを比較し、SQL文字列の一致だけで正しさを判断しない。
- 正答ケースと同数程度の「拒否すべきquery」を用意する。
- join fan-out、NULL、empty set、timezone、DST、numeric precision、duplicate key、RLSを必須fixtureにする。

### 16.3 品質gate

- compiler coreの新operatorはunit、golden、property、integration testを必須とする。
- security-critical moduleはcode owner reviewを必須とする。
- flaky testは放置せず隔離期限を持つ。
- coverage率だけをgateにせず、仕様の分岐、脅威、既知回答のcoverage表を管理する。

## 17. Observability

### 17.1 Structured logs

- JSON形式、UTC timestamp、severity、service version、request/query/mutation ID。
- stage、error code、semantic revision、compiler version、query/statement hash、duration、row/byte/affected-row count。
- literal、token、接続文字列、結果行、未公開commentは既定で記録しない。
- debug SQLはlocalまたは限定scopeで明示的に有効化し、保持期間を短くする。

### 17.2 Metrics

- request数、validation/compile/execute error数、error code別rate
- validation/compile/DB/serialization latency histogram
- active/queued query、pool使用率、timeout/cancel、result truncation
- catalog scan時間、object数、drift issue数、publish数
- mutation validation/commit/reject/rollback数、idempotent replay数、
  indeterminate/reconciliation数
- model/metric別の利用数は高cardinalityを避け、必要ならaudit DBで分析する

### 17.3 Traces

`mcp.request → auth → catalog.resolve → validate → plan → compile → db.acquire → db.query → serialize → audit`をspan化する。M6 mutation traceは独立した`validate → compile_mutation → db.mutate → commit/rollback → audit` pathを使う。OpenTelemetry exportはoptionalとし、coreはvendor-neutralにする。

### 17.4 SLO候補（betaで確定）

- Gateway起因のvalidation+compile p95 < 50 ms（100 model規模のwarm状態）
- 監査event欠落 0件
- M6以降のmutation audit欠落とsuccess-shaped indeterminate outcome 0件
- security test pass 100%
- supported LSQのsemantic correctness eval 100%。unsupportedは明示拒否

DB実行時間はデータ量とindexに依存するため、Gateway SLOと分離する。

## 18. CI/CDとリリース

### 18.1 Pull Request CI

1. format、lint、license header、JSON Schema lint
2. unit、property test、compiler golden diff
3. PostgreSQL 16–18 integration matrix
4. RLS/security、MCP contract、migration test
5. dependency/secret/license scan
6. container image buildとsmoke test
7. documentation link/check、migration checksum確認

### 18.2 Release pipeline

- SemVerを採用し、LSQ schema、Semantic Schema migration、MCP contract、compiler semanticsのcompatibility noteを生成する。
- tagからreproducible binaryとmulti-arch OCI image（最低`linux/amd64`、`linux/arm64`）を作る。
- release CIでLinux amd64/arm64それぞれのsmoke/contract testを実行する。cross build成功
  だけでは不十分とする。
- SBOM、provenance、checksum、署名を配布物に付与する。
- migrationはforward-onlyを基本とし、release前backup、N-1 upgrade test、互換期間を定義する。
- database migrationとbinary rolloutの順序をexpand/contract方式にする。
- release candidateをMac StudioのApple Containerでsmoke testし、CIだけでは見えない互換差を確認する。

### 18.3 Branch/release policy

- `main`は常にrelease可能にする。
- 大きな意味論変更はADR、threat model差分、golden diffを伴う。
- 1.0まではminorで破壊変更を許し得るが、migration guideを必須にする。
- 1.0以降はLSQ major versionの並行サポート期間を設ける。

## 19. マイルストーン

期間は人員確定前の約束にしない。2026-09-01にproject ownerは、betaで確認できた価値に
基づいてM6実装開始を承認した。未完了のM5 independent field/security evidenceは追跡を
継続し、遡って完了扱いにしない。未解決P0/P1 findingは引き続き`0.4` releaseをblockする。

### M0: Project foundation / RFC

- repository基盤、license、governance、ADR template、threat model
- Apple Container/`container-compose` bootstrap、PG 16–18 matrix
- LSQ v1、Semantic Schema v1、compiler意味論のRFC
- Wren AI/Cube/Malloy/MetricFlowとの実証比較

**Exit gate**: 3つの代表datasetと30問のevalで、PostgreSQL-nativeの価値仮説とMVP境界を承認。

### M1: Semantic Catalog alpha

- migration、revision、model、field、relationship、metric、term
- `pg_catalog`/COMMENT/constraint/RLS scanとdrift report
- CLI: `migrate`、`doctor`、`catalog scan`、`model validate`、`model publish`

**Exit gate**: scanが冪等、published revision不変、PG 16–18で同等のnormalized snapshot。

### M2: LSQ compiler alpha

- LSQ JSON Schema、typed IR、symbol/type/grain validation
- single-fact + many-to-one join、基本aggregate/filter/time grain
- parameterized PostgreSQL SQL、golden/property/fuzz test
- design-time/query-time lineage生成

**Exit gate**: 対応evalで正答100%、unsafe/ambiguous queryは誤生成せず拒否。

### M3: Secure execution + MCP MVP

- MCP stdio tools/resources
- auth scope、principal→role、GRANT/RLS、read-only executor
- budget/timeout/cancel/pagination、audit、structured log/metrics
- end-to-end agent demo

**Exit gate**: security suite 100%、cross-tenant漏えい0、監査event欠落0。

### M4: Developer preview

- installer/container image、quickstart、sample project、operations docs
- public API polish、error taxonomy、compatibility policy
- performance baseline、100 model規模のcatalog test
- 外部利用者2組以上の設計フィードバック

**Exit gate**: 新規利用者が30分以内に起動し、実DBのread-only pilotを完了。

### M5: Beta

- N-1 migration、backup/restore、failure recovery、SLO/dashboard
- MCP streamable HTTPの必要性を評価し、採用時は認証込みで実装
- hardening、SBOM/signing、security review、incident runbook
- adoption/価値指標の計測

**Exit gate**: 2つ以上の非fixture DBで4週間運用し、P0/P1 security/correctness defectがない。

### M6: 0.4 — Governed ingestionとportable Linux

**実装状況:** `0.4.0` source treeへの実装は完了。release commitでPostgreSQL
16〜18 suite、native Linux amd64/arm64 runtime gate、実装後reviewが通過した時点で
promotionする。

- LSM v1とSemantic Schemaのwritable-model projectionを仕様化する。
- 上限付きtyped `insert`と承認済み冪等`upsert`を実装し、raw SQL、任意DML、物理identifier、
  `UPDATE`、`DELETE`、`MERGE`、`COPY`、`CALL`をpublic contract外に保つ。
- 分離writer credential/role、RLS `WITH CHECK`、idempotency、atomic mutation audit、
  rollback/reconciliation behavior、安全な拒否testを追加する。
- release binary/OCI imageのLinux amd64/arm64 runtime jobを追加し、installer、TLS、query、
  mutation smokeを含める。
- `1.0`安定性を主張せず、`0.4`のcompatibility、migration、operations、incident、
  threat-model更新を公開する。

**Exit gate**: PostgreSQL 16–18で承認済みinsert/upsertが成功し、拒否・曖昧・duplicate・
cross-tenant・partial failure mutationが完全なaudit evidenceとともにfail closedになる。
Linux amd64/arm64 artifactが両方で必須smoke/contract suiteを実行できる。

### M7: 0.5 — Reference比較とinteroperability

**実装status:** `0.5.0` source treeで完了。2026-09-01時点の比較、fail-closedな
catalog drift gate、Apache Ossie `0.1.1`からreview用candidateを生成する一方向import、
PostgreSQL利用者価値のacceptance、固定したOSS runtimeの実行evidenceを記録した。
reference workflow
[`33517361442`](https://github.com/rioriost/postgresem/actions/runs/33517361442)
ではWren AI、Cube、Malloy、MetricFlowを同一digestのPostgreSQL 18 imageとdatasetに
対して実行し、4実装すべてが期待する`total_revenue`を返した。Python transitive
graph、Python/Node version、`uv`、npm package、Cube imageはpinまたはlockfileで固定し、
evidenceにも記録した。
release workflow
[`33526980637`](https://github.com/rioriost/postgresem/actions/runs/33526980637)
は`v0.5.0`、4種類のnative archive、Linux amd64/arm64 native runtime evidence、
署名済みchecksum、署名済みmulti-architecture imageを公開した。

- 共通PostgreSQL dataset/taskを使い、現行Wren AI、Cube、Malloy、MetricFlow releaseとの
  比較を再実行して文書化する。
- authoring、discovery、query semantics、mutation、API/SDK、lineage、governance、
  operationsのcapability/gap matrixを公開する。
- 外部modelをruntime正本にせず導入負荷を減らせる場合だけ、import/exportまたはmodel
  conversion adapterを追加する。
- feature数のparityではなく計測した利用者価値から後続作業を優先する。

**Exit gate**: 比較を再現でき、選択したgapにPostgreSQL利用者とfixtureが存在し、採用機能が
PostgreSQLを唯一のexecution engineかつsemantic authorityとして維持する。

### M8: 0.6 — Semantic・Mutation coverage

**実装status:** `0.6.0` source treeで完了。ADR 0014、Semantic Snapshot v2、
compiler semantics `0.2.0`、migration 0006-0007、accepted/rejectedを均衡させたcompiler
evaluation、duplicate-child/multi-branch PostgreSQL oracle、root/child RLS実行evidenceで
bounded fan-out contractを実装し、LSQ v1とSnapshot v1の読み込みを維持した。

- ADR 0014に従い、Semantic Snapshot v2の明示的aggregation anchorと、directな
  one-to-many dimension/filter向けの決定的な二段階PostgreSQL aggregationを追加する。
- LSQ v1を維持する。anchor欠落・混在、joined metric input/filter、many-to-many、
  multi-hop/reverse routing、allocationの曖昧性はgrainを推測せず拒否する。
- Snapshot v1のrevision hashと読み込み互換性を維持し、anchor変更をbreakingな
  model changeとして分類する。
- typed `update`/`delete`、multi-fact planning、cumulative metric、time spine、
  custom calendarは、bounded semanticsを定義する個別ADRまで延期する。
- operator追加前にknown-answer/rejection suiteを拡張し、duplicate child、
  multi-branch fan-out、RLS、PostgreSQL 16–18実行evidenceを含める。

**Exit gate**: 新query/mutation semanticsが対応fixtureで正答100%となり、曖昧・unsafe caseを
拒否し、GRANT/RLSを弱めない。

### M9: 0.7 — Application・Agent integration

**実装status:** `0.7.0` source treeで完了。ADR 0015、strict authority/JWKS
loading、RFC 9728 metadata、JWT subjectからroleへの完全一致mapping、stateless
MCP `2026-07-28` POST/SSE、stable authority単位のmutation idempotency、切断時
cancellation、legacy/current reconciliationの決定的precedence、PostgreSQL
16〜18 multi-user integration gateを実装済み。

- ADR 0015に従い、MCP `2026-07-28`の認証済みstateless Streamable HTTP endpointを
  実装し、MCP `2024-11-05` stdio adapterを維持する。
- OAuth resource serverとしてのみ動作する。RFC 9728 metadataを公開し、local設定された
  署名付きJWT access tokenのissuer/audience/expiryを厳密に検証し、検証済みidentityを
  operator設定のquery roleとoptional mutation PostgreSQL roleへ対応付ける。
- colocated HTTPS reverse proxy背後のloopback bind、Host/Origin完全一致allowlist、
  modern MCP request metadata header/body一致、request size上限、principal単位の
  rate/concurrency budget、privacy-safeなerror/logを必須とする。
- `server/discover`、現行result envelope、決定的capability/tool/resource discovery、
  revision-bound pagination、request SSE、disconnectからPostgreSQLへのcancellation、
  hard execution ceilingを実装する。
- remote mutationはdefault無効とする。server gate、検証済みtoken scope、operator identity
  mapping、既存mutation executor、PostgreSQL role check、RLS、idempotency、必須auditの
  全要件を満たす場合だけadvertiseする。
- 複数remote principalへretryを公開する前に、mutation idempotencyとreconciliationを
  検証済みauthority単位でnamespace化する。
- discovery、resource selection、pagination、cancellation、indeterminate mutationを同じ
  idempotency keyでretryする方法について、strict authority/config schemaと公式
  TypeScript/Python SDK guidanceを公開する。
- 2つの認証済みidentityが異なるPostgreSQL RLS resultを受け取り、相互のrole、cursor、
  mutation capability、audit authorityを選択できないmulti-user integration fixtureを
  追加する。

**Exit gate**: multi-user remote deploymentがlocal stdioと同じvisibility、role/RLS、
privacy、query、mutation invariantを維持し、invalid token/origin/host/metadataをfail
closedにし、cancellationがPostgreSQLへ到達してaudit lifecycleを閉じ、Linux
amd64/arm64およびPostgreSQL 16–18 CIが認証済みtransportを実行する。

### M10: 0.8 — PostgreSQL-native scale・operations

- prepared plan、connection管理、materialized view、optional pre-aggregation等の
  PostgreSQL-native手法で計測済みbottleneckを解消し、既定で第二のauthoritative datastoreを
  追加しない。
- large catalog/model authoring workflow、operational dashboard、upgrade automation、
  architecture別performance baselineを追加する。
- reference比較を再実行し、意図的な非parityを文書化する。

**Exit gate**: Linux amd64/arm64でscale targetとfailure recoveryを再現でき、determinism、
freshness、database authorizationを損なわない。

### M11: 0.9 — 1.0 release candidate

- candidate LSQ、LSM、Semantic Schema、MCP、CLI、error、migration、audit contractをfreezeする。
- independent security review、production pilot evidence、upgrade/rollback rehearsal、
  support policy、governance、deprecation policyを完了する。
- 1.0互換性保証を満たせないexperimental surfaceを削除または明示延期する。

**Exit gate**: 未解決P0/P1 correctness/security defectがなく、必須platform、N-1 upgrade、
recovery rehearsalが通り、release-candidate利用者がquery/ingestion workflowを運用できる。

### M12: 1.0 — Stable PostgreSQL Semantic Gateway

- stable contractとcompatibility/support期間を公開する。
- maintainer、release cadence、vulnerability response、sustainability ownershipを確立する。
- 最終reference比較と差別化statementを公開する。

**Exit gate**: correctness、mutation safety、security、migratability、operability、Linux
portability、interoperability、differentiation、governance、maintainer sustainabilityの
全gateを満たす。

## 20. MVPから正式プロジェクトへの段階

| 段階 | 提供価値 | 増やさないもの | 昇格基準 |
|---|---|---|---|
| Spike | catalog→簡易model→LSQ→SQLの縦切り | MCP remote、vector、複雑join | 価値と実現性を2週間規模で確認 |
| MVP | 単一DB、read-only、stdio MCP、基本metric、RLS | UI、cache、多DB、自由SQL | eval/security/lineage条件達成 |
| Preview | docs、packaging、実DB pilot | 分散化、pre-aggregation | 外部利用者が自力導入可能 |
| Beta / 0.3 | migration、運用、HTTP判断、SLO | writeとPostgreSQL外対応 | 4週間の実運用とsecurity review |
| 0.4 | governed insert/upsertとLinux amd64/arm64 runtime | 任意DMLとPostgreSQL外engine | mutation security/correctnessとdual-architecture runtime gate |
| 0.5 | 再現可能なreference比較とtargeted interoperability | feature数parity | evidence-backedなgap優先順位 |
| 0.6 | より広い安全なquery/mutation semantics | 曖昧な自動意味論 | correctness/rejection gate |
| 0.7 | 認証済みapplication/agent integration | anonymousまたはrequest-selected authority | remote invariantがlocalと同等 |
| 0.8 | PostgreSQL-native scale/operations | 必須外部cache/source of truth | 計測済みscale/recovery target |
| 0.9 | freeze済みrelease-candidate contract | 新experimental surface | production/security/platform evidence完了 |
| 1.0 | stable contract、support、governance | PostgreSQL外execution | 継続maintainerと互換性保証 |

正式化はコード量ではなく、以下の証拠で判断する。

- raw SQL/MCPサーバーより正答率・安全な拒否率・監査可能性が改善する。
- PostgreSQL内に意味を置く運用が、外部YAMLの二重管理より有利な利用者が存在する。
- RLS principal伝播を安全に運用できる。
- schema driftとmigrationを現実的に扱える。
- 既存OSSをfork/integrateするより独立coreを維持する合理性がある。

## 21. 既存OSSとの差別化

比較は優劣ではなく、対象範囲と正本の置き場所の違いとして扱う。各OSSは進化が速いため、
M0、M6直後、M10、1.0前に公式資料で再評価する。

| 観点 | Wren AI | Cube | Malloy | MetricFlow | postgresem |
|---|---|---|---|---|---|
| 主眼 | AI/GenBI向けcontext + semantic engine | 汎用semantic/analytics layer | semantic modeling/query language | dbt中心のmetrics compiler | PostgreSQL-native semantic contract + guarded agent gateway |
| model正本 | MDL/YAML等のproject file | YAML/JavaScript等のcode | `.malloy` file | dbt manifest/YAML | PostgreSQL `semantic` schema |
| DB catalog/COMMENT/FK取込 | scaffold機能あり | schema generationあり | connection schemaを利用 | dbt manifest中心 | catalog/COMMENT/FK/CHECK/RLSを第一級のevidenceとしてrevision管理 |
| compiler | 多data source semantic engine | 多data source semantic layer | Malloy→SQL compiler | metric query→SQL compiler | PostgreSQL限定typed LSQ→SQL、決定性と安全な拒否を仕様化 |
| MCP/agent | あり | あり | Publisher等であり | 主眼ではない | raw SQLなし。LSQ discovery/validate/query/explainに加え、独立gate付きtyped LSM mutation |
| governed write | product/API依存 | API/pre-aggregation workflow | 主たるsemantic contractではない | 主たるmetric contractではない | PostgreSQL専用typed ingestion、GRANT/RLS/constraintが正本。0.4から開始 |
| security正本 | semantic/product policy | semantic access policy | 接続先権限との組合せ | dbt/platformとの組合せ | PostgreSQL GRANT/RLSを最終正本にしprincipalをDBまで伝播 |
| lineage | 製品/engine機能 | 製品/semantic機能 | compiler metadata | semantic manifest/plan | revision、compiler、policy、source columnをqueryごとに構築 |
| 対象DB | 多数 | 多数 | 複数 | 複数warehouse | PostgreSQL専用 |

### 21.1 独自性

1. **Meaning lives with data**: 意味モデルがPostgreSQLのtransaction、backup、role、migrationの対象になる。
2. **Native evidence ingestion**: PostgreSQL固有のcatalog、COMMENT、制約、RLS、依存関係を補助情報ではなく管理対象として扱う。
3. **Database-enforced identity**: Gateway policyだけに依存せず、principalを非`BYPASSRLS` role/contextとしてDBへ伝える。
4. **Narrow deterministic contract**: 自由SQLや自然言語をcompiler入力にせず、versioned LSQとtyped IRを公開境界にする。
5. **Lineage by compilation**: Semantic Revisionから実行source/policyまでを同一pipelineで追跡する。
6. **PostgreSQL-only depth**: 多dialect抽象化より、PostgreSQLの型、RLS、catalog、EXPLAIN、timezone意味論を深く扱う。

### 21.2 差別化しない領域

- Wren AIのGenBI体験、Cubeのcache/pre-aggregationと多様なAPI、Malloyの表現力、MetricFlow/dbt ecosystemとは正面競合しない。
- 将来はimport/export adapterやcompiler比較fixtureを提供し、共存を検討する。
- M0で初期境界を確立した。M7とM10で現行releaseとの比較を繰り返し、import/exportや
  実装技法を採用し得るが、PostgreSQL以外のruntime抽象化や第二のsemantic source of
  truthは採用しない。

## 22. 主要リスクと判断ゲート

| リスク | 影響 | 緩和策 | 判断ゲート |
|---|---|---|---|
| 既存OSSとの差が小さい | 独自projectの価値不足 | 3 dataset/30問で比較、利用者interview | M0で継続/fork/integration/中止 |
| join fan-outとmetric意味論 | 静かに誤答する | v1範囲制限、grain/additivity、既知回答・拒否eval | M2で正答100%未達ならscopeをさらに削る |
| RLS principal伝播 | 越権・漏えい | 非owner role、`SET LOCAL`、pool test、外部review | M3未達ならremote executionを公開しない |
| governed mutation迂回または部分成功 | unauthorized/corrupt dataまたは偽success | 分離writer role、typed LSM、RLS `WITH CHECK`、constraint、idempotency、atomic audit、rollback/reconciliation test | M6未達ならmutationを公開しない |
| Semantic SchemaがDBを汚す | 導入拒否・migration事故 | 専用schema/role、forward migration、uninstall/export | Preview前に実DB pilot評価 |
| catalogのversion差・drift | 誤ったmodel/停止 | PG 16–18 fixture、fingerprint、明示scan | 各PG releaseでsupport更新判断 |
| COMMENTの品質不足 | discovery精度不足 | explicit term/editor workflow、候補confidence | Previewで運用工数を計測 |
| Rust/MCP ecosystem依存 | 実装遅延・protocol追随 | adapter分離、protocol contract test | M0 ADRでstack確定 |
| compiler自作の保守費 | project持続不能 | pure core、限定operator、既存compiler再利用評価 | M0/M4でbuild-vs-integrate再判定 |
| DoS/high-cost query | DB障害 | budget、EXPLAIN guard、timeout、concurrency | M3 load/security test |
| Apple Container Compose差 | ローカル再現性低下 | Compose交差機能、Mac smoke test、Linux CI | M1で同一fixture結果を確認 |
| Linux architecture gap | 対応CPUでrelease artifactが動かない | amd64/arm64でinstaller/binary/image実行test、native dependency追跡 | M6で両architectureをrelease blockingにする |
| metadata/監査の機密性 | schemaや利用傾向漏えい | metadata RLS、redaction、retention | M3 threat model review |
| pgvector先行導入 | 複雑化、誤った正しさ依存 | post-MVP、ranking限定、baseline比較 | Beta以降の独立ADR |

### 22.1 Stop条件

次のいずれかに該当する場合、機能追加ではなく中止・統合・方向転換を検討する。

- PostgreSQL内Semantic Schemaの運用上の利点を示すpilot利用者が得られない。
- 既存OSSの薄いadapterで同じ要件を満たせる。
- 対応範囲を限定しても、metricの正しさを安定して保証できない。
- RLS/role mappingを安全かつ理解可能に運用できない。
- 独立compilerとschema migrationを維持できるmaintainer体制がない。

## 23. 優先順位

### P0: MVPを成立させるもの

- LSQ v1とSemantic Schema v1の仕様
- immutable revision/publish
- catalog/COMMENT/FK/CHECK/RLS scanとdrift
- typed IRと限定的deterministic compiler
- read-only/RLS-aware executor
- MCP discovery/validate/query/explain
- lineage/audit、security/golden/integration tests
- Apple Container + Linux CIの再現環境

### P1: Preview/Betaに必要

- model diff、error UX、sample/eval拡充
- HTTP transportと本格認証（需要確認後）
- backup/restore、N-1 migration、SLO/dashboard
- packaging、signed release、external security review
- 大規模catalogのpagination/performance

### P2: 0.4に必要

- LSM v1、writable model metadata、insert/upsert compiler/executor
- 分離writer role、mutation audit/idempotency/reconciliation
- Linux amd64/arm64 runtime・installer execution gate

### P3: 1.0まで比較駆動で判断

- pgvector discovery ranking
- approved example query retrieval
- event trigger/CDC drift detection
- many-to-many/symmetric aggregate
- cache/pre-aggregation
- import/export adapter（Wren/Cube/Malloy/MetricFlow）
- UI、自然言語回答
- PostgreSQL外dialectは1.0まで対象外

## 24. 実装前に作成するADR

1. ADR-001: Rust採用とdependency選定
2. ADR-002: Semantic Schema v1とrevision/publish model
3. ADR-003: LSQ v1、型体系、NULL/timezone/numeric意味論
4. ADR-004: join cardinality、grain、additivity、拒否規則
5. ADR-005: principal→PostgreSQL role/session contextとRLS
6. ADR-006: MCP transport、認証境界、tool/resource contract
7. ADR-007: audit/lineage保持期間と機密情報redaction
8. ADR-008: migration、backup、compatibility、uninstall/export
9. ADR-009: build vs Wren/Malloy/他compiler integration評価
10. ADR-010: LSM v1、writable model metadata、insert/upsert semantics、idempotency
11. ADR-011: writer role/RLS/audit/reconciliation security boundary
12. ADR-012: Linux amd64/arm64 support evidenceとrelease matrix

## 25. 最初の実装バックログ

1. repository metadata、license、contribution/security文書を確定する。
2. `compose.yaml`、Containerfile、PG 16–18 fixtures、`make doctor/dev-up/test`を作る。
3. commerceとRLS multi-tenant fixture、30問の正答/拒否evalを先に作る。
4. LSQ v1とSemantic Expression v1のJSON SchemaをRFC化する。
5. Semantic Schema migration v1とrole/privilege設計を実装する。
6. catalog snapshot/import/driftを実装し、PG 16–18 golden snapshotを固定する。
7. compiler coreのsymbol/type/grain validatorとtyped IRを実装する。
8. single-fact、many-to-one、基本aggregate/filter/time grainを実装する。
9. guarded executor、RLS principal mapping、timeout/limit/cancelを実装する。
10. MCP stdio adapterと5 tools、query response、error taxonomyを実装する。
11. query-time lineage/auditとobservabilityを実装する。
12. end-to-end evalとMVP exit reviewを実施する。

### 25.1 M6実装バックログ

1. write-capable connection追加前にADR 010–012とthreat model更新を完了する。
2. LSM v1 JSON Schemaとwritable-model snapshot projectionをaccepted/rejection fixture付きで
   定義する。
3. database、transport、logging、audit I/Oを含まないpure deterministic insert/upsert
   compilerを実装する。
4. 既存runtime query roleへwrite capabilityを付与せず、専用writer roleとmigrationを追加する。
5. idempotency、audit lifecycle、transaction rollback、affected-row check、reconciliationを
   実装する。
6. mutationを既定無効にしたCLI/MCP capability negotiationを追加する。
7. Linux amd64/arm64のinstaller、binary、OCI、TLS、query、mutation smoke jobを追加する。
8. `0.4` migration、compatibility、security、operations、incident文書を公開する。

## 26. セルフレビュー結果と反映事項

**現在のレビュー判定: scope gate付きでM6（`0.4`）へGO。** 当初のM0条件付きreviewは
動作するread-only betaへつながった歴史的設計根拠として以下に残す。次に正当化される拡張は
governed ingestionと必須Linux portabilityであり、1.0宣言や汎用multi-database semantic
layer化ではない。

### 技術的な弱点

- **join/aggregateの正しさが最大の難所**: 当初想定しがちな汎用join graphは危険なため、MVPをsingle-fact + many-to-one/one-to-oneへ制限した。
- **RLSをGatewayで再現すると二重の認可正本になる**: RLS式は取り込み・説明対象に留め、DBで強制しprincipalをtransaction localに伝播する設計へした。
- **JSON Schemaだけでは意味を保証できない**: schema validationとsemantic/type/grain/cost validationを明確に分離した。
- **OIDは安定IDにならない**: canonical物理名とfingerprintを正本にし、OIDはsnapshot補助に限定した。
- **生成SQLの決定性とDB実行計画の決定性を混同しやすい**: 保証対象をIR/SQL/parameter/lineageに限定した。

### 過剰設計の削減

- microservice、message queue、独立policy serviceを採用せず、単一Gatewayにした。
- crate分割をcompiler coreとbinaryの2つに留めた。
- event trigger、logical decoding、cache、pre-aggregation、UI、vector searchをMVPから外した。
- many-to-many、複数fact、window function、任意UDFを先送りした。
- management APIを一般MCP toolにせず、CLI中心にした。

### 不足を補った項目

- principal mappingとconnection poolのcontext漏れ対策。
- COMMENTが機密情報の保管に不適切であることと、metadata自体のRLS。
- numeric/timezone/NULL、join grain/additivity、result byte上限。
- schema drift、migration、backup/restore、uninstall/exportの検討。
- 正答だけでなく「安全に拒否すべきquery」を品質指標に追加。
- stop条件とbuild-vs-integrateの再判断gate。

### 優先順位レビュー

M6の最優先はmutation contract、writer/RLS境界、rollback/audit correctness、Linux
amd64/arm64実行evidenceである。`0.4`以降は再現可能なreference比較と利用者evidenceを
優先する。MCPや自然言語の幅、cache、より豊富なsemanticsは、安全なcoreを維持できるgateを
満たす場合だけ追加する。pgvectorは発見baselineが不足した場合だけ導入する。

## 27. 公式参考資料

- PostgreSQL: [System Catalogs](https://www.postgresql.org/docs/current/catalogs.html)
- PostgreSQL: [`COMMENT`](https://www.postgresql.org/docs/current/sql-comment.html)
- PostgreSQL: [Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)
- PostgreSQL: [Row Security Policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html)
- PostgreSQL: [`INSERT`](https://www.postgresql.org/docs/current/sql-insert.html)
- PostgreSQL: [`CREATE POLICY`](https://www.postgresql.org/docs/current/sql-createpolicy.html)
- Apple: [`container`](https://github.com/apple/container)
- Wren AI: [What is Modeling Definition Language (MDL)?](https://docs.getwren.ai/oss/engine/concept/what_is_mdl)
- Wren AI: [MDL schema reference](https://docs.getwren.ai/oss/reference/mdl)
- Cube: [Introduction / Semantic Layer Architecture](https://docs.cube.dev/docs/introduction)
- Cube: [Access Control](https://docs.cube.dev/docs/data-modeling/access-control/index)
- Malloy: [Official repository and architecture overview](https://github.com/malloydata/malloy)
- MetricFlow: [Metric semantics in dbt-core](https://github.com/dbt-labs/dbt-core/blob/main/crates/dbt-metricflow/docs/metric-semantics.md)
