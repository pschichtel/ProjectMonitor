extern crate core;

use std::cmp::Reverse;
use std::error::Error;
use chrono::Utc;
use graphql_client::{GraphQLQuery, Response};
use serde::de::DeserializeOwned;
use serde::{Serialize};

type URI = String;
type DateTime = chrono::DateTime<Utc>;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github.graphql",
    query_path = "src/query.graphql",
    response_derives = "Debug",
)]
pub struct UserIssuesQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github.graphql",
    query_path = "src/query.graphql",
    response_derives = "Debug",
)]
pub struct OrgaIssuesQuery;

#[derive(Debug, Clone)]
pub struct Project {
    name: String,
    owner: String,
    url: URI,
    issues: Vec<Issue>,
}

#[derive(Debug, Clone)]
pub struct Issue {
    title: String,
    created_at: DateTime,
    url: URI,
    author: String,
}

async fn query_issues<Req, Res>(
    username: &str,
    access_token: &str,
    request_body: Req,
    f: fn(&str, Res) -> Vec<Project>,
) -> Result<Vec<Project>, Box<dyn Error>>
    where
        Req: Serialize,
        Res: DeserializeOwned {

    let client = reqwest::Client::new();
    let req = client.post("https://api.github.com/graphql")
        .json(&request_body)
        .basic_auth(username, Some(access_token))
        .header("User-Agent", "ProjectMonitor");
    let response = req
        .send()
        .await?;
    let bytes = response.bytes().await?;
    let body: Response<Res> = serde_json::from_slice(&bytes)?;

    Ok(f(username, body.data.unwrap()))
}

fn personal_projects(username: &str, data: user_issues_query::ResponseData) -> Vec<Project> {
    data
        .viewer
        .repositories
        .nodes
        .unwrap()
        .into_iter()
        .flatten()
        .filter(|repo| !repo.is_archived)
        .map(|repo| {
            let issues = repo.issues.nodes.unwrap().into_iter()
                .flatten()
                .map(|issue| Issue {
                    title: issue.title,
                    created_at: issue.created_at,
                    url: issue.url,
                    author: issue.author.unwrap().login,
                })
                .filter(|issue| issue.author != username)
                .collect::<Vec<_>>();
            Project {
                url: repo.url,
                name: repo.name,
                owner: repo.owner.login,
                issues,
            }
        })
        .filter(|project| !project.issues.is_empty())
        .collect::<Vec<_>>()
}

fn organization_projects(username: &str, data: orga_issues_query::ResponseData) -> Vec<Project> {
    data
        .viewer
        .organizations
        .nodes
        .unwrap()
        .into_iter()
        .flatten()
        .flat_map(|orga| {
            orga.repositories
                .nodes
                .unwrap()
                .into_iter()
                .flatten()
                .filter(|repo| !repo.is_archived)
                .map(|repo| {
                    let issues = repo.issues.nodes.unwrap().into_iter()
                        .flatten()
                        .map(|issue| Issue {
                            title: issue.title,
                            created_at: issue.created_at,
                            url: issue.url,
                            author: issue.author.unwrap().login,
                        })
                        .filter(|issue| issue.author != username)
                        .collect::<Vec<_>>();
                    Project {
                        url: repo.url,
                        name: repo.name,
                        owner: repo.owner.login,
                        issues,
                    }
                })
                .filter(|project| !project.issues.is_empty())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let github_username = option_env!("GITHUB_USERNAME").unwrap();
    let github_access_token = option_env!("GITHUB_ACCESS_TOKEN").unwrap();
    let personal = query_issues(
        github_username,
        github_access_token,
        UserIssuesQuery::build_query(user_issues_query::Variables {}),
        personal_projects,
    ).await?;
    let organization = query_issues(
        github_username,
        github_access_token,
        OrgaIssuesQuery::build_query(orga_issues_query::Variables {}),
        organization_projects,
    ).await?;


    let mut projects = personal.into_iter()
        .chain(organization.into_iter())
        .collect::<Vec<_>>();
    projects.sort_by_key(|project| {
        Reverse(project.clone().issues.into_iter().map(|i| i.created_at).max())
    });

    for project in &mut projects {
        project.issues.sort_by_key(|issue| Reverse(issue.created_at));

        println!("Project: {}/{} ({})", project.owner, project.name, project.url);

        for issue in project.issues.as_slice() {
            if issue.author != github_username {
                println!("  Issue: {} by {} ({}) -> {}", issue.title, issue.author, issue.created_at, issue.url);
            }
        }
    }

    Ok(())
}
