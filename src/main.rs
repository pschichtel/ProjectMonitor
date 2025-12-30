extern crate core;

use crate::email::TransportSecurity::StartTls;
use crate::email::{create_email_client, send_email, EmailContext, TransportSecurity};
use crate::github::{Project, Task, TaskType};
use core::time::Duration;
use github::GithubClientContext;
use lettre::transport::smtp::SUBMISSION_PORT;
use lettre::Address;
use std::cmp::Reverse;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Seek, SeekFrom};
use std::process::exit;
use tokio::signal::unix::{signal, SignalKind};
use tokio::{select, task};

mod github;
mod email;
mod error;

struct ResultingTasks {
    new_known: Vec<Project>,
    notify: Vec<Project>,
}

fn read_secret(name: &str) -> Option<String> {
    let direct_env_name = name.to_uppercase();
    match std::env::var(&direct_env_name) {
        Ok(result) => { return Some(result) },
        Err(_) => {},
    };

    let file_env_name = format!("{}_FILE", direct_env_name);
    match std::env::var(file_env_name) {
        Ok(result) => match fs::read_to_string(result) {
            Ok(file_content) => { return Some(file_content) }
            Err(_) => { }
        }
        Err(_) => {}
    };

    match fs::read_to_string(format!("/run/secrets/{}", name)) {
        Ok(file_content) => { return Some(file_content) }
        Err(_) => {}
    }

    None
}

fn read_required_secret(name: &str) -> String {
    match read_secret(name) {
        Some(value) => value,
        None => {
            println!("Failed to read required secret {}!", name);
            exit(1);
        },
    }
}

fn read_known_tasks(file: &mut File) -> Result<Vec<Project>, Box<dyn Error>> {
    let size = file.metadata()?.len();
    if size == 0 {
        return Ok(Vec::new());
    }
    file.seek(SeekFrom::Start(0))?;
    match serde_json::from_reader::<_, Vec<Project>>(BufReader::new(file)) {
        Ok(data) => Ok(data),
        Err(e) => {
            eprintln!("Failed to parse known tasks: {}", e);
            Ok(Vec::new())
        },
    }
}

fn write_known_tasks(file: &mut File, projects: &Vec<Project>) -> Result<(), Box<dyn Error>> {
    file.seek(SeekFrom::Start(0))?;

    file.set_len(0)?;
    serde_json::to_writer_pretty(BufWriter::new(file), &projects)?;
    Ok(())
}

async fn find_issues_for_notification(github_context: &GithubClientContext, retain_for: Duration, email_context: &mut EmailContext, persistence_path: &str) -> Result<(), Box<dyn Error>> {
    let mut file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(persistence_path)?;

    file.lock()?;

    let known_tasks = read_known_tasks(&mut file)?;
    let ResultingTasks { new_known,  notify } = check_tasks_against_persistence(&github_context, retain_for, &known_tasks).await?;
    // try notifying before writing the known tasks out, otherwise failed notifications will not be reattempted
    notify_about_tasks(&notify, email_context)?;
    if known_tasks != new_known {
        write_known_tasks(&mut file, &new_known)?;
    }

    file.unlock().expect("failed to unlock persistence file!");
    Ok(())
}

fn lookup_project<'a, 'b>(tasks: &'a mut Vec<Project>, subject: &'b Project) -> Option<&'a mut Project> {
    for project in tasks.iter_mut() {
        if project.url == subject.url {
            return Some(project);
        }
    }
    None
}

fn lookup_task<'a, 'b>(project: &'a mut Project, subject: &'b Task) -> Option<&'a mut Task> {
    for task in project.tasks.iter_mut() {
        if task.url == subject.url {
            return Some(task);
        }
    }
    None
}

fn upsert_task<'a>(tasks: &'a mut Vec<Project>, project: &Project, task: &Task) -> bool {
    let project = match lookup_project(tasks, project) {
        Some(project) => project,
        None => {
            let mut project_clone = (*project).clone();
            project_clone.tasks.retain(|t| t.url == task.url);
            tasks.push(project_clone);
            return true;
        }
    };
    if lookup_task(project, task).is_none() {
        project.tasks.push(task.clone());
        return true;
    }
    false
}

async fn check_tasks_against_persistence(github_context: &GithubClientContext, retain_for: Duration, known_tasks: &Vec<Project>) -> Result<ResultingTasks, Box<dyn Error>> {
    let now = chrono::Utc::now();
    let mut known_tasks = known_tasks.clone();
    let mut all_tasks = github::fetch_all_projects(&github_context).await?;
    let mut notify_tasks: Vec<Project> = Vec::new();

    known_tasks.retain_mut(|known_project| {
        match lookup_project(&mut all_tasks, known_project) {
            Some(project) => {
                known_project.tasks.retain(|t| t.observed_at > now - retain_for && lookup_task(project, t).is_some());
                !project.tasks.is_empty()
            },
            None => false,
        }
    });

    for project in all_tasks.iter() {
        for task in project.tasks.iter() {
            if upsert_task(&mut known_tasks, project, task) {
                upsert_task(&mut notify_tasks, project, task);
            }
        }
    }

    notify_tasks.sort_by_key(|project| {
        Reverse(project.tasks.as_slice().into_iter().map(|i| i.created_at).max())
    });

    Ok(ResultingTasks { new_known: known_tasks, notify: notify_tasks })
}

fn notify_about_tasks(notify_tasks: &Vec<Project>, email_context: &mut EmailContext) -> Result<(), Box<dyn Error>> {
    if !notify_tasks.is_empty() {
        let mut email_body = String::new();
        for project in notify_tasks {
            email_body.push_str(format!("Project: {}/{} ({})\n", project.owner, project.name, project.url).as_str());

            for task in project.tasks.as_slice() {
                let prefix = match &task.task_type {
                    TaskType::Issue      => "Issue:       ",
                    TaskType::Pr         => "Pull Request:",
                    TaskType::Discussion => "Discussion:  ",
                };
                email_body.push_str(format!("  {} #{} {} by @{} ({}) -> {}\n", prefix, task.id, task.title, task.author, task.created_at, task.url).as_str());
            }
        }

        println!("{}", email_body);

        send_email(
            email_context,
            "GitHub: New Unsubscribed Tasks",
            email_body.as_str(),
        )?;
    } else {
        println!("No new tasks to notify about!");
    }
    Ok(())
}

fn get_env(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) => value,
        Err(_) => {
            println!("Failed to read env var {}!", name);
            exit(1);
        }
    }
}

fn email_address_from_env(name: &str) -> Address {
    let value = get_env(name);
    match value.parse::<Address>() {
        Ok(addr) => addr,
        Err(err) => {
            println!("Failed to parse email address from {} of var {}: {}", value, name, err);
            exit(1);
        },
    }
}

fn delay_from_env(name: &str) -> u64 {
    let value = get_env(name);
    match u64::from_str_radix(value.as_str(), 10) {
        Ok(delay) => delay,
        Err(err) => {
            println!("Failed to parse {} of var {} as delay value: {}", value, name, err);
            exit(1);
        }
    }
}

fn bool_from_env(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => value.to_lowercase().trim() == "true",
        Err(_) => default,
    }
}

#[tokio::main]
async fn main() {
    let build_hash = option_env!("BUILD_HASH");
    if let Some(hash) = build_hash {
        if hash != "" && hash != "unknown" {
            println!("Built from: https://github.com/pschichtel/ProjectMonitor/commit/{}", hash);
        }
    }

    let github_username = read_required_secret("github_username");
    let github_access_token = read_required_secret("github_access_token");

    let client = reqwest::Client::new();
    let github_context = GithubClientContext {
        client,
        username: github_username.to_string(),
        access_token: github_access_token.to_string()
    };

    let smtp_host = get_env("SMTP_HOST");
    let smtp_port = std::env::var("SMTP_PORT")
        .map(|port| u16::from_str_radix(port.as_str(), 10).unwrap_or(SUBMISSION_PORT))
        .unwrap_or(SUBMISSION_PORT);
    let smtp_username = read_secret("smtp_username");
    let smtp_password = read_secret("smtp_password");
    let smtp_security = if bool_from_env("SMTP_STARTTLS", false) {
        StartTls
    } else {
        TransportSecurity::None
    };
    let email_from = email_address_from_env("EMAIL_FROM");
    let email_to = email_address_from_env("EMAIL_TO");
    let mut email_context = create_email_client(smtp_host.as_str(), smtp_port, smtp_username, smtp_password, smtp_security, email_from, email_to)
        .expect("failed to setup email client");

    let persistence_path = std::env::var("PERSISTENCE_FILE")
        .unwrap_or("persistence.json".to_string());

    let delay = delay_from_env("DELAY");

    task::spawn(async {
        let mut sigint = signal(SignalKind::interrupt()).unwrap();
        let mut sigterm = signal(SignalKind::terminate()).unwrap();

        select! {
            _ = sigint.recv() => {
                println!("Received SIGINT, exiting cleanly...");
            },
            _ = sigterm.recv() => {
                println!("Received SIGTERM, exiting cleanly...");
            },
        }
        exit(0);
    });

    loop {
        match find_issues_for_notification(&github_context, Duration::from_hours(24), &mut email_context, persistence_path.as_str()).await {
            Ok(_) => {
                println!("Waiting for next check...")
            }
            Err(err) => {
                println!("Failed to check for new tasks: {}", err);
            }
        };
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}
