# PostgreSQL Semantic Gateway

[English](README.md)

`postgresem`は、AIエージェントとアプリケーションのためのPostgreSQLネイティブな
セマンティックゲートウェイです。型付きのLogical Semantic Query（LSQ）と
Logical Semantic Mutation（LSM）を受け取り、変更不能な公開済みセマンティック定義に
基づいて解決し、PostgreSQLの認可の下でパラメータ化された操作を実行します。

このガイドは**postgresem 1.0.0**を対象としています。PostgreSQLを唯一の実行エンジン
とし、データとその統制された意味の両方を管理する正本として使用します。

## What problem does postgresem solve?

PostgreSQLは、テーブル、カラム、型、キー、制約、コメント、権限、行レベルセキュリティ
など、データの構造を把握しています。しかし構造だけでは、売上計上に使う日時、
承認された指標の定義、集計単位を崩さずに結合できるテーブルまでは説明できません。

こうした意味は、アプリケーションコード、BIツール、プロンプト、ドキュメント、
YAMLファイルなどへ重複して記述されがちです。それぞれが独立して変化すると、
定義の不一致が生まれます。物理スキーマだけを参照するエージェントは、構文上は
正しいSQLでも、注文を二重計上したり、業務上誤った定義を使ったり、想定した
アプリケーションの公開範囲を超えてデータを取得したりする可能性があります。

`postgresem`は、レビュー済みのモデル、フィールド、リレーション、指標、ポリシー
関連付けをデータとともにPostgreSQLへ保存します。`pg_catalog`、コメント、キー、
制約などのメタデータはモデル化の根拠になりますが、業務上の意味についての人による
レビューを置き換えるものではありません。定義は変更不能なリビジョンとして公開され、
ハッシュで検証されます。

アプリケーションはSQLではなく承認済みの意味名を使います。コンパイラーは、
決定的で上限付きのパラメータ化操作を生成するか、未対応または曖昧な入力を拒否します。

| 利点 | 得られる効果 |
|---|---|
| 意味定義の正本を一つに集約 | アプリケーションやエージェントが、それぞれ別の業務定義を維持する代わりに、PostgreSQL内の公開済み定義を共有します。 |
| データベースによるアクセス制御 | PostgreSQLのGRANTとRLSを最終的な権限として維持します。セマンティック層がデータベースの拒否を覆すことはありません。 |
| 一貫した運用 | スキーマ変更、意味定義の公開、差分検出、バックアップ、復元をPostgreSQLの運用境界内で扱えます。 |
| 追跡可能性 | 実行を意味定義のリビジョン、コンパイラーバージョン、ポリシーコンテキスト、監査記録に関連付けます。 |
| 追加インフラの削減 | コア機能に外部カタログサービス、ベクトルデータベース、ポリシーエンジン、結果キャッシュは不要です。 |

## Usage Scenario

| シナリオ | postgresemの使い方 |
|---|---|
| 業務データを扱うAIアシスタント | MCPクライアントが承認済みモデルを発見し、SQLを生成せずに売上やサブスクリプションなどの指標を問い合わせます。 |
| アプリケーションのレポート機能 | ダッシュボードが、共通の指標定義、承認済みの結合、明示的な集計ルールを使ってLSQを送信します。 |
| 統制されたデータ投入 | 独立した書き込みロールを通じて型付きLSMのinsertや承認済みupsertを実行し、冪等な再実行と結果照合を行います。 |
| メタデータと変更の管理 | 運用者がPostgreSQLカタログを取得し、モデル候補を生成して、公開前に意味定義や認可の変更を比較します。 |

マルチテナントアプリケーションでは、認証済みHTTPの主体を運用者が設定した
PostgreSQLロールへ対応付け、参照できる行をRLSで制御します。
[Commerceサンプル](examples/commerce/README.md)と
[ローカルWebデモ](examples/web_demo/README.md)で連携例を確認できます。

対象範囲は意図的に限定しています。任意SQLや汎用的なupdate/delete、PostgreSQL以外
での実行、自動的な事前集計やマテリアライズドビューへのルーティングは提供しません。
未対応の複数ファクト、多対多、多段リレーションを使う集計は、推測せず拒否します。
対応する意味論は[互換性ポリシー](docs/compatibility.md)、自身のデータベースへの
導入は[モデル作成・運用ガイド](docs/operations.md)を参照してください。

## Installation

対応するPostgreSQLは**16、17、18**です。ネイティブバイナリは**LinuxとmacOSの
amd64／arm64**向けに提供します。コンテナはLinux amd64／arm64で動作し、
Apple silicon搭載macOSではApple Containerを利用できます。

**導入用ファイルとサンプルの取得**

```sh
git clone --branch v1.0.0 --depth 1 https://github.com/rioriost/postgresem.git
cd postgresem
```

以降のリポジトリ内コマンドは、このディレクトリから実行してください。

**ネイティブCLI**

[Cosign](https://docs.sigstore.dev/cosign/system_config/installation/)を導入し、
`curl`、`tar`、および`shasum`または`sha256sum`が利用できる状態で実行します。

```sh
scripts/install.sh 1.0.0
export PATH="$HOME/.local/bin:$PATH"
postgresem --version
postgresem contract show
```

インストーラーはホストに対応するファイルを選び、リリース署名とアーカイブの
チェックサムを検証して、sudoを使わず`~/.local/bin`へバイナリを配置します。
PostgreSQLの作成、マイグレーションの適用、認証情報の設定は行いません。
既存データベースへの導入は[運用ガイド](docs/operations.md)を参照してください。

**ローカルコンテナ環境**

サンプル環境は、取得したソースからゲートウェイをビルドし、PostgreSQL 18を起動して
マイグレーションを適用し、架空のCommerceモデルを公開します。本番環境の構成
テンプレートではなく、ローカルデモ用です。この方法ではネイティブCLIは不要です。

Git、Make、および下表のいずれかのコンテナランタイムを使用します。
サンプルの実行にはPython 3.9以降も必要です。

```sh
cp .env.example .env
chmod 600 .env
```

起動前に`.env`を編集し、すべての仮パスワードを用途ごとに異なるランダムな
ローカル専用の値へ置き換え、対応する接続URLも更新してください。
本番の認証情報は使用せず、`.env`はコミットしないでください。

| 環境 | 起動 | データベースのデータを削除せず停止 |
|---|---|---|
| Docker EngineとCompose v2を使うLinux | `make docker-up` | `make docker-down` |
| Apple Container 1.0.0と`container-compose` 1.1.0を使うApple silicon搭載macOS | `make dev-up` | `make dev-down` |
| rootless Podman 4.9以降とsystemdを使うLinux | [Quadletの導入手順](docs/linux-containers.md#rootless-podman-quadlet)を参照 | 導入したユーザーサービスを停止 |

詳細は[Linuxコンテナの設定](docs/linux-containers.md)または
[Apple Containerクイックスタート](docs/quickstart.md)を参照してください。

## Quick Usage

上記の手順でローカル環境を起動してください。以下はDocker Compose向けです。
Apple Containerでは`make docker-mcp`を`make mcp`へ置き換えます。

**MCPによるサンプルデータの取得と投入**

```sh
python3 examples/commerce/mcp_smoke.py \
  --lsq examples/commerce/revenue-by-month.json \
  --lsm examples/commerce/order-insert.json \
  -- make docker-mcp
```

クライアントはMCPを初期化し、モデルを発見して、クエリを検証・実行した後、
統制された書き込み経路で架空の注文を投入します。
**このコマンドはサンプルデータベースへ書き込みます。**
同じLSMの冪等キーで再実行した場合は、別の注文を追加せず、確定済みの結果を返します。

注文の売上合計を取得するLSQは、次のように記述します。

```json
{
  "schema_version": "1",
  "model": "orders",
  "metrics": [{"metric": "revenue"}],
  "limit": 10
}
```

MCPクライアントは、このオブジェクトを`query_semantic_model`の`lsq`引数として、
ツール引数`schema_version: "1"`とともに送信します。クエリ結果にはカラムのメタデータ、
行データ、リビジョンと監査の識別子、結果の打ち切り状態が含まれます。
numeric型の値は、精度を保つためJSON文字列で返します。

**Webデモを試す**

```sh
python3 examples/web_demo/server.py -- make docker-mcp
```

<http://127.0.0.1:8765>を開いてください。ブラウザーはMCP経由で定義済みの
セマンティッククエリを実行し、PostgreSQLへ直接接続しません。`Ctrl-C`でデモを
終了した後、Installationの表にあるコマンドでコンテナ環境を停止します。

**エージェントやアプリケーションを接続する**

ローカルMCPクライアントには、作業ディレクトリを取得したリポジトリに設定し、
`make docker-mcp`、Apple Containerの場合は`make mcp`を起動するよう指定します。
これらは対話型シェルではなく、stdio上で改行区切りのJSON-RPCを処理します。

| 操作 | MCPツール |
|---|---|
| モデルの発見 | `list_semantic_models`、`describe_semantic_model` |
| クエリ | `validate_semantic_query`、`explain_semantic_query`、`query_semantic_model` |
| 統制された書き込み（有効化時） | `validate_semantic_mutation`、`mutate_semantic_model`、`reconcile_semantic_mutation` |

リモートクライアントには、[認証済みHTTPの導入ガイド](docs/mcp-http.md)に従って
`postgresem mcp serve-http`を使用します。stdioはMCP `2024-11-05`、
認証済みのステートレスStreamable HTTPは`2026-07-28`に対応します。
拒否されたリクエストの扱いは[エラーリファレンス](docs/error-reference.md)を
参照してください。

## Security boundary

**PostgreSQLを権限の最終的な正本とします。** クエリは読み取り専用トランザクションで
実行し、所有者でもsuperuserでも`BYPASSRLS`でもない検証済みロールを使用します。
トランザクション単位のタイムアウトを設定し、データ操作前に永続化された監査開始
記録を必須とします。結果には行数とバイト数の上限があります。

書き込みには独立した認証情報、ロール、コンパイラー、実行器、冪等性管理、
監査ライフサイクルを使用します。書き込み可能なのは公開済みのinsert/upsert定義
だけです。業務データの変更、確定済みの再実行結果、監査の確定処理は同一の
トランザクションで行います。PostgreSQLのカラム単位GRANT、RLSの
`USING`／`WITH CHECK`、制約、トリガーは引き続き強制されます。

クエリや書き込みのリクエストからSQL、物理識別子、接続認証情報、
データベースロールは指定できません。
stdioの実行権限は起動時に固定し、HTTPでは検証済みの主体を使って事前設定済みの
対応付けからのみ選択します。MCPレスポンスに生成SQLや物理リネージは含めません。
診断ログには入力値、認証情報、結果行、非公開名、主体情報を含めず、非公開の
セマンティックオブジェクトと未知のオブジェクトには同じ公開エラーを返します。

PostgreSQL接続には明示的な`sslmode`が必要です。リモート接続には
`sslmode=require`を使用し、プラットフォームの信頼ストアによる証明書とホスト名の
検証を行います。`sslmode=disable`はローカルまたは別途保護された接続に限定してください。

HTTPリスナーはループバックだけにバインドし、同居するHTTPSリバースプロキシを
必要とします。ローカルのauthority／JWKS設定でJWTを検証し、トークン発行、
転送ヘッダーによる主体情報の信頼、鍵の動的な検出は行いません。リモート書き込みには、
運用者による明示的な有効化、検証済みスコープ、対応付けられた書き込みロールが
必要です。設定を変更した場合はプロセスを再起動します。

LinuxのComposeとQuadletではUID/GID `10001`でゲートウェイを実行します。
Apple Containerではhostsファイルへの対応のためCompose設定上はrootを指定しますが、
起動時に権限を下げ、MCPを明示的に`postgresem`ユーザーで実行します。

サプライチェーンは継続的に監視します。依存関係のチェック、ワークフローActionの
固定、署名検証を、アプリケーションのセキュリティ対策と併用します。
本番のバックアップ保持、HA、復旧目標、IDプロバイダーの運用、プロキシの設定は
運用者の責任です。[SECURITY.md](SECURITY.md)、[SUPPORT.md](SUPPORT.md)、
[バックアップと復元](docs/backup-restore.md)も参照してください。

## Packaging status

1.0.0のリリース成果物は、次の構成です。

| 成果物 | 形式・対象 |
|---|---|
| ネイティブアーカイブ | `postgresem-1.0.0-{linux,darwin}-{amd64,arm64}.tar.gz` |
| OCIイメージ | `ghcr.io/rioriost/postgresem:1.0.0`、`linux/amd64`／`linux/arm64`向け |
| アーカイブの完全性情報 | `SHA256SUMS`、`SHA256SUMS.sig`、`SHA256SUMS.pem` |
| イメージのメタデータ | SBOMとビルド来歴 |
| 導入用ソース | `Dockerfile`、`Containerfile`、Composeファイル、rootless Podman用Quadletユニット |

ネイティブアーカイブにはバイナリ、スキーマ、契約マニフェスト、一部のポリシー
文書を同梱します。導入用ファイル、マイグレーション、サンプルはソースリポジトリに
含まれます。インストーラーが配置するのはバイナリのみです。

リリース処理では、公開前にLinuxの両アーキテクチャでバイナリとイメージを実行します。
チェックサムと変更不能なイメージdigestには、GitHub OIDCを使ったSigstoreの
キーレス署名を付けます。検証では、想定するリリースワークフローとタグの識別情報、
およびissuerの両方を制約してください。チェックサムだけでは配布元を認証できません。
再現可能な導入にはイメージdigestを固定します。ローカルビルドしたイメージは、
署名済みリリース成果物ではありません。

ダウンロードは[GitHub Releases](https://github.com/rioriost/postgresem/releases)、
バージョンの保証範囲は[互換性ポリシー](docs/compatibility.md)と
[廃止ポリシー](docs/deprecation-policy.md)を参照してください。

## License

`postgresem`は[MIT License](LICENSE)で提供します。
貢献方法は[CONTRIBUTING.md](CONTRIBUTING.md)を参照してください。
