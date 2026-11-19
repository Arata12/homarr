//! Coolify (`coolify/coolify-integration.ts`): instance overview (version, projects, applications, services, servers).

use std::collections::HashMap;

use homarr_common::HomarrResult;
use reqwest::RequestBuilder;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::http;
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const PROJECT_DETAIL_CONCURRENCY: usize = 4;
const PROJECT_DETAIL_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoolifyApplication {
    pub id: Option<i64>,
    pub uuid: String,
    pub name: String,
    pub status: Option<String>,
    pub fqdn: Option<String>,
    pub environment_id: Option<i64>,
    pub server_id: Option<i64>,
    pub destination_id: Option<i64>,
    pub updated_at: Option<String>,
    pub last_online_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoolifyEnvironment {
    pub id: Option<i64>,
    pub uuid: Option<String>,
    pub name: String,
    pub project_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoolifyProject {
    pub id: Option<i64>,
    pub uuid: String,
    pub name: String,
    pub description: Option<String>,
    /// Only present on project detail responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environments: Option<Vec<CoolifyEnvironment>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoolifyServerSettings {
    pub server_id: Option<i64>,
    pub is_build_server: Option<bool>,
    pub is_reachable: Option<bool>,
    pub is_usable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoolifyServer {
    pub id: Option<i64>,
    pub uuid: String,
    pub name: String,
    pub description: Option<String>,
    pub ip: Option<String>,
    pub is_reachable: Option<bool>,
    pub is_usable: Option<bool>,
    pub settings: Option<CoolifyServerSettings>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoolifyServiceApplication {
    pub id: Option<i64>,
    pub uuid: String,
    pub name: String,
    pub fqdn: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoolifyService {
    pub id: Option<i64>,
    pub uuid: String,
    pub name: String,
    pub status: Option<String>,
    pub fqdn: Option<String>,
    pub environment_id: Option<i64>,
    pub server_id: Option<i64>,
    pub destination_id: Option<i64>,
    pub updated_at: Option<String>,
    pub applications: Option<Vec<CoolifyServiceApplication>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoolifyResource {
    pub id: Option<i64>,
    pub uuid: String,
    pub name: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoolifyContext {
    pub project_name: Option<String>,
    pub project_uuid: Option<String>,
    pub environment_name: Option<String>,
    pub environment_uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolifyApplicationWithContext {
    #[serde(flatten)]
    pub application: CoolifyApplication,
    #[serde(flatten)]
    pub context: CoolifyContext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolifyServiceWithContext {
    #[serde(flatten)]
    pub service: CoolifyService,
    #[serde(flatten)]
    pub context: CoolifyContext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolifyInstanceInfo {
    pub version: String,
    pub projects: Option<Vec<CoolifyProject>>,
    pub applications: Vec<CoolifyApplicationWithContext>,
    pub services: Vec<CoolifyServiceWithContext>,
    pub servers: Vec<CoolifyServer>,
}

fn authed(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<RequestBuilder> {
    Ok(http::get(rt, ctx, path, &[])?.bearer_auth(ctx.require_secret("apiKey")?))
}

async fn get_parsed<T: DeserializeOwned>(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    timeout_secs: Option<u64>,
) -> HomarrResult<T> {
    let mut req = authed(rt, ctx, path)?;
    if let Some(secs) = timeout_secs {
        req = req.timeout(http::timeout(secs));
    }
    let value = http::send_json(ctx, req, path).await?;
    serde_json::from_value(value).map_err(|err| parse_error(ctx, path, err))
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    http::send(ctx, http::get(rt, ctx, "/api/health", &[])?).await?;
    // Test API key authentication.
    http::send(ctx, authed(rt, ctx, "/api/v1/version")?).await?;
    Ok(())
}

pub async fn version(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<String> {
    let text = http::send_text(ctx, authed(rt, ctx, "/api/v1/version")?).await?;
    Ok(text.trim().to_string())
}

pub async fn healthcheck(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<String> {
    let value =
        http::send_json(ctx, authed(rt, ctx, "/api/v1/healthcheck")?, "healthcheck").await?;
    http::str_of(&value, "status")
        .map(str::to_string)
        .ok_or_else(|| parse_error(ctx, "healthcheck", "missing status"))
}

pub async fn applications(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<CoolifyApplication>> {
    get_parsed(rt, ctx, "/api/v1/applications", None).await
}

pub async fn application(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    uuid: &str,
) -> HomarrResult<CoolifyApplication> {
    let encoded = percent_encoding::utf8_percent_encode(uuid, percent_encoding::NON_ALPHANUMERIC);
    get_parsed(rt, ctx, &format!("/api/v1/applications/{encoded}"), None).await
}

pub async fn projects(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<CoolifyProject>> {
    get_parsed(rt, ctx, "/api/v1/projects", None).await
}

pub async fn project(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    uuid: &str,
) -> HomarrResult<CoolifyProject> {
    let encoded = percent_encoding::utf8_percent_encode(uuid, percent_encoding::NON_ALPHANUMERIC);
    get_parsed(
        rt,
        ctx,
        &format!("/api/v1/projects/{encoded}"),
        Some(PROJECT_DETAIL_TIMEOUT_SECS),
    )
    .await
}

pub async fn environments(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<CoolifyEnvironment>> {
    get_parsed(rt, ctx, "/api/v1/environments", None).await
}

pub async fn environment(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    name_or_uuid: &str,
) -> HomarrResult<CoolifyEnvironment> {
    let encoded =
        percent_encoding::utf8_percent_encode(name_or_uuid, percent_encoding::NON_ALPHANUMERIC);
    get_parsed(rt, ctx, &format!("/api/v1/environments/{encoded}"), None).await
}

pub async fn resources(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<CoolifyResource>> {
    get_parsed(rt, ctx, "/api/v1/resources", None).await
}

pub async fn servers(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<CoolifyServer>> {
    get_parsed(rt, ctx, "/api/v1/servers", None).await
}

pub async fn services(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<CoolifyService>> {
    get_parsed(rt, ctx, "/api/v1/services", None).await
}

/// All projects with their environments. Projects whose detail request fails keep their list entry.
async fn projects_with_environments(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<CoolifyProject>> {
    let list = projects(rt, ctx).await?;
    let mut details: Vec<HomarrResult<CoolifyProject>> = Vec::with_capacity(list.len());
    for batch in list.chunks(PROJECT_DETAIL_CONCURRENCY) {
        details.extend(
            futures_util::future::join_all(batch.iter().map(|p| project(rt, ctx, &p.uuid))).await,
        );
    }
    let failed = details.iter().filter(|d| d.is_err()).count();
    if failed > 0 {
        tracing::warn!(
            integration = %ctx.name,
            failed,
            projects = list.len(),
            "Coolify project context incomplete; continuing with available projects"
        );
    }
    Ok(list
        .into_iter()
        .zip(details)
        .map(|(listed, detail)| match detail {
            Ok(mut detail) => {
                detail.description = detail.description.or(listed.description);
                detail
            }
            Err(_) => listed,
        })
        .collect())
}

pub async fn instance_info(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<CoolifyInstanceInfo> {
    let (version, applications, services, projects, servers) = tokio::join!(
        version(rt, ctx),
        applications(rt, ctx),
        services(rt, ctx),
        projects_with_environments(rt, ctx),
        servers(rt, ctx),
    );
    let projects = match projects {
        Ok(projects) => Some(projects),
        Err(error) => {
            tracing::warn!(integration = %ctx.name, %error, "Coolify project context unavailable; continuing with resource statuses");
            None
        }
    };
    Ok(build_instance_info(
        version?,
        applications?,
        services?,
        projects,
        servers?,
    ))
}

pub fn build_instance_info(
    version: String,
    applications: Vec<CoolifyApplication>,
    services: Vec<CoolifyService>,
    projects: Option<Vec<CoolifyProject>>,
    servers: Vec<CoolifyServer>,
) -> CoolifyInstanceInfo {
    let mut env_to_project: HashMap<i64, CoolifyContext> = HashMap::new();
    for project in projects.iter().flatten() {
        for env in project.environments.iter().flatten() {
            if let Some(id) = env.id {
                env_to_project.insert(
                    id,
                    CoolifyContext {
                        project_name: Some(project.name.clone()),
                        project_uuid: Some(project.uuid.clone()),
                        environment_name: Some(env.name.clone()),
                        environment_uuid: env.uuid.clone(),
                    },
                );
            }
        }
    }
    let context_of = |environment_id: Option<i64>| {
        environment_id
            .filter(|id| *id != 0)
            .and_then(|id| env_to_project.get(&id).cloned())
            .unwrap_or_default()
    };
    CoolifyInstanceInfo {
        version,
        applications: applications
            .into_iter()
            .map(|application| CoolifyApplicationWithContext {
                context: context_of(application.environment_id),
                application,
            })
            .collect(),
        services: services
            .into_iter()
            .map(|mut service| {
                service.fqdn = service.fqdn.take().or_else(|| {
                    service
                        .applications
                        .iter()
                        .flatten()
                        .find_map(|app| app.fqdn.clone().filter(|f| !f.is_empty()))
                });
                CoolifyServiceWithContext {
                    context: context_of(service.environment_id),
                    service,
                }
            })
            .collect(),
        projects,
        servers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_project_context_and_service_fqdn() {
        let projects = vec![CoolifyProject {
            uuid: "p1".into(),
            name: "Prod".into(),
            environments: Some(vec![CoolifyEnvironment {
                id: Some(5),
                uuid: Some("e1".into()),
                name: "main".into(),
                project_id: Some(1),
            }]),
            ..Default::default()
        }];
        let applications = vec![
            CoolifyApplication {
                uuid: "a1".into(),
                name: "api".into(),
                environment_id: Some(5),
                ..Default::default()
            },
            CoolifyApplication {
                uuid: "a2".into(),
                name: "orphan".into(),
                environment_id: Some(9),
                ..Default::default()
            },
        ];
        let services = vec![CoolifyService {
            uuid: "s1".into(),
            name: "db".into(),
            environment_id: Some(5),
            applications: Some(vec![CoolifyServiceApplication {
                uuid: "x".into(),
                name: "x".into(),
                fqdn: Some("https://db.example".into()),
                ..Default::default()
            }]),
            ..Default::default()
        }];
        let info = build_instance_info("v4".into(), applications, services, Some(projects), vec![]);
        assert_eq!(
            info.applications[0].context.project_name.as_deref(),
            Some("Prod")
        );
        assert_eq!(
            info.applications[0].context.environment_uuid.as_deref(),
            Some("e1")
        );
        assert_eq!(info.applications[1].context, CoolifyContext::default());
        assert_eq!(
            info.services[0].service.fqdn.as_deref(),
            Some("https://db.example")
        );
        let json = serde_json::to_value(&info.applications[0]).unwrap();
        assert_eq!(json["projectName"], "Prod");
        assert_eq!(json["environment_id"], 5);
    }
}
