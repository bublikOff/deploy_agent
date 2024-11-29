mod git_repo;
mod temp_dir;

use std::str::FromStr;
use reqwest::Client;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use temp_dir::TempDir;
use git_repo::GitRepo;

#[tokio::main]
async fn main() {

    // Reporting that agent is trying to start
    println!("[{}] Starting deploy agent ...", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"));

    // Fetching settings for Telegram bot
    let bot_token = std::env::var("BOT_TOKEN").unwrap_or_default().trim_matches('"').to_string();
    let bot_chat_id = match i64::from_str(std::env::var("BOT_CHAT_ID").unwrap_or_default().trim_matches('"')) {
        Ok(chat_id) => chat_id,
        Err(_) => {
            eprintln!("[{}] Failed to parse Telegram BOT_CHAT_ID: {:?}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), std::env::var("BOT_CHAT_ID").unwrap_or_default().trim_matches('"'));
            std::process::exit(1);
        }
    };

    // Checking the bot token format for errors
    if telegram_validate_bot_token(&bot_token) == false {
        eprintln!("[{}] Incorrect Telegram BOT_TOKEN provided: {:?}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), bot_token);
        std::process::exit(1);
    }

    // The list of repositories we are going to monitor
    let mut git_repos: HashMap<String, GitRepo> = HashMap::new();

    // Check up to GIT_REPO50 environment variable (or any number you need)
    for i in 1..=50 {
        match std::env::var(format!("GIT_REPO{}", i)) {
            Ok(git_url) => {
                if let Some(git_repo) = GitRepo::from_url(&git_url) {
                    println!("[{}] Git repository {:?} will be monitored", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), git_url);
                    git_repos.insert(git_url, git_repo);
                } else {
                    break;
                }
            },
            _ => {
                break;
            }
        };
    }

    // Ensuring at least one repository is specified
    if git_repos.len() == 0 {
        eprintln!("[{}] No Git repositories provided", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"));
        std::process::exit(1);
    }

    //
    let host = match hostname::get() {
        Ok(hostname) => hostname.to_str().unwrap().to_string(),
        Err(_) => "unknown".to_string(),
    };

    //
    let client = Client::new();
    let deploy_lock = Arc::new(Mutex::new(()));
    let mut last_update_id = 0;

    //
    let task = tokio::spawn(async move {

        //
        println!("[{}] Initializing Telegram bot ...", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"));

        // If the event number is zero, simply use the latest event number as the starting position
        if last_update_id == 0 {
            loop {

                // Trying to retrieve the latest identifier
                last_update_id = telegram_get_latest_update_id(&client, &bot_token).await;

                // If the last_update_id is 0 or greater, consider the connection to Telegram services successful; otherwise, retry in a loop until successful
                match last_update_id >= 0 {
                    true => break,
                    false => {
                        eprintln!("[{}] Failed to retrieve the latest update ID. Retrying in 30 seconds...", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"));
                        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await
                    }
                };
            }

        }

        // Reporting that agent started succesfully
        println!("[{}] Deploy agent started successfully", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"));

        //
        loop {
            if let Ok(updates) = telegram_get_updates(&client, &bot_token, last_update_id, tokio::time::Duration::from_secs(30)).await {
                for update in updates {

                    match process_update(update, bot_chat_id) {

                        //
                        Some((update_id, Some((git_repo_name, git_repo_branch_name, git_repo_commit_id, _git_commit_message)))) => {

                            //
                            last_update_id = update_id + 1;

                            // Verifying that we are monitoring this repository
                            if let Some(git_repo) = find_repo_branch(&git_repos, &git_repo_name, &git_repo_branch_name) {

                                //
                                println!("[{}] Received Git repository push event \"{}:{}@{}\"", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), git_repo_name, git_repo_branch_name, git_repo_commit_id);

                                // Trying to reserve the ability to start the deploy process
                                let lock = deploy_lock.lock().await;

                                println!("--- Deployment of \"{}:{}@{}\" started --- \n\n", git_repo_name, git_repo_branch_name, git_repo_commit_id);
                                telegram_send_message(&client, &bot_token, bot_chat_id, format!("Deployment of \"{}:{}@{}\" started on {}", git_repo_name, git_repo_branch_name, git_repo_commit_id, host).as_str()).await;

                                // Creating a temporary directory
                                let temp_dir_repo_name = git_repo_name.replace('/', ".");
                                let temp_dir = TempDir::new(&temp_dir_repo_name, &git_repo_commit_id).unwrap();

                                //
                                let git_clone_command = format!(
                                    "git clone --filter=blob:none --no-checkout --branch {} --single-branch {}/{}.git {} && \
                                    cd {} && \
                                    git sparse-checkout init --no-cone && \
                                    git sparse-checkout set deploy && \
                                    git checkout",
                                    git_repo_branch_name,
                                    git_repo.url.clone(),
                                    git_repo_name,
                                    temp_dir.path().to_str().unwrap(),
                                    temp_dir.path().to_str().unwrap()
                                );

                                // Launching the command to clone data from the repository. We need to clone deploy folder
                                let status = if std::env::consts::OS == "windows" {
                                    std::process::Command::new("cmd").arg("/C").arg(git_clone_command)
                                        // .stdout(std::process::Stdio::null())
                                        // .stderr(std::process::Stdio::null())
                                        .status()
                                        .unwrap()
                                }
                                else {
                                    std::process::Command::new("bash").arg("-c").arg(git_clone_command)
                                        // .stdout(std::process::Stdio::null())
                                        // .stderr(std::process::Stdio::null())
                                        .status()
                                        .unwrap()
                                };

                                // Verifying that the deploy folder cloning from the repository was successful
                                if !status.success() {

                                    println!("\n\n--- Deployment of \"{}:{}@{}\" finished ---", git_repo_name, git_repo_branch_name, git_repo_commit_id);
                                    telegram_send_message(&client, &bot_token, bot_chat_id, format!("Deployment of \"{}:{}@{}\" finished on {}", git_repo_name, git_repo_branch_name, git_repo_commit_id, host).as_str()).await;

                                    println!("[{}] Failed to execute clone for \"{}:{}@{}\"", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), git_repo_name, git_repo_branch_name, git_repo_commit_id);
                                    telegram_send_message(&client, &bot_token, bot_chat_id, format!("Failed to execute \"{}:{}@{}\" clone on {}", git_repo_name, git_repo_branch_name, git_repo_commit_id, host).as_str()).await;

                                    drop(lock);
                                    continue;
                                }

                                // Building the path to deploy folder and deploy file name (depends from OS)
                                let deploy_file = format!("deploy.{}", match std::env::consts::OS == "windows" { true => "cmd", false => "sh" });
                                let deploy_directory = temp_dir.path().join("deploy/");

                                // Checking for the deploy folder and the deploy file
                                if deploy_directory.exists() && deploy_directory.join(deploy_file.clone()).exists() {

                                    // Trying to execute the deploy script
                                    let status = if std::env::consts::OS == "windows" {
                                        std::process::Command::new("cmd")
                                            .arg("/C").arg(".\\deploy.cmd")
                                            .current_dir(deploy_directory)
                                            // .stdout(std::process::Stdio::null())
                                            // .stderr(std::process::Stdio::null())
                                            .status()
                                            .unwrap()
                                    } else {
                                        std::process::Command::new("bash")
                                            .arg("-c").arg("chmod +x ./deploy.sh && ./deploy.sh")
                                            .current_dir(deploy_directory)
                                            // .stdout(std::process::Stdio::null())
                                            // .stderr(std::process::Stdio::null())
                                            .status()
                                            .unwrap()
                                    };

                                    //
                                    println!("\n\n--- Deployment of \"{}:{}@{}\" finished ---", git_repo_name, git_repo_branch_name, git_repo_commit_id);
                                    telegram_send_message(&client, &bot_token, bot_chat_id, format!("Deployment of \"{}:{}@{}\" finished on {}", git_repo_name, git_repo_branch_name, git_repo_commit_id, host).as_str()).await;

                                    //
                                    if !status.success() {

                                        eprintln!("[{}] Failed to execute deployment file ({}) for \"{}:{}@{}\"", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), deploy_file, git_repo_name, git_repo_branch_name, git_repo_commit_id);
                                        telegram_send_message(&client, &bot_token, bot_chat_id, format!("Failed to execute deplay file ({}) for \"{}:{}@{}\" on {}", deploy_file, git_repo_name, git_repo_branch_name, git_repo_commit_id, host).as_str()).await;

                                        // Releasing the deploy lock
                                        drop(lock);
                                        continue;
                                    }

                                } else {
                                    println!("\n\n--- Deployment of \"{}:{}@{}\" finished ---", git_repo_name, git_repo_branch_name, git_repo_commit_id);
                                    telegram_send_message(&client, &bot_token, bot_chat_id, format!("Deployment of \"{}:{}@{}\" finished on {}", git_repo_name, git_repo_branch_name, git_repo_commit_id, host).as_str()).await;

                                    eprintln!("[{}] Repository \"{}:{}@{}\" has no deployment file {:?}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), git_repo_name, git_repo_branch_name, git_repo_commit_id, deploy_file);
                                    telegram_send_message(&client, &bot_token, bot_chat_id, format!("Repository \"{}:{}@{}\" has no deplay file {:?} on {}", git_repo_name, git_repo_branch_name, git_repo_commit_id, deploy_file, host).as_str()).await;
                                }

                                // Releasing the deploy lock
                                drop(lock);
                            }
                        },

                        // There is a message in the channel, but it's not from the git server
                        Some((update_id, None)) => {
                            last_update_id = update_id + 1;
                        },

                        // Something went wrong
                        None => { }
                    };
                }
            }

            // Sleeping for 1 second to avoid spamming Telegram services
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await
        }

    });

    //
    if cfg!(target_os = "linux") {

        println!("[{}] Waiting for Ctrl+C or SIGTERM signal to stop service", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"));

        #[cfg(target_os = "linux")]
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();

        #[cfg(target_os = "linux")]
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap();

        #[cfg(target_os = "linux")]
        tokio::select! {
            _ = sigterm.recv() => { },
            _ = sigint.recv() => { },
            _ = tokio::signal::ctrl_c() => {},
        };
    }

    //
    else {
        println!("[{}] Waiting for Ctrl+C to stop service", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"));
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
        };
    }

    //
    task.abort();

}

//
async fn telegram_get_updates (client: &Client, bot_token: &str, offset: i64, timeout: tokio::time::Duration) -> Result<Vec<Value>, reqwest::Error> {

    let url = format!("https://api.telegram.org/bot{}/getUpdates", bot_token);
    let response = client
        .post(&url)
        .json(&serde_json::json!({ "offset": offset, "timeout": timeout.as_secs() }))
        .send()
        .await?
        .json::<Value>()
        .await?;

    if let Some(result) = response.get("result") {
        Ok(result.as_array().unwrap_or(&vec![]).to_vec())
    } else {
        Ok(vec![])
    }
}

//
async fn telegram_get_latest_update_id (client: &Client, bot_token: &str) -> i64 {

    let mut last_update_id = 0;

    loop {
        match telegram_get_updates(client, bot_token, last_update_id, tokio::time::Duration::from_secs(3)).await {
            Ok(updates) if !updates.is_empty() => {
                last_update_id = updates.last().map(|update| update["update_id"].as_i64().unwrap_or(0) + 1).unwrap_or(0);
            }
            Ok(_) => {
                break
            },
            Err(e) => {
                last_update_id = -1;
                println!("[{}] Failed to drain old Telegram updates: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), e);
                break;
            }
        }
    }

    last_update_id

}

// Function to send a message to a Telegram channel
async fn telegram_send_message (client: &Client, bot_token: &str, bot_chat_id: i64, message: &str) {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let _response = client
        .post(&url)
        .json(&json!({ "chat_id": bot_chat_id, "text": message }))
        .send()
        .await;
}

//
fn telegram_validate_bot_token (token: &str) -> bool {
    let re = regex::Regex::new(r"^\d{8,}:[A-Za-z0-9_-]{35}$").unwrap();
    re.is_match(token)
}

//
fn process_update (update: Value, chat_id: i64) -> Option<(i64, Option<(String, String, String, String)>)> {

    let update_id = update["update_id"].as_i64()?;
    let channel_post = update["channel_post"].as_object()?;

    if channel_post["chat"]["id"].as_i64()? != chat_id {
        return Some((update_id, None));
    }

    if let Some(text) = channel_post["text"].as_str() {

        let pattern = r#"\[(.*?\/.*?):(.*?)\] (\d+) new commit\n\[(.*?)\] (.*)"#;
        let re = regex::Regex::new(pattern).unwrap();

        if let Some(captures) = re.captures(text) {
            let repo_name = captures.get(1).map_or("", |m| m.as_str()).to_string();
            let branch_name = captures.get(2).map_or("", |m| m.as_str()).to_string();
            let commit_id = captures.get(4).map_or("", |m| m.as_str()).to_string();
            let commit_message = captures.get(5).map_or("", |m| m.as_str()).to_string();

            return Some((update_id, Some((repo_name, branch_name, commit_id, commit_message))));
        }
    }

    Some((update_id, None))

}

// Function to search for a repo and branch in the git_repos HashMap
fn find_repo_branch<'a>(git_repos: &'a HashMap<String, GitRepo>, requested_repo: &'a str, requested_branch: &'a str) -> Option<&'a GitRepo> {
    git_repos.values().find(|repo| repo.repo == requested_repo && repo.branch == requested_branch)
}