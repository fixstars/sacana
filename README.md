# sacana (Slackbot As Computer Account maNAger)

sacanaはSlack上からコンピューター上のユーザーアカウントを操作するためのSlack botです。

研究室やサークル、会社内などの共用コンピューターに導入することで、各団体のSlack上でユーザーアカウントの発行を行うことができるようになります。誰でも自由にアカウントの追加を許されている共用コンピューターの運用に際して、人手による運用コストを削減できるメリットがあります。
Slackにbotを追加するための権限と導入対象のコンピューターの管理権限が必要です。

## 免責事項

本ソフトウェアはその特性上、セキュリティが重要な環境での導入には慎重な判断を行ってください。
本ソフトウェアの導入や利用によって発生する如何なる問題についても開発元は一切の責任を負いません。
詳しくは、後述の「ライセンス」項をご参照ください。

## 動作環境

systemdが入ったLinux上での動作を想定しています。
Ubuntu 16.04, Ubuntu 18.04, CentOS 7.2で動作を確認しています。

## インストール

1. 依存パッケージをインストールします。
    - UbuntuなどDebian系では `sudo apt-get install passwd pkg-config libssl-dev` でインストール出来ます。
1. [Rustをインストール](https://www.rust-lang.org/install.html)します。
    - 最新のstableの利用を推奨します。
1. `cargo build --release`でビルドします。
1. `settings.json.sample` を参考に `settings.json` を記述します。
    - `SLACK_API_TOKEN` : bot用のSlackのAPIトークンを記述してください。
    - `channels` : 監視するチャンネル名をリストで記述してください。記述されたチャンネル全てを監視します。
    - `public_key_uri_format` : `{}` をユーザー名に置換して公開鍵のURIが得られるような文字列を記述してください。
    - `host_list_uri` : ホストの一覧を得られるURIを記述してください。ここで、一覧は各行にホストの名前を記述したテキストファイルです。
    - `certificate_file` (オプション) : `host_list_uri` のアクセス先などのSSL証明書を追加したい場合、証明書のファイル(PEM形式)のパスを記述してください。必要ない場合はこのオプションを記述する必要はありません。
1. `install.sh` を実行します。
    - systemdにサービス登録を行うため、root権限が必要です。
    - インストール先を変更したい場合は、 `INSTALL_DIR` 環境変数にインストール先のパスを指定して `install.sh` を実行すると変更できます。
1. 起動すると、設定されたチャンネルに"Hello, this is sacana@xxxx"と応答があるのを確認します(「xxxx」はホスト名)。
1. (オプション) 動作確認後、 `/etc/ssh/sshd_config` に `PasswordAuthentication no` を記述します。
    - このbotによって管理されるアカウントは公開鍵認証でログインできるようになるので、パスワード認証はオフにしたほうがセキュリティ上安全です。

## 使い方

以下ではホスト名が `HOSTNAME` のコンピューター上で次の `settings.json` を用いて実行し、Slack上では `@computer-account-manager` の名前でbotとして追加した場合を例にして、使い方を説明します。

```json
{
  "SLACK_API_TOKEN": "(Botユーザー @computer-account-manager のトークン)",
  "channels": ["computer-account"],
  "public_key_uri_format": "https://github.com/{}.keys",
  "host_list_uri": "https://example.net/host_list.txt"
}
```

また、 `https://example.net/host_list.txt` の中身は以下のようになります。

```
HOSTNAME
HOSTNAME2
```

### 新規にアカウントを作る

1. https://github.com/<自分のID>.keys に公開鍵が登録されていることを確認します。
    - この設定の場合、Githubの自分のアカウントでSSH鍵を登録するとhttps://github.com/<自分のID>.keysから自分の公開鍵を取得できます。
    - ここで、<自分のID> にはSlackの表示名(display name)が入ります。
        - したがってこの設定の場合、Slackの表示名をGithubのアカウント名と同じものにする必要があります。
    - 公開鍵の取得ができればよいため、 `public_key_uri_format` のURIは必ずしもgitリポジトリのホスティングサービスのものである必要はありません。
1. #computer-account チャンネルで `@computer-account-manager create HOSTNAME` と発言します。
    - Slackの表示名でアカウントが作成されるのと同時に `public_key_uri_format` から取得した公開鍵の登録も行われます。
    - 作成したアカウントのパスワードは空となります。
1. SSHで公開鍵認証によるログインができるか確認します。
    - ログイン後に `passwd` コマンドでパスワードを設定しましょう。
    - ログインできない場合下記の鍵の更新を行います。

### 既存のアカウントの公開鍵を更新する

1. https://github.com/<自分のID>.keys に公開鍵が登録されていることを確認します。
1. #computer-account チャンネルで `@computer-account-manager update HOSTNAME` と発言します。
    - **注意**: 既に `$HOME/.ssh/authorized_keys` が存在する場合上書きされます。
    - 「新規にアカウントを作りたい場合」と同様、自分のSlackの表示名と同じ名前のアカウントの公開鍵を更新します。
1. SSHでログインできるか確認します。
    - ログインできない場合 https://github.com/<自分のID>.keys に正しい公開鍵が登録されているか確認してください。

### グループに参加する

- グループ GROUPNAME に参加したい場合、 #computer-account チャンネルで `@computer-account-manager join GROUPNAME HOSTNAME` と発言すると参加できます。
    - **注意**: 誰もが `sudo` グループや `wheel` グループに参加できてしまうため、管理者権限を利用者全員に付与したくない環境での利用は十分注意してください。

### slackbotが動いているか確認する

- #computer-account チャンネルで `@computer-account-manager ping` と発言すると起動しているbotからスレッドに `pong@HOSTNAME` と返信が来ます。

### 使い方を確認する

- `@computer-account-manager` にDMで `help` と送ることでヘルプを見ることができます。
    - このメッセージを見るためには `host_list` で最初に書かれたホスト上のbotサービスが正常に稼働している必要があります。

### サービスを再起動する

- root権限で `systemctl restart sacana` を実行します。

### 管理対象のコンピューターを追加/削除する

1. ホスト一覧を更新します。
    - 管理対象を追加したい場合は、一覧に新たなコンピューターのホスト名を追加します。
    - 管理対象を除外したい場合は、一覧から対象コンピューターのホスト名を削除します。
    - この際、ホスト一覧で一番上に書かれたホストが `help` コマンドなどの応答を行うので、一覧の一番上のホスト名には気をつけてください。
1. ホスト一覧から削除したコンピューター上でbotのサービスを停止します。
    - root権限で `systemctl stop sacana && systemctl disable sacana` を実行します。
1. ホスト一覧に新たに追加したコンピューター上でbotのサービスをインストール・起動します。
1. 更新後のホスト一覧にあるすべてのコンピューター上でbotのサービスを再起動します。
1. `@computer-account-manager ping` コマンドでホスト一覧に記載されているコンピューターすべてで正常にサービスが動作していることを確認します。
    - ホスト一覧から削除したホストから返事があったり、ホスト一覧に記載されているホストから返事がなかった場合はそれぞれの端末について確認してください。

## 故障かな？と思ったら

### コマンドに応答がない

まず、`@computer-account-manager ping`コマンドで`pong`が返ってくることを確認してください。

目安として、1分以上経っても目的のコンピューターから応答が返ってこないなら、サービスが起動していない可能性があります。その場合は以下の手順に従ってサービスの再起動を行います。

1. root権限で `systemctl restart sacana` を実行します。
   再起動によって"Hello, this is sacana@xxxx"と応答があったら、コマンドを再入力してください。
1. 再起動による応答がなかった場合、syslogでエラーの詳細を確認し、修正してください。
    - 既定だと`/var/log/syslog*`にあるので、`grep sacana`で確認します。
1. 不明な（修正不可能な）エラーの場合は、issueに報告してください。

#### コマンドが `help` の場合 / コマンドが間違っている場合

DMでの `help` コマンドや、コマンドを間違えた場合の返答はホスト一覧の一番上のホストが行っています。
これらの場合はホスト一覧の一番上のホストについて確認してください。
ホスト自体は正常でもホスト一覧側の記載がタイプミスなどで間違っている可能性もあります。


### アカウントが作れない

例えば`public_key_uri_format`で指定したリンク先がGitHubの場合、Slackの表示名がGitHubのアカウント名と異なるとアカウントを作ることができません。
その場合はSlackのプロフィール設定から表示名を適切なものに設定してください。
また、GitHubに公開鍵が登録されていない場合もログインできません。
その場合はまずGitHubに公開鍵を登録した後、 `@computer-account-manager update HOSTNAME` でHOSTNAME上のアカウントの公開鍵を更新します。

## ライセンス

本プロジェクトは

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0) ([LICENSE-APACHE2.0](LICENSE-APACHE2.0))
- [MIT License](https://opensource.org/licenses/MIT) ([LICENSE-MIT](LICENSE-MIT))

のデュアルライセンスです。
