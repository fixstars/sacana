use log::{debug, error, info, trace};
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;

mod runtime_error;
use crate::runtime_error::{as_str, Result, RuntimeError};

mod slack;
use crate::slack::{
    add_reaction, channel_type, conversations_history, post_ephemeral_attachments, post_message,
    post_message_to_thread, rtm_connect, try_connect_to_slack_com, users_list,
    users_public_channel_list, ChannelType,
};

mod linux_user_manage;
use crate::linux_user_manage::{create_account, join_group, update_account};

fn to_naive_date_time(timestamp_string: &str) -> Result<chrono::NaiveDateTime> {
    Ok(chrono::NaiveDateTime::parse_from_str(
        timestamp_string,
        "%s.%f",
    )?)
}

fn to_string(timestamp: &chrono::NaiveDateTime) -> String {
    timestamp.format("%s.%f").to_string()
}

/// 実行バイナリの置かれているディレクトリを取得
fn get_module_directory() -> Result<std::path::PathBuf> {
    let mut path = std::fs::read_link(std::path::Path::new("/proc/self/exe"))?;
    path.pop();
    Ok(path)
}

/// 設定ファイルの読み込み
fn read_settings() -> Result<serde_json::Value> {
    let mut path = get_module_directory()?;
    path.push("settings.json");
    let file = std::fs::File::open(path)?;
    Ok(serde_json::from_reader(file)?)
}

fn certificate_from_pem(pem_file: &str) -> Result<reqwest::Certificate> {
    let mut buf = Vec::new();
    std::fs::File::open(pem_file)?.read_to_end(&mut buf)?;
    Ok(reqwest::Certificate::from_pem(&buf)?)
}

fn get_hosts(uri: &str, certificate_file: Option<&str>) -> Result<Vec<String>> {
    let client = if let Some(file_name) = certificate_file {
        reqwest::blocking::Client::builder()
            .add_root_certificate(certificate_from_pem(file_name)?)
            .build()?
    } else {
        reqwest::blocking::Client::new()
    };
    Ok(client
        .get(uri)
        .send()?
        .text()?
        .trim_matches('\n')
        .split('\n')
        .map(std::string::ToString::to_string)
        .collect())
}

/// mes_json が channels で指定されたチャンネルでのメッセージかつ
/// my_id で指定されたユーザー宛のメッセージかつ
/// my_id で指定されたユーザーからのメッセージでない場合
/// そのチャンネルIDを返す
fn channel_of_message_to_me_at_channels<'a>(
    mes_json: &serde_json::Value,
    my_id: &str,
    channels: &'a [String],
) -> Option<&'a String> {
    if let (Some(mes_type), Some(mes_channel), Some(mes_user), Some(mes_text)) = (
        mes_json["type"].as_str(),
        mes_json["channel"].as_str(),
        mes_json["user"].as_str(),
        mes_json["text"].as_str(),
    ) {
        if mes_type.trim_matches('"') == "message"
            && mes_user.trim_matches('"') != my_id
            && mes_text
                .trim_matches('"')
                .starts_with(&format!("<@{}>", my_id))
        {
            return channels
                .iter()
                .find(|&x| mes_channel.trim_matches('"') == x);
        }
    }
    None
}

/// mes_json がDirect Message上でのメッセージかつ
/// my_id で指定されたユーザーからのメッセージでない場合
/// trueを返す
fn is_message_at_dm(mes_json: &serde_json::Value, api_token: &str, my_id: &str) -> bool {
    if let (Some(mes_type), Some(mes_channel), Some(mes_user)) = (
        mes_json["type"].as_str(),
        mes_json["channel"].as_str(),
        mes_json["user"].as_str(),
    ) {
        match channel_type(api_token, mes_channel.trim_matches('"')) {
            Ok(ChannelType::DirectMessage) => {
                mes_type.trim_matches('"') == "message" && mes_user.trim_matches('"') != my_id
            }
            _ => false,
        }
    } else {
        false
    }
}

type WebSocket =
    tungstenite::protocol::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;
/// RTMのセットアップ
fn rtm_setup(api_token: &str) -> Result<(String, WebSocket)> {
    try_connect_to_slack_com()?;
    // RTM の URL を取得
    let v = rtm_connect(api_token)?;
    debug!("{}", serde_json::to_string_pretty(&v)?);
    debug!("{}", v["url"]);

    // 自分のIDを取得
    let my_id = as_str(&v["self"]["id"])?.trim_matches('"').to_string();
    debug!("My ID is {}.", my_id);

    // 先ほど取得したURLでRTMのクライアントを起動
    let (client, response) = tungstenite::client::connect(as_str(&v["url"])?)?;
    debug!("{response:?}");
    Ok((my_id, client))
}

/// `channel_names` で与えられたチャンネルが、公開チャンネルとして存在するか確認する
fn check_channels(api_token: &str, channel_names: &[String]) -> Result<Vec<String>> {
    // 公開チャンネルの一覧を取得
    let public_channels = users_public_channel_list(api_token, None)?
        .into_iter()
        .map(|v| (v.name, v.id))
        .collect::<HashMap<_, _>>();
    channel_names
        .iter()
        .map(|c| {
            Ok(public_channels
                .get(c)
                .ok_or_else(|| RuntimeError::new(format!("there is no channel named {}", c)))?
                .to_string())
        })
        .collect()
}

/// ユーザー一覧の取得
fn get_users(api_token: &str) -> Result<HashMap<String, String>> {
    users_list(api_token)?
        .into_iter()
        .map(|v| {
            Ok((
                as_str(&v["id"])?.to_string(),
                as_str(&v["profile"]["display_name_normalized"])?.to_string(),
            ))
        })
        .collect()
}

const ATTRIBUTE_NUMS: usize = 6usize;
fn make_hostname_field(what: &str) -> serde_json::Map<String, serde_json::Value> {
    vec![
        ("title".to_string(), "_HOSTNAME_".into()),
        (
            "value".to_string(),
            format!(
                "The host name on which you want to {} (see below _HOSTNAME_ list)",
                what
            )
            .into(),
        ),
    ]
    .into_iter()
    .collect()
}

fn make_available_channel_field(
    channel_name: String,
) -> serde_json::Map<String, serde_json::Value> {
    vec![
        ("title".to_string(), "available on".into()),
        ("value".to_string(), channel_name.into()),
    ]
    .into_iter()
    .collect()
}

fn format_fields(
    fields: &[serde_json::Map<String, serde_json::Value>],
) -> Vec<serde_json::Map<String, serde_json::Value>> {
    let size = if fields.len() % 2 == 0 {
        fields.len()
    } else {
        fields.len() - 1
    };
    let mut fs = fields.to_owned();
    for x in fs.iter_mut().take(size) {
        x.insert("short".to_string(), "true".into());
    }
    fs
}

fn make_head_lower(text: &str) -> String {
    text.chars()
        .by_ref()
        .take(1)
        .map(|x| x.to_ascii_lowercase())
        .chain(text.chars().by_ref().skip(1))
        .collect()
}

enum DescriptionOrList<'a> {
    Description(&'a str),
    List(&'a [String]),
}

type AttributeEntry<'a> = (
    &'a str,
    &'a DescriptionOrList<'a>,
    Option<&'a [serde_json::Map<String, serde_json::Value>]>,
);

fn make_attributes(data: [AttributeEntry; ATTRIBUTE_NUMS]) -> serde_json::Value {
    const COLORS: [&str; ATTRIBUTE_NUMS] = [
        "#007dc6", "#ed1b23", "#fdb811", "#71bf44", "#00a650", "#6c6e71",
    ];
    data.iter()
        .zip(COLORS.iter())
        .map(|(&(text, desc_or_list, fields), &c)| {
            let mut attribute = serde_json::Map::new();
            attribute.insert("color".to_string(), c.to_string().into());
            match desc_or_list {
                DescriptionOrList::Description(d) => {
                    attribute.insert("text".to_string(), [text, d].join("\n").into());
                    attribute.insert(
                        "fallback".to_string(),
                        [text.to_string(), make_head_lower(d)].join(": ").into(),
                    );
                }
                DescriptionOrList::List(l) => {
                    attribute.insert(
                        "text".to_string(),
                        [text.to_string(), l.join("\n    ")].join("\n    ").into(),
                    );
                    attribute.insert(
                        "fallback".to_string(),
                        [text.to_string(), l.join(", ")].join(": ").into(),
                    );
                }
            }
            if let Some(f) = fields {
                attribute.insert("fields".to_string(), format_fields(f).into());
                attribute.insert("mrkdwn_in".to_string(), json!(["text", "fields"]));
            } else {
                attribute.insert("mrkdwn_in".to_string(), json!(["text"]));
            }
            attribute.into()
        })
        .collect::<Vec<serde_json::Value>>()
        .into()
}

/// helpメッセージの生成
fn make_help_message(
    my_id: &str,
    channels: &[String],
    uri: &str,
    hosts: &[String],
) -> serde_json::Value {
    let channels_names = channels
        .iter()
        .map(|x| format!("<#{}>", x))
        .collect::<Vec<_>>()
        .join(", ");
    let dm = format!("DM(<@{}>)", my_id);
    let channels_and_dm =
        channels_names.clone() + if channels_names.is_empty() { "" } else { ", " } + &dm;
    make_attributes([
        (
            &format!("*<@{}> create _HOSTNAME_*", my_id),
            &DescriptionOrList::Description("Creates you an account on _HOSTNAME_"),
            Some(&[
                make_hostname_field("create your account"),
                make_available_channel_field(channels_names.clone()),
            ]),
        ),
        (
            &format!("*<@{}> update _HOSTNAME_*", my_id),
            &DescriptionOrList::Description(&format!("Retrieves all public keys from `{}` and add them to `$HOME/.ssh/authorized_keys` (this command *WILL OVERWRITE* your `$HOME/.ssh/authorized_keys`)", uri)),
            Some(&[
                make_hostname_field("update your `authorized_keys`"),
                make_available_channel_field(channels_names.clone())
            ])
        ),
        (
            &format!("*<@{}> join _GROUPNAME_ _HOSTNAME_*", my_id),
            &DescriptionOrList::Description("Join _GROUPNAME_ group on _HOSTNAME_"),
            Some(&[
                vec![("title".to_string(), "_GROUPNAME_".into()),
                     ("value".to_string(), "The group name which you want to join on _HOSTNAME_ . You can check the available groups on _HOSTNAME_ using `cat /etc/groups` .".into())]
                    .into_iter()
                    .collect(),
                make_hostname_field("join the group"),
                make_available_channel_field(channels_names)
            ])
        ),
        (
            &format!("*<@{}> ping*", my_id),
            &DescriptionOrList::Description("Get pongs from alive bots"),
            Some(&[make_available_channel_field(channels_and_dm)]),
        ),
        (
            &format!("*<@{}> help*", my_id),
            &DescriptionOrList::Description("Shows this message"),
            Some(&[make_available_channel_field(dm)]),
        ),
        ("*_HOSTNAME_ list*", &DescriptionOrList::List(hosts), None)
    ])
}

struct CommandHandler {
    pic_of_response: bool,
    api_token: String,
    local_host_name: String,
    hosts: Vec<String>,
    channels: Vec<String>,
    users: HashMap<String, String>,
    my_id: String,
    uri_format: String,
    last_timestamp: Option<chrono::NaiveDateTime>,
}

impl CommandHandler {
    /// Slackに起動報告をする
    fn report_startup(&self) -> Result<()> {
        for channel in &self.channels {
            post_message(
                &self.api_token,
                channel,
                &format!("Hello, this is sacana@{}.", self.local_host_name),
            )?;
        }
        Ok(())
    }

    /// コマンド列が不正
    fn invalid_command_sequence(
        &self,
        user_id: &str,
        channel: &str,
        timestamp: &str,
    ) -> Result<()> {
        if self.pic_of_response {
            post_message(
                &self.api_token,
                channel,
                &format!("<@{}> Invalid command sequence.", user_id),
            )?;
            add_reaction(&self.api_token, channel, timestamp, "x")?;
            self.help(user_id, channel, timestamp, false)?;
        }
        Ok(())
    }
    /// コマンドの実行結果をログやSlackに出力
    fn handle_command_result(
        &self,
        user_id: &str,
        channel: &str,
        timestamp: &str,
        result: Result<()>,
        info_message: &str,
        slack_message: &str,
    ) -> Result<()> {
        if let Err(e) = result {
            post_message(&self.api_token, channel, &format!("<@{}> {}", user_id, e))?;
            add_reaction(&self.api_token, channel, timestamp, "x")
        } else {
            info!("{}", info_message);
            post_message(
                &self.api_token,
                channel,
                &format!("<@{}> {}", user_id, slack_message),
            )?;
            add_reaction(&self.api_token, channel, timestamp, "o")
        }
    }

    /// help
    fn help(&self, user_id: &str, channel: &str, timestamp: &str, check: bool) -> Result<()> {
        if self.pic_of_response {
            post_ephemeral_attachments(
                &self.api_token,
                channel,
                user_id,
                make_help_message(
                    &self.my_id,
                    &self.channels,
                    &self.uri_format.replace("{}", &self.users[user_id]),
                    &self.hosts,
                ),
            )?;
            if check {
                add_reaction(&self.api_token, channel, timestamp, "ballot_box_with_check")?
            }
        }
        Ok(())
    }
    /// ping
    fn ping(&self, channel: &str, timestamp: &str) -> Result<()> {
        post_message_to_thread(
            &self.api_token,
            channel,
            timestamp,
            &format!("pong@{}", self.local_host_name),
        )
    }
    /// create
    fn create(&self, user_id: &str, user_name: &str, channel: &str, timestamp: &str) -> Result<()> {
        self.handle_command_result(
            user_id,
            channel,
            timestamp,
            create_account(user_name, &self.local_host_name, &self.uri_format),
            &format!("{} create account", user_name),
            "creating account is succeeded.",
        )
    }
    /// update
    fn update(&self, user_id: &str, user_name: &str, channel: &str, timestamp: &str) -> Result<()> {
        self.handle_command_result(
            user_id,
            channel,
            timestamp,
            update_account(user_name, &self.local_host_name, &self.uri_format),
            &format!("{} update keys", user_name),
            "updating key is succeeded.",
        )
    }
    /// join
    fn join(
        &self,
        user_id: &str,
        user_name: &str,
        channel: &str,
        timestamp: &str,
        group_name: &str,
    ) -> Result<()> {
        self.handle_command_result(
            user_id,
            channel,
            timestamp,
            join_group(user_name, group_name, &self.local_host_name),
            &format!("{} joined {} group.", user_name, group_name),
            &format!("joined {} group.", group_name),
        )
    }
    /// ホスト名チェック
    fn check_host_name(
        &self,
        user_id: &str,
        channel: &str,
        timestamp: &str,
        hostname: Option<&&str>,
    ) -> Result<bool> {
        if self.pic_of_response {
            match hostname {
                None => {
                    post_message(
                        &self.api_token,
                        channel,
                        &format!("<@{}> Invalid hostname.", user_id),
                    )?;
                    add_reaction(&self.api_token, channel, timestamp, "x")?;
                    self.help(user_id, channel, timestamp, false)?;
                }
                Some(name) => {
                    if !self.hosts.iter().any(|x| x == name) {
                        post_message(
                            &self.api_token,
                            channel,
                            &format!("<@{}> Invalid hostname '{}'.", user_id, name),
                        )?;
                        add_reaction(&self.api_token, channel, timestamp, "x")?;
                        self.help(user_id, channel, timestamp, false)?;
                    }
                }
            }
        }
        Ok(hostname.is_some() && hostname.unwrap() == &self.local_host_name)
    }

    fn dm(&self, mes_json: serde_json::Value) -> Result<Option<chrono::NaiveDateTime>> {
        debug!("{}", serde_json::to_string_pretty(&mes_json)?);
        let raw_message = as_str(&mes_json["text"])?;
        debug!("Raw message:\n{}", raw_message);
        let channel = as_str(&mes_json["channel"])?;
        let user_id = as_str(&mes_json["user"])?;
        let timestamp = as_str(&mes_json["ts"])?;
        let splitted_messages: Vec<&str> = if raw_message
            .trim_matches('"')
            .starts_with(&format!("<@{}>", self.my_id))
        {
            //DMの場合かつリプライの場合、「@<bot名>」以後の入力を受け取る
            raw_message.split_whitespace().skip(1).collect()
        } else {
            //DMの場合かつリプライではない場合、入力を全て受け取る
            raw_message.split_whitespace().collect()
        };
        match (
            splitted_messages.first(),
            splitted_messages.len(),
            self.pic_of_response,
        ) {
            (Some(&"help"), 1, true) => self.help(user_id, channel, timestamp, true)?,
            (Some(&"ping"), 1, _) => self.ping(channel, timestamp)?,
            (_, _, true) => self.invalid_command_sequence(user_id, channel, timestamp)?,
            (_, _, false) => return Ok(None),
        }
        Ok(Some(to_naive_date_time(timestamp)?))
    }

    fn message(&self, mes_json: serde_json::Value) -> Result<Option<chrono::NaiveDateTime>> {
        if is_message_at_dm(&mes_json, &self.api_token, &self.my_id) {
            return self.dm(mes_json);
        }
        let channel = if let Some(x) =
            channel_of_message_to_me_at_channels(&mes_json, &self.my_id, &self.channels)
        {
            x
        } else {
            return Ok(None);
        };
        debug!("{}", serde_json::to_string_pretty(&mes_json)?);
        let raw_message = as_str(&mes_json["text"])?;
        debug!("Raw message:\n{}", raw_message);
        let user_id = as_str(&mes_json["user"])?;
        let timestamp = as_str(&mes_json["ts"])?;
        let splitted_messages: Vec<&str> = raw_message.split_whitespace().skip(1).collect();
        match (splitted_messages.first(), splitted_messages.len()) {
            (Some(&"help"), 1) => {
                if self.pic_of_response {
                    post_message(
                        &self.api_token,
                        channel,
                        &format!("<@{}> please type `help` at Direct Message to me.", user_id),
                    )?;
                    add_reaction(&self.api_token, channel, timestamp, "exclamation")?
                }
            }
            (Some(&"ping"), 1) => self.ping(channel, timestamp)?,
            (Some(&"create"), 2) => {
                if self.check_host_name(user_id, channel, timestamp, splitted_messages.last())? {
                    self.create(user_id, &self.users[user_id], channel, timestamp)?
                }
            }
            (Some(&"update"), 2) => {
                if self.check_host_name(user_id, channel, timestamp, splitted_messages.last())? {
                    self.update(user_id, &self.users[user_id], channel, timestamp)?
                }
            }
            (Some(&"join"), 3) => {
                if self.check_host_name(user_id, channel, timestamp, splitted_messages.last())? {
                    self.join(
                        user_id,
                        &self.users[user_id],
                        channel,
                        timestamp,
                        splitted_messages[1],
                    )?
                }
            }
            _ => self.invalid_command_sequence(user_id, channel, timestamp)?,
        }
        Ok(Some(to_naive_date_time(timestamp)?))
    }

    /// RTMの一部のイベントを処理
    fn handle_events(&mut self, mes_json: &serde_json::Value) -> Result<bool> {
        Ok(if let Some(mes_type) = mes_json["type"].as_str() {
            match mes_type.trim_matches('"') {
                "message" => false,
                "hello" => true,
                "goodbye" => {
                    return Err(
                        RuntimeError::new("goodbye event was caught. try to reconenct...").into(),
                    )
                }
                "user_change" => {
                    if let Some(x) = self.users.get_mut(as_str(&mes_json["user"]["id"])?) {
                        *x = as_str(&mes_json["user"]["profile"]["display_name_normalized"])?
                            .to_string();
                        return Ok(true);
                    }
                    self.users.insert(
                        as_str(&mes_json["user"]["id"])?.to_string(),
                        as_str(&mes_json["user"]["profile"]["display_name_normalized"])?
                            .to_string(),
                    );
                    true
                }
                "team_join" => {
                    self.users.insert(
                        as_str(&mes_json["user"]["id"])?.to_string(),
                        as_str(&mes_json["user"]["profile"]["display_name_normalized"])?
                            .to_string(),
                    );
                    true
                }
                "user_typing" => true,
                "desktop_notification" => true,
                _ => false,
            }
        } else {
            return Err(RuntimeError::new("receive non-event object on RTM").into());
        })
    }

    fn update_timestamp(&mut self, timestamp: Option<chrono::NaiveDateTime>) -> Result<()> {
        if let Some(x) = timestamp {
            let mut flag = false;
            if let Some(sx) = self.last_timestamp {
                if sx < x {
                    flag = true
                }
            } else {
                flag = true;
            }
            if flag {
                self.last_timestamp = Some(x);
            }
        }
        Ok(())
    }

    fn on_text(&mut self, text: String) -> Result<()> {
        let mes_json_ = text.parse::<serde_json::Value>();
        match mes_json_ {
            Ok(mes_json) => {
                if let Ok(pretty_string) = serde_json::to_string_pretty(&mes_json) {
                    debug!("{}", pretty_string);
                }
                if self.handle_events(&mes_json)? {
                    return Ok(());
                }
                if self.last_timestamp.is_none() {
                    if let Some(x) = mes_json["ts"].as_str() {
                        let _ = self.update_timestamp(Some(to_naive_date_time(x)?));
                    }
                }
                let result = self.message(mes_json);
                match result {
                    Ok(res) => self.update_timestamp(res)?,
                    Err(e) => error!("{}", e),
                }
            }
            Err(e) => error!("{}", e),
        }
        Ok(())
    }

    fn handle_messages_while_dead(&mut self) -> Result<()> {
        let mut timestamps = Vec::new();
        for channel in &self.channels {
            let messages = conversations_history(
                &self.api_token,
                channel,
                &self.last_timestamp.as_ref().map(to_string),
            )?;
            let last_timestamp = if let Some(x) = messages.first() {
                Some(to_naive_date_time(as_str(&x["ts"])?)?)
            } else {
                None
            };
            for mut message in messages.into_iter().rev() {
                message["channel"] = json!(&channel);
                let _ = self.message(message).map_err(|e| error!("{}", e));
            }
            timestamps.push(last_timestamp);
        }
        for timestamp in timestamps {
            self.update_timestamp(timestamp)?;
        }
        Ok(())
    }
}

fn main() {
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info");
    env_logger::Builder::from_env(env).init();
    let settings: serde_json::Value = read_settings().unwrap();
    let api_token: String = settings["SLACK_API_TOKEN"].as_str().unwrap().to_string();
    // 設定ファイルまたは uname の実行結果から local_host_name を取得
    let local_host_name: String = if let Some(x) = settings["hostname"].as_str() {
        x.to_string()
    } else if let Ok(x) = std::process::Command::new("uname").arg("-n").output() {
        std::str::from_utf8(&x.stdout).unwrap().trim().to_string()
    } else {
        panic!("`uname -n` can't be executed");
    };
    let channel_names = settings["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let uri_format = settings["public_key_uri_format"]
        .as_str()
        .unwrap()
        .to_string();
    // 端末ホスト名のリスト(先頭の端末はホスト名が不正な場合のエラーメッセージ返答をする)
    let hosts = get_hosts(
        settings["host_list_uri"].as_str().unwrap(),
        settings["certificate_file"].as_str(),
    )
    .unwrap();
    debug!("hosts = {:?}", hosts);
    let (my_id, mut client) = rtm_setup(&api_token).unwrap();
    let mut command_handler = CommandHandler {
        pic_of_response: hosts[0] == local_host_name,
        api_token: api_token.clone(),
        local_host_name,
        hosts,
        channels: check_channels(&api_token, &channel_names).unwrap(),
        users: get_users(&api_token).unwrap(),
        my_id,
        uri_format,
        last_timestamp: None,
    };
    command_handler.report_startup().unwrap();

    info!("poling started");
    // メッセージのポーリング
    loop {
        let _ = || -> Result<()> {
            loop {
                let message = client.read();
                let m = message?;
                trace!("Recv: {:?}", m);
                use tungstenite::protocol::Message::*;
                match m {
                    Text(s) => command_handler.on_text(s)?,
                    Binary(_) => debug!("get binary"),
                    Close(_) => debug!("get closure"),
                    Ping(ping) => {
                        debug!("Ping");
                        let pong = Pong(ping);
                        debug!("Send {:?}", pong);
                        client.send(pong)?;
                    }
                    Pong(_) => debug!("Pong"),
                    Frame(x) => debug!("Frame({x:?})"),
                }
            }
        }()
        .map_err(|e| info!("{}", e));
        let (my_id, new_client) = rtm_setup(&api_token).unwrap();
        command_handler.my_id = my_id;
        client = new_client;
        command_handler.channels = check_channels(&api_token, &channel_names).unwrap();
        command_handler.users = get_users(&api_token).unwrap();
        command_handler.handle_messages_while_dead().unwrap();
        info!("poling restarted");
    }
}
