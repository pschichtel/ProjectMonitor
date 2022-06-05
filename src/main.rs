extern crate core;

use std::cmp::Reverse;
use std::error::Error;
use github::{ClientContext, Task};

mod github;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let github_username = option_env!("GITHUB_USERNAME").unwrap();
    let github_access_token = option_env!("GITHUB_ACCESS_TOKEN").unwrap();
    let client = reqwest::Client::new();
    let context = ClientContext {
        client,
        username: github_username.to_string(),
        access_token: github_access_token.to_string()
    };
    let mut projects = github::fetch_all_projects(&context).await?;
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

    Ok(())
}
