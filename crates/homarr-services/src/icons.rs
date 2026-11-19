use homarr_common::{validate_outbound_url, HomarrResult};
use homarr_core::{AppConfig, HttpClientFactory};
use homarr_db::{
    repos::icons::{sync_repository_icons, NewIconEntry},
    Database,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconSyncSummary {
    pub repositories_synced: usize,
    pub total_icons: usize,
    pub errors: Vec<String>,
}

struct IconRepoSpec {
    slug: &'static str,
    kind: RepoKind,
    api_url: &'static str,
    cdn_prefix: &'static str,
}

enum RepoKind {
    GitHubTree,
    JsdelivrFlat,
}

const REPO_SPECS: &[IconRepoSpec] = &[
    IconRepoSpec {
        slug: "homarr-labs/dashboard-icons",
        kind: RepoKind::GitHubTree,
        api_url: "https://api.github.com/repos/homarr-labs/dashboard-icons/git/trees/main?recursive=true",
        cdn_prefix: "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/",
    },
    IconRepoSpec {
        slug: "selfhst/icons",
        kind: RepoKind::GitHubTree,
        api_url: "https://api.github.com/repos/selfhst/icons/git/trees/main?recursive=true",
        cdn_prefix: "https://cdn.jsdelivr.net/gh/selfhst/icons/",
    },
    IconRepoSpec {
        slug: "PapirusDevelopmentTeam/papirus-icon-theme",
        kind: RepoKind::JsdelivrFlat,
        api_url: "https://data.jsdelivr.com/v1/packages/gh/PapirusDevelopmentTeam/papirus_icons@master?structure=flat",
        cdn_prefix: "https://cdn.jsdelivr.net/gh/PapirusDevelopmentTeam/papirus_icons/",
    },
    IconRepoSpec {
        slug: "loganmarchione/homelab-svg-assets",
        kind: RepoKind::JsdelivrFlat,
        api_url: "https://data.jsdelivr.com/v1/packages/gh/loganmarchione/homelab-svg-assets@main?structure=flat",
        cdn_prefix: "https://cdn.jsdelivr.net/gh/loganmarchione/homelab-svg-assets/",
    },
    IconRepoSpec {
        slug: "simple-icons/simple-icons",
        kind: RepoKind::GitHubTree,
        api_url: "https://api.github.com/repos/simple-icons/simple-icons/git/trees/develop?recursive=true",
        cdn_prefix: "https://cdn.simpleicons.org/",
    },
];

#[derive(Clone)]
pub struct IconService {
    config: AppConfig,
    http: HttpClientFactory,
}

impl IconService {
    pub fn new(config: AppConfig, http: HttpClientFactory) -> Self {
        Self { config, http }
    }

    pub async fn sync_all(&self, db: &Database) -> HomarrResult<IconSyncSummary> {
        if self.config.no_external_connection {
            tracing::info!("Skipping icon repository sync because NO_EXTERNAL_CONNECTION=true");
            return Ok(IconSyncSummary {
                repositories_synced: 0,
                total_icons: 0,
                errors: Vec::new(),
            });
        }

        let mut synced = 0usize;
        let mut total_icons = 0usize;
        let mut errors = Vec::new();

        for spec in REPO_SPECS {
            match self.fetch_repo_entries(spec).await {
                Ok(entries) => {
                    if !entries.is_empty() {
                        match sync_repository_icons(db, spec.slug, &entries).await {
                            Ok((inserted, _deleted)) => {
                                synced += 1;
                                total_icons += inserted;
                            }
                            Err(err) => {
                                errors.push(format!("{}: {err}", spec.slug));
                            }
                        }
                    }
                }
                Err(err) => {
                    errors.push(format!("{}: {err}", spec.slug));
                }
            }
        }

        Ok(IconSyncSummary {
            repositories_synced: synced,
            total_icons,
            errors,
        })
    }

    async fn fetch_repo_entries(&self, spec: &IconRepoSpec) -> HomarrResult<Vec<NewIconEntry>> {
        let resp = self
            .http
            .standard()
            .get(spec.api_url)
            .header("Accept", "application/vnd.github+json, application/json")
            .send()
            .await
            .map_err(|e| homarr_common::HomarrError::Upstream(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(homarr_common::HomarrError::Upstream(format!(
                "Icon repo {} returned HTTP {}",
                spec.slug,
                resp.status()
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| homarr_common::HomarrError::Upstream(e.to_string()))?;

        let mut out = Vec::new();
        match spec.kind {
            RepoKind::GitHubTree => {
                if let Some(tree) = json.get("tree").and_then(|v| v.as_array()) {
                    for node in tree {
                        let path = node.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        if !is_icon_asset(path) {
                            continue;
                        }
                        if spec.slug == "homarr-labs/dashboard-icons"
                            && !path.starts_with("svg/")
                            && !path.starts_with("png/")
                        {
                            continue;
                        }
                        let file_name = path.rsplit('/').next().unwrap_or(path);
                        let stem = file_name
                            .strip_suffix(".svg")
                            .or_else(|| file_name.strip_suffix(".png"))
                            .or_else(|| file_name.strip_suffix(".webp"))
                            .unwrap_or(file_name);
                        let url = if spec.slug == "simple-icons/simple-icons" {
                            format!("{}{}", spec.cdn_prefix, stem)
                        } else {
                            format!("{}{}", spec.cdn_prefix, path.trim_start_matches('/'))
                        };
                        let checksum = node
                            .get("sha")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| sha256_hex(&url));
                        out.push(NewIconEntry {
                            name: file_name.to_string(),
                            url,
                            checksum,
                        });
                    }
                }
            }
            RepoKind::JsdelivrFlat => {
                if let Some(files) = json.get("files").and_then(|v| v.as_array()) {
                    for file in files {
                        let name = file.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        if !is_icon_asset(name) {
                            continue;
                        }
                        let clean = name.trim_start_matches('/');
                        let file_name = clean.rsplit('/').next().unwrap_or(clean);
                        let url = format!("{}{}", spec.cdn_prefix, clean);
                        let checksum = file
                            .get("hash")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| sha256_hex(&url));
                        out.push(NewIconEntry {
                            name: file_name.to_string(),
                            url,
                            checksum,
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    pub async fn fetch_favicon_url(&self, site_url: &str) -> HomarrResult<Option<String>> {
        let base = validate_outbound_url(site_url, None)?;
        let (bytes, _, status) = self
            .http
            .guarded_get(base.as_str(), None, None, 2 * 1024 * 1024)
            .await?;
        if status < 400 {
            let html = String::from_utf8_lossy(&bytes);
            if let Some(found) = extract_favicon_from_html(&html, &base) {
                return Ok(Some(found));
            }
        }
        let fallback = base.join("/favicon.ico").map(|u| u.to_string()).ok();
        Ok(fallback)
    }
}

fn is_icon_asset(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".svg") || lower.ends_with(".png") || lower.ends_with(".webp")
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

fn extract_favicon_from_html(html: &str, base: &url::Url) -> Option<String> {
    let re = Regex::new(r#"(?i)<link[^>]+rel=["'][^"']*icon[^"']*["'][^>]*href=["']([^"']+)["']|<link[^>]+href=["']([^"']+)["'][^>]*rel=["'][^"']*icon[^"']*["']"#).ok()?;
    let caps = re.captures(html)?;
    let href = caps
        .get(1)
        .or_else(|| caps.get(2))
        .map(|m| m.as_str().trim())?;
    base.join(href).ok().map(|u| u.to_string())
}
