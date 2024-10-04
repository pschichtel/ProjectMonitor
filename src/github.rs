use std::error::Error;
use chrono::Utc;
use std::cmp::Reverse;
use serde::{Serialize, Deserialize};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use graphql_client::{GraphQLQuery, Response};
use futures::future::join_all;
use tokio::try_join;
use crate::error::QueryError;

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

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub owner: String,
    pub url: URI,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Task {
    Issue(Issue),
    Pr(PullRequest),
    Discussion(Discussion),
}

impl Task {
    pub fn url(&self) -> URI {
        match self {
            Task::Issue(issue) => issue.url.clone(),
            Task::Pr(pr) => pr.url.clone(),
            Task::Discussion(discussion) => discussion.url.clone(),
        }
    }

    pub fn created_at(&self) -> chrono::DateTime<Utc> {
        match self {
            Task::Issue(issue) => issue.created_at,
            Task::Pr(pr) => pr.created_at,
            Task::Discussion(discussion) => discussion.created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Issue {
    pub id: i64,
    pub title: String,
    pub created_at: DateTime,
    pub url: URI,
    pub author: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: i64,
    pub title: String,
    pub created_at: DateTime,
    pub url: URI,
    pub author: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Discussion {
    pub id: i64,
    pub title: String,
    pub created_at: DateTime,
    pub url: URI,
    pub author: String,
}

pub struct GithubClientContext {
    pub client: reqwest::Client,
    pub username: String,
    pub access_token: String,
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
    context: &GithubClientContext,
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
    let status = response.status();
    if !status.is_success() {
        return Err(Box::new(QueryError::HttpError(status.as_u16())));
    }
    let bytes = response.bytes().await?;
    let body: Response<Res> = serde_json::from_slice(&bytes)?;

    match body.data {
        Some(data) => Ok(data),
        None => {
            Err(Box::new(QueryError::NoData))
        }
    }
}

#[derive(Debug)]
struct Repo {
    owner: String,
    name: String,
}

async fn fetch_viewer_repos(context: &GithubClientContext) -> Result<Vec<Repo>, Box<dyn Error>> {
    let mut output: Vec<Repo> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let variables = viewer_repos_query::Variables { cursor: cursor.as_ref().map(|a| a.clone()) };
        let result = run_query::<_, viewer_repos_query::ResponseData>(context, ViewerReposQuery::build_query(variables)).await?;
        let values = result.viewer.repositories.edges
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|edge| edge.node)
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

async fn fetch_viewer_organizations(context: &GithubClientContext) -> Result<Vec<String>, Box<dyn Error>> {
    let mut output: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let variables = viewer_organizations_query::Variables { cursor: cursor.as_ref().map(|a| a.clone()) };
        let result = run_query::<_, viewer_organizations_query::ResponseData>(context, ViewerOrganizationsQuery::build_query(variables)).await?;
        let values = result.viewer.organizations.edges
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|edge| edge.node)
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

async fn fetch_orga_repos(context: &GithubClientContext, login: &str) -> Result<Vec<Repo>, Box<dyn Error>> {
    let mut output: Vec<Repo> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let variables = organization_repos_query::Variables { login: login.to_string(), cursor: cursor.as_ref().map(|a| a.clone()) };
        let result = run_query::<_, organization_repos_query::ResponseData>(context, OrganizationReposQuery::build_query(variables)).await?;
        let orga_repos = result.organization.ok_or("no organization")?.repositories;
        let values = orga_repos.edges
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|edge| edge.node)
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

async fn fetch_all_orga_repos(context: &GithubClientContext) -> Result<Vec<Repo>, Box<dyn Error>> {
    let orgas = &fetch_viewer_organizations(&context).await?;
    let mut futures = Vec::new();
    for orga in orgas {
        futures.push(fetch_orga_repos(context, orga.as_str()));
    }

    let results: Result<Vec<Vec<Repo>>, Box<dyn Error>> =
        join_all(futures).await.into_iter().collect();

    results.map(|nested| nested.into_iter().flatten().collect::<Vec<Repo>>())
}

async fn fetch_all_repos(context: &GithubClientContext) -> Result<Vec<Repo>, Box<dyn Error>> {
    let (viewer_repos, orga_repos) =
        try_join!(fetch_viewer_repos(&context), fetch_all_orga_repos(&context))?;

    let repos: Vec<Repo> = viewer_repos.into_iter()
         .chain(orga_repos.into_iter())
         .collect();

    Ok(repos)
}

trait Authored {
    fn get_author_name(&self) -> Option<String>;

    fn get_author_name_or_default(&self) -> String {
        self.get_author_name().unwrap_or("<deleted user>".to_string())
    }
}

impl Authored for repo_query::RepoQueryRepositoryIssuesEdgesNode {
    fn get_author_name(&self) -> Option<String> {
        self.author.as_ref().map(|author| author.login.clone())
    }
}

impl Authored for repo_query::RepoQueryRepositoryPullRequestsEdgesNode {
    fn get_author_name(&self) -> Option<String> {
        self.author.as_ref().map(|author| author.login.clone())
    }
}

impl Authored for repo_query::RepoQueryRepositoryDiscussionsEdgesNode {
    fn get_author_name(&self) -> Option<String> {
        self.author.as_ref().map(|author| author.login.clone())
    }
}

async fn fetch_project(context: &GithubClientContext, owner: &str, name: &str) -> Result<Project, Box<dyn Error>> {
    let mut tasks: Vec<Task> = Vec::new();
    let mut issue_cursor: Option<String> = None;
    let mut pull_request_cursor: Option<String> = None;
    let mut discussion_cursor: Option<String> = None;
    let mut repo;

    loop {
        let variables = repo_query::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
            issue_cursor: issue_cursor.as_ref().map(|a| a.clone()),
            pull_request_cursor: pull_request_cursor.as_ref().map(|a| a.clone()),
            discussion_cursor: discussion_cursor.as_ref().map(|a| a.clone()),
        };
        let result = run_query::<_, repo_query::ResponseData>(context, RepoQuery::build_query(variables)).await?;
        repo = result.repository.ok_or("no repository")?;
        let issues = repo.issues.edges
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|edge| edge.node)
            .filter(|issue| issue.viewer_subscription.as_ref() != Some(&repo_query::SubscriptionState::SUBSCRIBED))
            .map(|issue| Issue { id: issue.number, author: issue.get_author_name_or_default(), url: issue.url, title: issue.title, created_at: issue.created_at })
            .filter(|issue| issue.author != context.username)
            .map(|issue| Task::Issue(issue));

        tasks.extend(issues);

        let pull_requests = repo.pull_requests.edges
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|edge| edge.node)
            .filter(|pr| pr.viewer_subscription.as_ref() != Some(&repo_query::SubscriptionState::SUBSCRIBED))
            .map(|pr| PullRequest { id: pr.number, author: pr.get_author_name_or_default(), url: pr.url, title: pr.title, created_at: pr.created_at })
            .filter(|pr| pr.author != context.username)
            .map(|pr| Task::Pr(pr));

        tasks.extend(pull_requests);

        let discussions = repo.discussions.edges
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|edge| edge.node)
            .filter(|discussion| discussion.viewer_subscription.as_ref() != Some(&repo_query::SubscriptionState::SUBSCRIBED))
            .map(|discussion| Discussion { id: discussion.number, author: discussion.get_author_name_or_default(), url: discussion.url, title: discussion.title, created_at: discussion.created_at })
            .filter(|discussion| discussion.author != context.username)
            .map(|discussion| Task::Discussion(discussion));

        tasks.extend(discussions);

        issue_cursor = repo.issues.page_info.end_cursor;
        pull_request_cursor = repo.pull_requests.page_info.end_cursor;
        discussion_cursor = repo.discussions.page_info.end_cursor;
        if !repo.issues.page_info.has_next_page || repo.pull_requests.page_info.has_next_page || repo.discussions.page_info.has_next_page {
            break;
        }
    }
    tasks.sort_by_key(|task| Reverse(task.created_at()));
    let project = Project {
        url: repo.url,
        name: name.to_string(),
        owner: owner.to_string(),
        tasks,
    };

    Ok(project)
}

pub async fn fetch_all_projects(context: &GithubClientContext) -> Result<Vec<Project>, Box<dyn Error>> {
    let repos = &fetch_all_repos(context).await?;
    let mut futures = Vec::new();
    for repo in repos {
        futures.push(fetch_project(context, repo.owner.as_str(), repo.name.as_str()));
    }

    join_all(futures).await.into_iter().collect()
}
