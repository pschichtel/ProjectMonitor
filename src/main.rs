extern crate core;

use std::cmp::Reverse;
use std::error::Error;
use std::fs;
use lettre::EmailAddress;
use github::{GithubClientContext, Task};
use crate::email::{create_email_client, send_email, TransportSecurity};
use crate::email::TransportSecurity::StartTls;

mod github;
mod email;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let github_username = read_secret("github_username").unwrap();
    let github_access_token = read_secret("github_access_token").unwrap();
    let client = reqwest::Client::new();
    let github_context = GithubClientContext {
        client,
        username: github_username.to_string(),
        access_token: github_access_token.to_string()
    };

    let smtp_host = std::env::var("SMTP_HOST")?;
    let smtp_port = std::env::var("SMTP_PORT")
        .map(|port| u16::from_str_radix(port.as_str(), 10).unwrap())
        .unwrap_or(587);
    let smtp_username = read_secret("smtp_username");
    let smtp_password = read_secret("smtp_password");
    let smtp_security = match std::env::var("SMTP_STARTTLS") {
        Ok(value) => if value.to_lowercase().trim() == "true" { StartTls } else { TransportSecurity::None }
        Err(_) => { TransportSecurity::None }
    };
    let mut email_context = create_email_client(smtp_host.as_str(), smtp_port, smtp_username, smtp_password, smtp_security);
    let email_from = EmailAddress::new(std::env::var("EMAIL_FROM")?)?;
    let email_to = EmailAddress::new(std::env::var("EMAIL_TO")?)?;


    let mut projects = github::fetch_all_projects(&github_context).await?;
    projects.sort_by_key(|project| {
        Reverse(project.tasks.as_slice().into_iter().map(|i| i.created_at()).max())
    });

    for project in &mut projects {
        if project.tasks.is_empty() {
            continue;
        }

        println!("Project: {}/{} ({})", project.owner, project.name, project.url);

        for issue in project.tasks.as_slice() {
            match issue {
                Task::Issue(issue) => {
                    println!("  Issue:        {} by {} ({}) -> {}", issue.title, issue.author, issue.created_at, issue.url);
                }
                Task::Pr(pull_request) => {
                    println!("  Pull Request: {} by {} ({}) -> {}", pull_request.title, pull_request.author, pull_request.created_at, pull_request.url);
                }
            }
        }
    }



    send_email(&mut email_context, email_from, email_to)?;

    Ok(())
}
