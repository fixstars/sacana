use std::io::{BufRead, Write};

use crate::runtime_error::{path_join, Result, RuntimeError};

/// uri_format が指すuriに user_id のpublic keyが存在するかどうかを判定
fn public_keys_exist(uri_format: &str, user_id: &str) -> Result<()> {
    let uri = uri_format.replace("{}", user_id);
    let response = reqwest::blocking::Client::new().head(&uri).send()?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(RuntimeError::new(format!("can't access {}: {}", uri, response.text()?)).into())
    }
}

/// uri_format が指すuriから user_id のpublic keyを取得
fn get_public_keys(uri_format: &str, user_id: &str) -> Result<String> {
    let uri = uri_format.replace("{}", user_id);
    let response = reqwest::blocking::get(&uri)?;
    if response.status().is_success() {
        Ok(response.text()?)
    } else {
        Err(RuntimeError::new(format!(
            "get public key from {} failed: {}",
            uri,
            response.text()?
        ))
        .into())
    }
}

/// /etc/passwdから user_id の行を抜き出す
fn etc_passwd(user_id: &str) -> Result<Option<String>> {
    let file = std::fs::File::open("/etc/passwd")?;
    for line in std::io::BufReader::new(&file).lines() {
        let l = line?;
        if let Some(u) = l.clone().split(':').next() {
            if u == user_id {
                return Ok(Some(l));
            }
        }
    }
    Ok(None)
}

/// etc_passwdの結果からユーザーのホームディレクトリを取得
fn home_directory(passwd_line: String) -> String {
    passwd_line.split(':').nth(5).unwrap().to_string()
}

/// ユーザーを作成
fn add_user(user_name: &str, local_host_name: &str) -> Result<()> {
    if etc_passwd(user_name)?.is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Your account already exists on {}", local_host_name),
        )
        .into());
    }
    let useradd = std::process::Command::new("useradd")
        .arg("-m")
        .arg("-s")
        .arg("/bin/bash")
        .arg("-p")
        .arg("")
        .arg(user_name)
        .output()?;
    if !useradd.status.success() {
        return Err(RuntimeError::new(format!(
            "`useradd` failed. status code: {}",
            useradd
                .status
                .code()
                .ok_or_else(|| RuntimeError::new("`useradd` is killed by signal"))?
        ))
        .into());
    }
    Ok(())
}

/// user_name の $HOME に .ssh を作成し、そのパスを取得
/// ディレクトリが既に存在した場合は特に何もせずにパスを返す
fn create_ssh_directory(user_name: &str, local_host_name: &str) -> Result<String> {
    let ep = etc_passwd(user_name)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Your account doesn't exist on {}", local_host_name),
        )
    })?;
    let hd = home_directory(ep);
    let ssh_dir = path_join(&[&hd, ".ssh"])?;
    std::fs::create_dir_all(&ssh_dir)?;
    Ok(ssh_dir)
}

/// ssh_dir/authorized_keysに uri_format で指定したURIから取得した公開鍵を上書き
fn overwrite_ssh_public_key(ssh_dir: &str, user_name: &str, uri_format: &str) -> Result<()> {
    let path = path_join(&[ssh_dir, "authorized_keys"])?;
    let keys = get_public_keys(uri_format, user_name)?;
    std::io::BufWriter::new(std::fs::File::create(path)?).write_all(&keys.into_bytes())?;
    Ok(())
}

/// ssh_dir 以下のファイルのパーミッションを700に、所有者を user_name に変更
fn set_owner_and_permission(ssh_dir: &str, user_name: &str) -> Result<()> {
    let chmod = std::process::Command::new("chmod")
        .arg("700")
        .arg(ssh_dir)
        .output()?;
    if !chmod.status.success() {
        return Err(RuntimeError::new(format!(
            "`chmod` failed. status code: {}",
            chmod
                .status
                .code()
                .ok_or_else(|| RuntimeError::new("chmod is killed by signal"))?
        ))
        .into());
    }
    let chown = std::process::Command::new("chown")
        .arg("-R")
        .arg(format!("{0}:{0}", user_name))
        .arg(ssh_dir)
        .output()?;
    if !chown.status.success() {
        return Err(RuntimeError::new(format!(
            "chown failed. status code: {}",
            chown
                .status
                .code()
                .ok_or_else(|| RuntimeError::new("chown is killed by signal"))?
        ))
        .into());
    }
    Ok(())
}

/// ユーザーのauthorized_keysを更新
pub fn update_account(user_name: &str, local_host_name: &str, uri_format: &str) -> Result<()> {
    let ssh_dir = create_ssh_directory(user_name, local_host_name)?;
    overwrite_ssh_public_key(&ssh_dir, user_name, uri_format)?;
    set_owner_and_permission(&ssh_dir, user_name)
}

/// アカウントを作成
pub fn create_account(user_name: &str, local_host_name: &str, uri_format: &str) -> Result<()> {
    public_keys_exist(uri_format, user_name)?;
    add_user(user_name, local_host_name)?;
    update_account(user_name, local_host_name, uri_format)
}

/// ユーザーをグループに追加
pub fn join_group(user_name: &str, group_name: &str, local_host_name: &str) -> Result<()> {
    etc_passwd(user_name)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Your account doesn't exist on {}", local_host_name),
        )
    })?;
    let usermod = std::process::Command::new("usermod")
        .arg("-aG")
        .arg(group_name)
        .arg(user_name)
        .output()?;
    if !usermod.status.success() {
        return Err(RuntimeError::new(format!(
            "usermod failed. status code: {}",
            usermod
                .status
                .code()
                .ok_or_else(|| RuntimeError::new("usermod is killed by signal"))?
        ))
        .into());
    }
    Ok(())
}
