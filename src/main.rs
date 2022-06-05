extern crate core;

use std::cmp::Reverse;
use std::error::Error;
use std::fmt::Debug;
use chrono::Utc;
use futures::future::join_all;
use graphql_client::{GraphQLQuery, Response};
use serde::de::DeserializeOwned;
use serde::{Serialize};
use tokio::try_join;

type URI = String;
type DateTime = chrono::DateTime<Utc>;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github.graphql",
    query_path = "src/query.graphql",
    response_derives = "Debug",
)]
pub struct ViewerReposQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github.graphql",
    query_path = "src/query.graphql",
    response_derives = "Debug",
)]
pub struct ViewerOrganizationsQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github.graphql",
    query_path = "src/query.graphql",
    response_derives = "Debug",
)]
pub struct OrganizationReposQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github.graphql",
    query_path = "src/query.graphql",
    response_derives = "Debug",
)]
pub struct RepoQuery;

#[derive(Debug, Clone)]
pub struct Project {
    name: String,
    owner: String,
    url: URI,
    tasks: Vec<Task>,
}

#[derive(Debug, Clone)]
pub enum Task {
    Issue(Issue),
    Pr(PullRequest),
}

impl Task {
    fn created_at(&self) -> chrono::DateTime<Utc> {
        match self {
            Task::Issue(issue) => issue.created_at,
            Task::Pr(pr) => pr.created_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Issue {
    title: String,
    created_at: DateTime,
    url: URI,
    author: String,
}

#[derive(Debug, Clone)]
pub struct PullRequest {
    title: String,
    created_at: DateTime,
    url: URI,
    author: String,
}

struct ClientContext {
    client: reqwest::Client,
    username: String,
    access_token: String,
}

impl PartialEq for repo_query::SubscriptionState {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (&repo_query::SubscriptionState::SUBSCRIBED, &repo_query::SubscriptionState::SUBSCRIBED) => true,
            (&repo_query::SubscriptionState::UNSUBSCRIBED, &repo_query::SubscriptionState::UNSUBSCRIBED) => true,
            (&repo_query::SubscriptionState::IGNORED, &repo_query::SubscriptionState::IGNORED) => true,
            _ => false,
        }
    }

    fn ne(&self, other: &Self) -> bool {
        !self.eq(other)
    }
}

async fn run_query<Req, Res>(
    context: &ClientContext,
    request_body: Req,
) -> Result<Res, Box<dyn Error>>
    where
        Req: Serialize,
        Res: DeserializeOwned,
        Res: Debug {
    let req = context.client.post("https://api.github.com/graphql")
        .json(&request_body)
        .basic_auth(context.username.as_str(), Some(context.access_token.as_str()))
        .header("User-Agent", "ProjectMonitor");
    let response = req
        .send()
        .await?;
    let bytes = response.bytes().await?;
    let body: Response<Res> = serde_json::from_slice(&bytes)?;

    // println!("body={:?}", body);

    Ok(body.data.unwrap())
}

#[derive(Debug)]
struct Repo {
    owner: String,
    name: String,
}

async fn fetch_viewer_repos(context: &ClientContext) -> Result<Vec<Repo>, Box<dyn Error>> {
    let mut output: Vec<Repo> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let variables = viewer_repos_query::Variables { cursor: cursor.as_ref().map(|a| a.clone()) };
        let result = run_query::<_, viewer_repos_query::ResponseData>(context, ViewerReposQuery::build_query(variables)).await?;
        let values = result.viewer.repositories.edges.unwrap()
            .into_iter()
            .flatten()
            .map(|edge| edge.node.unwrap())
            .filter(|repo| !repo.is_archived)
            .map(|repo| Repo { owner: repo.owner.login, name: repo.name });

        output.extend(values);

        let page_info = result.viewer.repositories.page_info;
        cursor = page_info.end_cursor;
        if !page_info.has_next_page {
            break;
        }
    }
    Ok(output)
}

async fn fetch_viewer_organizations(context: &ClientContext) -> Result<Vec<String>, Box<dyn Error>> {
    let mut output: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let variables = viewer_organizations_query::Variables { cursor: cursor.as_ref().map(|a| a.clone()) };
        let result = run_query::<_, viewer_organizations_query::ResponseData>(context, ViewerOrganizationsQuery::build_query(variables)).await?;
        let values = result.viewer.organizations.edges.unwrap()
            .into_iter()
            .flatten()
            .map(|edge| edge.node.unwrap())
            .filter(|orga| orga.viewer_can_administer)
            .map(|orga| orga.login);

        output.extend(values);

        let page_info = result.viewer.organizations.page_info;
        cursor = page_info.end_cursor;
        if !page_info.has_next_page {
            break;
        }
    }
    Ok(output)
}

async fn fetch_orga_repos(context: &ClientContext, login: &str) -> Result<Vec<Repo>, Box<dyn Error>> {
    let mut output: Vec<Repo> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let variables = organization_repos_query::Variables { login: login.to_string(), cursor: cursor.as_ref().map(|a| a.clone()) };
        let result = run_query::<_, organization_repos_query::ResponseData>(context, OrganizationReposQuery::build_query(variables)).await?;
        let orga_repos = result.organization.unwrap().repositories;
        let values = orga_repos.edges.unwrap()
            .into_iter()
            .flatten()
            .map(|edge| edge.node.unwrap())
            .filter(|repo| !repo.is_archived)
            .map(|repo| Repo { owner: repo.owner.login, name: repo.name });

        output.extend(values);

        let page_info = orga_repos.page_info;
        cursor = page_info.end_cursor;
        if !page_info.has_next_page {
            break;
        }
    }
    Ok(output)
}

async fn fetch_all_orga_repos(context: &ClientContext) -> Result<Vec<Repo>, Box<dyn Error>> {
    let orgas = &fetch_viewer_organizations(&context).await?;
    let mut futures = Vec::new();
    for orga in orgas {
        futures.push(fetch_orga_repos(context, orga.as_str()));
    }

    let results: Result<Vec<Vec<Repo>>, Box<dyn Error>> =
        join_all(futures).await.into_iter().collect();

    results.map(|nested| nested.into_iter().flatten().collect::<Vec<Repo>>())
}

async fn fetch_all_repos(context: &ClientContext) -> Result<Vec<Repo>, Box<dyn Error>> {
    let (viewer_repos, orga_repos) =
        try_join!(fetch_viewer_repos(&context), fetch_all_orga_repos(&context))?;

    let repos: Vec<Repo> = viewer_repos.into_iter()
         .chain(orga_repos.into_iter())
         .collect();

    Ok(repos)
}

async fn fetch_project(context: &ClientContext, owner: &str, name: &str) -> Result<Project, Box<dyn Error>> {
    let mut tasks: Vec<Task> = Vec::new();
    let mut issue_cursor: Option<String> = None;
    let mut pull_request_cursor: Option<String> = None;
    let mut repo;
    loop {
        let variables = repo_query::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
            issue_cursor: issue_cursor.as_ref().map(|a| a.clone()),
            pull_request_cursor: pull_request_cursor.as_ref().map(|a| a.clone()),
        };
        let result = run_query::<_, repo_query::ResponseData>(context, RepoQuery::build_query(variables)).await?;
        repo = result.repository.unwrap();
        let issues = repo.issues.edges.unwrap()
            .into_iter()
            .flatten()
            .map(|edge| edge.node.unwrap())
            .filter(|issue| issue.viewer_subscription.as_ref() != Some(&repo_query::SubscriptionState::SUBSCRIBED))
            .map(|issue| Issue { author: issue.author.map(|a| a.login).unwrap_or("<deleted user>".to_string()), url: issue.url, title: issue.title, created_at: issue.created_at })
            .filter(|issue| issue.author != context.username)
            .map(|issue| Task::Issue(issue));

        tasks.extend(issues);

        let pull_requests = repo.pull_requests.edges.unwrap()
            .into_iter()
            .flatten()
            .map(|edge| edge.node.unwrap())
            .filter(|pr| pr.viewer_subscription.as_ref() != Some(&repo_query::SubscriptionState::SUBSCRIBED))
            .map(|pr| PullRequest { author: pr.author.map(|a| a.login).unwrap_or("<deleted user>".to_string()), url: pr.url, title: pr.title, created_at: pr.created_at })
            .filter(|pr| pr.author != context.username)
            .map(|pr| Task::Pr(pr));

        tasks.extend(pull_requests);

        issue_cursor = repo.issues.page_info.end_cursor;
        pull_request_cursor = repo.pull_requests.page_info.end_cursor;
        if !repo.issues.page_info.has_next_page || repo.pull_requests.page_info.has_next_page {
            break;
        }
    }
    tasks.sort_by_key(|task| Reverse(task.created_at()));
    let project = Project {
        url: repo.url,
        name: name.to_string(),
        owner: owner.to_string(),
        tasks: tasks,
    };

    Ok(project)
}

async fn fetch_all_projects(context: &ClientContext) -> Result<Vec<Project>, Box<dyn Error>> {
    let repos = &fetch_all_repos(context).await?;
    let mut futures = Vec::new();
    for repo in repos {
        futures.push(fetch_project(context, repo.owner.as_str(), repo.name.as_str()));
    }

    join_all(futures).await.into_iter().collect()
}

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
    let mut projects = fetch_all_projects(&context).await?;
    projects.sort_by_key(|project| {
        Reverse(project.clone().tasks.into_iter().map(|i| i.created_at()).max())
    });

    for project in &mut projects {
        if project.tasks.is_empty() {
            continue;
        }

        println!("Project: {}/{} ({})", project.owner, project.name, project.url);

        for issue in project.tasks.as_slice() {
            match issue {
                Task::Issue(issue) => {
                    println!("  Issue: {} by {} ({}) -> {}", issue.title, issue.author, issue.created_at, issue.url);
                }
                Task::Pr(pull_request) => {
                    println!("  Pull Request: {} by {} ({}) -> {}", pull_request.title, pull_request.author, pull_request.created_at, pull_request.url);
                }
            }
        }
    }

    Ok(())
}
