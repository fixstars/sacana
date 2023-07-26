use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::runtime_error::{as_array, Result, RuntimeError};

fn post(api_token: &str, body: HashMap<&str, &str>, uri: &str) -> Result<()> {
    debug!("{:?}", body);
    let client = reqwest::blocking::Client::new()
        .post(uri)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )
        .header(
            reqwest::header::AUTHORIZATION,
            "Bearer ".to_owned() + api_token,
        )
        .body(serde_json::to_vec(&body)?)
        .send()?;
    let text = client.text()?;
    let v: serde_json::Value = text.parse()?;
    debug!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

/// channel に text を投稿する
pub fn post_message(api_token: &str, channel: &str, text: &str) -> Result<()> {
    let mut body = HashMap::new();
    body.insert("channel", channel);
    body.insert("text", text);
    body.insert("as_user", "true");
    post(api_token, body, "https://slack.com/api/chat.postMessage")
}

/// channel の ts のスレッドに text を投稿する
pub fn post_message_to_thread(api_token: &str, channel: &str, ts: &str, text: &str) -> Result<()> {
    let mut body = HashMap::new();
    body.insert("channel", channel);
    body.insert("text", text);
    body.insert("as_user", "true");
    body.insert("thread_ts", ts);
    post(api_token, body, "https://slack.com/api/chat.postMessage")
}

/// channel に user のみに見える attachments を投稿する
pub fn post_ephemeral_attachments(
    api_token: &str,
    channel: &str,
    user: &str,
    attachments: serde_json::Value,
) -> Result<()> {
    let attachments_str = attachments.to_string();
    let mut body = HashMap::new();
    body.insert("channel", channel);
    body.insert("attachments", &attachments_str);
    body.insert("user", user);
    body.insert("as_user", "true");
    post(api_token, body, "https://slack.com/api/chat.postEphemeral")
}

/// channnel の ts の投稿に reaction を付加する
pub fn add_reaction(api_token: &str, channel: &str, ts: &str, reaction: &str) -> Result<()> {
    let mut body = HashMap::new();
    body.insert("name", reaction);
    body.insert("channel", channel);
    body.insert("timestamp", ts);
    post(api_token, body, "https://slack.com/api/reactions.add")
}

/// slack.com のAPIを叩いて MAX_RETRY 回返って来なかった場合 Err を返す
pub fn try_connect_to_slack_com() -> Result<()> {
    const MAX_TRY_TIMES: usize = 5;
    const SLEEP_TIME: std::time::Duration = std::time::Duration::from_secs(10);
    for i in 0..MAX_TRY_TIMES {
        if post(
            "invalid token",
            HashMap::new(),
            "https://slack.com/api/auth.test",
        )
        .is_ok()
        {
            return Ok(());
        } else {
            warn!("connecting to slack.com failed");
        }
        if i + 1 != MAX_TRY_TIMES {
            std::thread::sleep(SLEEP_TIME);
        }
    }
    Err(RuntimeError::new("connecting to slack.com failed").into())
}

/// Real Time Messaging session を開始する
pub fn rtm_connect(api_token: &str) -> Result<serde_json::Value> {
    Ok(reqwest::blocking::Client::new()
        .post("https://slack.com/api/rtm.connect")
        .form(&[("token", api_token)])
        .send()?
        .text()?
        .parse()?)
}

#[derive(Clone, Copy, Debug)]
pub enum ChannelType {
    PublicChannel,
    PrivateChannel,
    DirectMessage,
    GroupDirectMessage,
}

fn get_channel_type_name_for_slack_api(x: ChannelType) -> &'static str {
    match x {
        ChannelType::PublicChannel => "public_channel",
        ChannelType::PrivateChannel => "private_channel",
        ChannelType::DirectMessage => "im",
        ChannelType::GroupDirectMessage => "mpim",
    }
}

/// channel がpublicチャンネル/privateチャンネル/DMのどれかを確認する
pub fn channel_type(api_token: &str, channel: &str) -> Result<ChannelType> {
    let response = &reqwest::blocking::Client::new()
        .post("https://slack.com/api/conversations.info")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded; charset=utf-8",
        )
        .form(&[("token", api_token), ("channel", channel)])
        .send()?
        .text()?
        .parse::<serde_json::Value>()?["channel"];
    debug!("{:?}", response);
    Ok(if let Some(true) = response["is_mpim"].as_bool() {
        ChannelType::GroupDirectMessage
    } else if let Some(true) = response["is_channel"].as_bool() {
        ChannelType::PublicChannel
    } else if let Some(true) = response["is_group"].as_bool() {
        ChannelType::PrivateChannel
    } else if let Some(true) = response["is_im"].as_bool() {
        ChannelType::DirectMessage
    } else {
        return Err(RuntimeError::new("invalid conversation object").into());
    })
}

fn get_users_list(
    api_token: &str,
    cursor: Option<String>,
) -> Result<(Vec<serde_json::Value>, Option<String>)> {
    let form_params = {
        let mut params = HashMap::new();
        params.insert("token", api_token);
        if let Some(ref c) = cursor {
            params.insert("cursor", c);
        }
        params.insert("limit", "200");
        params
    };
    let response = reqwest::blocking::Client::new()
        .post("https://slack.com/api/users.list")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded; charset=utf-8",
        )
        .form(&form_params)
        .send()?
        .text()?
        .parse::<serde_json::Value>()?;
    debug!("{}", serde_json::to_string_pretty(&response)?);
    Ok((
        as_array(&response["members"])?.clone(),
        response["response_metadata"]["next_cursor"]
            .as_str()
            .and_then(|s| if s.is_empty() { None } else { Some(s) })
            .map(std::string::ToString::to_string),
    ))
}

/// 全ユーザー情報を取得
pub fn users_list(api_token: &str) -> Result<Vec<serde_json::Value>> {
    let mut list = Vec::new();
    let mut cursor = None;
    loop {
        let (mut ret, next_cursor) = get_users_list(api_token, cursor)?;
        list.append(&mut ret);
        if next_cursor.is_some() {
            cursor = next_cursor;
        } else {
            break;
        }
    }
    Ok(list)
}

/// channel の last_timestamp から最新までの履歴200件を取得
pub fn conversations_history(
    api_token: &str,
    channel: &str,
    last_timestamp: &Option<String>,
) -> Result<Vec<serde_json::Value>> {
    let mut param = HashMap::new();
    param.insert("token", api_token);
    param.insert("channel", channel);
    param.insert("limit", "200");
    if let Some(x) = last_timestamp {
        param.insert("oldest", x);
    }
    Ok(as_array(
        &reqwest::blocking::Client::new()
            .post("https://slack.com/api/conversations.history")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded; charset=utf-8",
            )
            .form(&param)
            .send()?
            .text()?
            .parse::<serde_json::Value>()?["messages"],
    )?
    .clone())
}

#[derive(Serialize, Deserialize, Debug)]
struct Cursor {
    next_cursor: String,
}
#[derive(Serialize, Debug)]
struct UsersConversationsParams<'a> {
    token: &'a str,
    cursor: Option<&'a str>,
    exclude_archive: Option<bool>,
    limit: Option<u16>,
    team_id: Option<&'a str>,
    types: Option<&'a str>,
    user: Option<&'a str>,
}
impl<'a> UsersConversationsParams<'a> {
    fn new(token: &'a str) -> Self {
        UsersConversationsParams {
            token,
            cursor: None,
            exclude_archive: None,
            limit: None,
            team_id: None,
            types: None,
            user: None,
        }
    }
}
#[derive(Deserialize, Debug)]
pub struct Channel {
    pub id: String,
    pub name: String,
}
#[derive(Deserialize, Debug)]
struct UsersConversationsResponce {
    ok: bool,
    channels: Option<Vec<Channel>>,
    response_metadata: Option<Cursor>,
    error: Option<String>,
}

fn get_users_conversations(
    api_token: &str,
    cursor: Option<String>,
    exclude_archive: Option<bool>,
    types: Option<&[ChannelType]>,
    user: Option<&str>,
) -> Result<(Vec<Channel>, Option<String>)> {
    let s;
    let form_params = {
        let mut p = UsersConversationsParams::new(api_token);
        p.cursor = cursor.as_deref();
        p.exclude_archive = exclude_archive;
        p.limit = Some(200);
        if let Some(t) = types {
            let v = t
                .iter()
                .copied()
                .map(get_channel_type_name_for_slack_api)
                .collect::<Vec<_>>();
            s = v.join(",");
            p.types = Some(&s);
        }
        p.user = user;
        dbg!(p)
    };
    let UsersConversationsResponce {
        ok,
        channels,
        response_metadata,
        error,
    }
    = reqwest::blocking::Client::new()
        .get("https://slack.com/api/users.conversations")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded; charset=utf-8",
        )
        .query(&form_params)
        .send()?
        .json()?;
    if ok {
        let next_cursor = response_metadata.unwrap().next_cursor;
        if next_cursor.is_empty() {
            Ok((channels.unwrap(), None))
        } else {
            Ok((channels.unwrap(), Some(next_cursor)))
        }
    } else {
        let message = if let Some(e) = error {
            format!("API error: users.conversations failed \"{e}\"")
        } else {
            "API error: users.conversations failed".to_string()
        };
        Err(RuntimeError::new(message).into())
    }
}

/// ユーザーが属している全てのpublic channelを取得
pub fn users_public_channel_list(api_token: &str, user: Option<&str>) -> Result<Vec<Channel>> {
    let mut list = Vec::new();
    let mut cursor = None;
    loop {
        let (mut ret, next_cursor) = get_users_conversations(
            api_token,
            cursor,
            Some(true),
            Some(&[ChannelType::PublicChannel]),
            user,
        )?;
        list.append(&mut ret);
        if next_cursor.is_some() {
            cursor = next_cursor;
        } else {
            break;
        }
    }
    Ok(list)
}
