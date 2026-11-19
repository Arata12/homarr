use std::time::Duration;

use chrono::Utc;
use homarr_common::{HomarrError, HomarrResult};
use homarr_core::{AppConfig, HttpClientFactory};
use homarr_db::{
    repos::{apps, boards, groups, integrations, users},
    Database,
};
use homarr_redis::RedisStore;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherData {
    pub current: WeatherCurrent,
    pub daily: Vec<WeatherDaily>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherCurrent {
    pub temperature_2m: f64,
    pub apparent_temperature: f64,
    pub weather_code: i64,
    pub wind_speed_10m: f64,
    pub is_day: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherDaily {
    pub time: String,
    pub weather_code: i64,
    pub temperature_2m_max: f64,
    pub temperature_2m_min: f64,
    pub sunrise: Option<String>,
    pub sunset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AirQualityData {
    pub european_aqi: f64,
    pub us_aqi: f64,
    pub pm10: f64,
    pub pm2_5: f64,
    pub carbon_monoxide: f64,
    pub nitrogen_dioxide: f64,
    pub ozone: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeocodingCity {
    pub id: i64,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub country: Option<String>,
    pub admin1: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedEntry {
    pub title: String,
    pub link: String,
    pub published: Option<String>,
    pub description: Option<String>,
    pub feed_title: Option<String>,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockPriceInfo {
    pub symbol: String,
    pub short_name: Option<String>,
    pub currency: Option<String>,
    pub regular_market_price: f64,
    pub previous_close: f64,
    pub change: f64,
    pub change_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub provider: String,
    pub identifier: String,
    pub latest_release: Option<String>,
    pub published_at: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftStatus {
    pub online: bool,
    pub host: String,
    pub port: u16,
    pub version: Option<String>,
    pub motd: Option<String>,
    pub players_online: u64,
    pub players_max: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuckDuckGoBang {
    pub t: String,
    pub s: String,
    #[serde(default)]
    pub d: Option<String>,
    pub u: String,
    #[serde(default)]
    pub c: Option<String>,
    #[serde(default)]
    pub sc: Option<String>,
    #[serde(default)]
    pub r: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckSummary {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSnapshot {
    pub version: String,
    pub users_count: usize,
    pub groups_count: usize,
    pub boards_count: usize,
    pub apps_count: usize,
    pub integrations_count: usize,
    pub generated_at: i64,
}

#[derive(Clone)]
pub struct ExternalDataService {
    config: AppConfig,
    http: HttpClientFactory,
    redis: RedisStore,
}

impl ExternalDataService {
    pub fn new(config: AppConfig, http: HttpClientFactory, redis: RedisStore) -> Self {
        Self {
            config,
            http,
            redis,
        }
    }

    pub async fn get_weather(&self, lat: f64, lon: f64) -> HomarrResult<WeatherData> {
        let cache_key = format!("ext:weather:{lat:.2}:{lon:.2}");
        if let Ok(Some(cached)) = self.redis.get_json::<WeatherData>(&cache_key).await {
            return Ok(cached);
        }
        if self.config.no_external_connection {
            return Err(HomarrError::Forbidden(
                "External connections are disabled".into(),
            ));
        }

        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,apparent_temperature,is_day,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset&timezone=auto"
        );
        let resp = self
            .http
            .standard()
            .get(&url)
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Open-Meteo weather failed: {e}")))?;
        let raw: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Invalid weather JSON: {e}")))?;

        let cur = raw.get("current").cloned().unwrap_or(Value::Null);
        let current = WeatherCurrent {
            temperature_2m: cur
                .get("temperature_2m")
                .and_then(|v| v.as_f64())
                .unwrap_or_default(),
            apparent_temperature: cur
                .get("apparent_temperature")
                .and_then(|v| v.as_f64())
                .unwrap_or_default(),
            weather_code: cur
                .get("weather_code")
                .and_then(|v| v.as_i64())
                .unwrap_or_default(),
            wind_speed_10m: cur
                .get("wind_speed_10m")
                .and_then(|v| v.as_f64())
                .unwrap_or_default(),
            is_day: cur.get("is_day").and_then(|v| v.as_i64()).unwrap_or(1) == 1,
        };

        let mut daily = Vec::new();
        if let Some(d) = raw.get("daily").and_then(|v| v.as_object()) {
            let times = d.get("time").and_then(|v| v.as_array());
            let codes = d.get("weather_code").and_then(|v| v.as_array());
            let highs = d.get("temperature_2m_max").and_then(|v| v.as_array());
            let lows = d.get("temperature_2m_min").and_then(|v| v.as_array());
            let sunrises = d.get("sunrise").and_then(|v| v.as_array());
            let sunsets = d.get("sunset").and_then(|v| v.as_array());
            if let Some(times) = times {
                for (i, t) in times.iter().enumerate() {
                    daily.push(WeatherDaily {
                        time: t.as_str().unwrap_or_default().to_string(),
                        weather_code: codes
                            .and_then(|a| a.get(i))
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0),
                        temperature_2m_max: highs
                            .and_then(|a| a.get(i))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0),
                        temperature_2m_min: lows
                            .and_then(|a| a.get(i))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0),
                        sunrise: sunrises
                            .and_then(|a| a.get(i))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        sunset: sunsets
                            .and_then(|a| a.get(i))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                    });
                }
            }
        }

        let data = WeatherData { current, daily };
        let _ = self.redis.set_json_ex(&cache_key, &data, 600).await;
        Ok(data)
    }

    pub async fn get_air_quality(&self, lat: f64, lon: f64) -> HomarrResult<AirQualityData> {
        let cache_key = format!("ext:aqi:{lat:.2}:{lon:.2}");
        if let Ok(Some(cached)) = self.redis.get_json::<AirQualityData>(&cache_key).await {
            return Ok(cached);
        }
        if self.config.no_external_connection {
            return Err(HomarrError::Forbidden(
                "External connections are disabled".into(),
            ));
        }

        let url = format!(
            "https://air-quality-api.open-meteo.com/v1/air-quality?latitude={lat}&longitude={lon}&current=european_aqi,us_aqi,pm10,pm2_5,carbon_monoxide,nitrogen_dioxide,ozone"
        );
        let resp = self
            .http
            .standard()
            .get(&url)
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Open-Meteo AQI failed: {e}")))?;
        let raw: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Invalid AQI JSON: {e}")))?;
        let cur = raw.get("current").cloned().unwrap_or(Value::Null);
        let data = AirQualityData {
            european_aqi: cur
                .get("european_aqi")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            us_aqi: cur.get("us_aqi").and_then(|v| v.as_f64()).unwrap_or(0.0),
            pm10: cur.get("pm10").and_then(|v| v.as_f64()).unwrap_or(0.0),
            pm2_5: cur.get("pm2_5").and_then(|v| v.as_f64()).unwrap_or(0.0),
            carbon_monoxide: cur
                .get("carbon_monoxide")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            nitrogen_dioxide: cur
                .get("nitrogen_dioxide")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            ozone: cur.get("ozone").and_then(|v| v.as_f64()).unwrap_or(0.0),
        };
        let _ = self.redis.set_json_ex(&cache_key, &data, 900).await;
        Ok(data)
    }

    pub async fn search_cities(&self, query: &str) -> HomarrResult<Vec<GeocodingCity>> {
        if self.config.no_external_connection || query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let encoded: String =
            url::form_urlencoded::byte_serialize(query.trim().as_bytes()).collect();
        let url = format!("https://geocoding-api.open-meteo.com/v1/search?name={encoded}&count=10&language=en&format=json");
        let resp = self
            .http
            .standard()
            .get(&url)
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Geocoding failed: {e}")))?;
        let raw: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Invalid geocoding JSON: {e}")))?;
        let results = raw
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(results
            .into_iter()
            .filter_map(|item| {
                Some(GeocodingCity {
                    id: item.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                    name: item.get("name")?.as_str()?.to_string(),
                    latitude: item.get("latitude")?.as_f64()?,
                    longitude: item.get("longitude")?.as_f64()?,
                    country: item
                        .get("country")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    admin1: item
                        .get("admin1")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                })
            })
            .collect())
    }

    pub async fn fetch_feeds(&self, urls: &[String], limit: usize) -> HomarrResult<Vec<FeedEntry>> {
        if self.config.no_external_connection {
            return Ok(Vec::new());
        }
        let mut all = Vec::new();
        for url in urls.iter().take(10) {
            match self
                .http
                .guarded_get(url, None, None, 2 * 1024 * 1024)
                .await
            {
                Ok((bytes, _, status)) if status < 400 => {
                    let xml = String::from_utf8_lossy(&bytes);
                    all.extend(parse_rss_or_atom(&xml));
                }
                Ok((_, _, status)) => {
                    tracing::warn!(url = %url, status = status, "RSS feed returned HTTP error");
                }
                Err(err) => {
                    tracing::warn!(url = %url, error = %err, "Failed to fetch RSS feed");
                }
            }
        }
        all.truncate(limit.clamp(1, 200));
        Ok(all)
    }

    pub async fn get_stock_price(&self, symbol: &str) -> HomarrResult<StockPriceInfo> {
        let sym = symbol.trim().to_ascii_uppercase();
        if sym.is_empty() {
            return Err(HomarrError::BadRequest("Stock symbol is required".into()));
        }
        let cache_key = format!("ext:stock:{sym}");
        if let Ok(Some(cached)) = self.redis.get_json::<StockPriceInfo>(&cache_key).await {
            return Ok(cached);
        }
        if self.config.no_external_connection {
            return Err(HomarrError::Forbidden(
                "External connections are disabled".into(),
            ));
        }

        let encoded: String = url::form_urlencoded::byte_serialize(sym.as_bytes()).collect();
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{encoded}?interval=1d&range=5d"
        );
        let resp = self
            .http
            .standard()
            .get(&url)
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Stock request failed: {e}")))?;
        let raw: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Invalid stock JSON: {e}")))?;
        let meta = raw
            .get("chart")
            .and_then(|c| c.get("result"))
            .and_then(|r| r.as_array())
            .and_then(|a| a.first())
            .and_then(|item| item.get("meta"))
            .ok_or_else(|| HomarrError::NotFound(format!("Stock '{sym}' not found")))?;

        let price = meta
            .get("regularMarketPrice")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let prev = meta
            .get("chartPreviousClose")
            .or_else(|| meta.get("previousClose"))
            .and_then(|v| v.as_f64())
            .unwrap_or(price);
        let change = price - prev;
        let change_percent = if prev > 0.0 {
            (change / prev) * 100.0
        } else {
            0.0
        };
        let info = StockPriceInfo {
            symbol: sym,
            short_name: meta
                .get("shortName")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            currency: meta
                .get("currency")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            regular_market_price: price,
            previous_close: prev,
            change,
            change_percent,
        };
        let _ = self.redis.set_json_ex(&cache_key, &info, 300).await;
        Ok(info)
    }

    pub async fn get_latest_release(
        &self,
        provider: &str,
        identifier: &str,
    ) -> HomarrResult<ReleaseInfo> {
        let cache_key = format!("ext:rel:{provider}:{identifier}");
        if let Ok(Some(cached)) = self.redis.get_json::<ReleaseInfo>(&cache_key).await {
            return Ok(cached);
        }
        if self.config.no_external_connection {
            return Err(HomarrError::Forbidden(
                "External connections are disabled".into(),
            ));
        }

        let clean_id = identifier.trim();
        let url = match provider {
            "github" => format!("https://api.github.com/repos/{clean_id}/releases/latest"),
            "gitlab" => {
                let encoded: String =
                    url::form_urlencoded::byte_serialize(clean_id.as_bytes()).collect();
                format!("https://gitlab.com/api/v4/projects/{encoded}/releases")
            }
            "dockerHub" => {
                format!("https://hub.docker.com/v2/repositories/{clean_id}/tags?page_size=1")
            }
            "npm" => format!("https://registry.npmjs.org/{clean_id}/latest"),
            "codeberg" => format!("https://codeberg.org/api/v1/repos/{clean_id}/releases?limit=1"),
            other => {
                return Err(HomarrError::BadRequest(format!(
                    "Unsupported release provider '{other}'"
                )));
            }
        };

        let resp = self
            .http
            .standard()
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Release fetch failed: {e}")))?;
        let raw: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Invalid release JSON: {e}")))?;

        let info = match provider {
            "github" => ReleaseInfo {
                provider: provider.to_string(),
                identifier: clean_id.to_string(),
                latest_release: raw
                    .get("tag_name")
                    .or_else(|| raw.get("name"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                published_at: raw
                    .get("published_at")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                url: raw
                    .get("html_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                notes: raw.get("body").and_then(|v| v.as_str()).map(str::to_string),
            },
            "gitlab" | "codeberg" => {
                let item = raw
                    .as_array()
                    .and_then(|a| a.first())
                    .cloned()
                    .unwrap_or(Value::Null);
                ReleaseInfo {
                    provider: provider.to_string(),
                    identifier: clean_id.to_string(),
                    latest_release: item
                        .get("tag_name")
                        .or_else(|| item.get("name"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    published_at: item
                        .get("released_at")
                        .or_else(|| item.get("published_at"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    url: item
                        .get("_links")
                        .and_then(|l| l.get("self"))
                        .or_else(|| item.get("html_url"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    notes: item
                        .get("description")
                        .or_else(|| item.get("body"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                }
            }
            "dockerHub" => {
                let item = raw
                    .get("results")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .cloned()
                    .unwrap_or(Value::Null);
                ReleaseInfo {
                    provider: provider.to_string(),
                    identifier: clean_id.to_string(),
                    latest_release: item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    published_at: item
                        .get("last_updated")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    url: Some(format!("https://hub.docker.com/r/{clean_id}/tags")),
                    notes: None,
                }
            }
            "npm" => ReleaseInfo {
                provider: provider.to_string(),
                identifier: clean_id.to_string(),
                latest_release: raw
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                published_at: None,
                url: Some(format!("https://www.npmjs.com/package/{clean_id}")),
                notes: raw
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            },
            _ => unreachable!(),
        };

        let _ = self.redis.set_json_ex(&cache_key, &info, 900).await;
        Ok(info)
    }

    pub async fn ping_minecraft_server(&self, host: &str, port: u16) -> MinecraftStatus {
        if let Ok(status) = tcp_minecraft_slp(host, port).await {
            return status;
        }
        if !self.config.no_external_connection {
            let url = format!("https://api.mcsrvstat.us/3/{host}:{port}");
            if let Ok(resp) = self.http.standard().get(&url).send().await {
                if let Ok(json) = resp.json::<Value>().await {
                    let online = json
                        .get("online")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    return MinecraftStatus {
                        online,
                        host: host.to_string(),
                        port,
                        version: json
                            .get("version")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        motd: json
                            .get("motd")
                            .and_then(|m| m.get("clean"))
                            .and_then(|c| c.as_array())
                            .and_then(|a| a.first())
                            .and_then(|s| s.as_str())
                            .map(str::to_string),
                        players_online: json
                            .get("players")
                            .and_then(|p| p.get("online"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        players_max: json
                            .get("players")
                            .and_then(|p| p.get("max"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                    };
                }
            }
        }
        MinecraftStatus {
            online: false,
            host: host.to_string(),
            port,
            version: None,
            motd: None,
            players_online: 0,
            players_max: 0,
        }
    }

    pub async fn get_ddg_bangs(&self) -> HomarrResult<Vec<DuckDuckGoBang>> {
        if self.config.no_external_connection {
            return Ok(Vec::new());
        }
        let cache_key = "ext:ddg-bangs:v1";
        if let Ok(Some(cached)) = self.redis.get_json::<Vec<DuckDuckGoBang>>(cache_key).await {
            return Ok(cached);
        }
        let resp = self
            .http
            .standard()
            .get("https://duckduckgo.com/bang.js")
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("DDG bangs failed: {e}")))?;
        let mut bangs: Vec<DuckDuckGoBang> = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("DDG bangs JSON invalid: {e}")))?;
        for b in &mut bangs {
            b.t = b.t.trim().to_ascii_lowercase();
        }
        bangs.retain(|b| !b.t.is_empty());
        bangs.sort_by(|a, b| a.t.cmp(&b.t));
        let _ = self.redis.set_json_ex(cache_key, &bangs, 86_400).await;
        Ok(bangs)
    }

    pub async fn check_updates(&self) -> HomarrResult<UpdateCheckSummary> {
        let current = env!("CARGO_PKG_VERSION").to_string();
        let now = Utc::now().timestamp();
        let cache_key = format!("ext:updates:{current}");
        if let Ok(Some(cached)) = self.redis.get_json::<UpdateCheckSummary>(&cache_key).await {
            return Ok(cached);
        }
        if self.config.no_external_connection {
            return Ok(UpdateCheckSummary {
                current_version: current,
                latest_version: None,
                update_available: false,
                release_url: None,
                checked_at: now,
            });
        }
        let resp = self
            .http
            .standard()
            .get("https://api.github.com/repos/homarr-labs/homarr/releases/latest")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await;
        let summary = match resp {
            Ok(r) if r.status().is_success() => {
                let raw: Value = r.json().await.unwrap_or(Value::Null);
                let tag = raw
                    .get("tag_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim_start_matches('v').to_string());
                let release_url = raw
                    .get("html_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let update_available = tag
                    .as_deref()
                    .map(|latest| compare_semver_gt(latest, &current))
                    .unwrap_or(false);
                UpdateCheckSummary {
                    current_version: current,
                    latest_version: tag,
                    update_available,
                    release_url,
                    checked_at: now,
                }
            }
            _ => UpdateCheckSummary {
                current_version: current,
                latest_version: None,
                update_available: false,
                release_url: None,
                checked_at: now,
            },
        };
        let _ = self.redis.set_json_ex(&cache_key, &summary, 86_400).await;
        Ok(summary)
    }

    pub async fn build_analytics_snapshot(&self, db: &Database) -> HomarrResult<AnalyticsSnapshot> {
        let all_users = users::list_all(db).await?;
        let all_groups = groups::list_all(db).await?;
        let all_boards = boards::list_all(db).await?;
        let all_apps = apps::list_all(db).await?;
        let all_ints = integrations::list_all(db).await?;
        Ok(AnalyticsSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            users_count: all_users.len(),
            groups_count: all_groups.len(),
            boards_count: all_boards.len(),
            apps_count: all_apps.len(),
            integrations_count: all_ints.len(),
            generated_at: Utc::now().timestamp(),
        })
    }
}

fn compare_semver_gt(candidate: &str, current: &str) -> bool {
    let parse = |s: &str| -> [u64; 3] {
        let mut parts = [0u64; 3];
        for (i, p) in s.split('.').take(3).enumerate() {
            parts[i] = p
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|d| d.parse::<u64>().ok())
                .unwrap_or(0);
        }
        parts
    };
    parse(candidate) > parse(current)
}

async fn tcp_minecraft_slp(host: &str, port: u16) -> HomarrResult<MinecraftStatus> {
    // Resolve once and connect only to checked addresses, so DNS rebinding can't reach loopback.
    let addrs: Vec<std::net::SocketAddr> = tokio::time::timeout(
        Duration::from_secs(4),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| HomarrError::Upstream("Minecraft DNS lookup timeout".into()))?
    .map_err(|e| HomarrError::Upstream(format!("Minecraft DNS lookup failed: {e}")))?
    .collect();
    if addrs.is_empty() || addrs.iter().any(|a| homarr_common::is_blocked_ip(a.ip())) {
        return Err(HomarrError::Forbidden(
            "Minecraft server address is not allowed".into(),
        ));
    }
    let mut stream = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(&addrs[..]))
        .await
        .map_err(|_| HomarrError::Upstream("Minecraft TCP connect timeout".into()))?
        .map_err(|e| HomarrError::Upstream(format!("Minecraft TCP connect failed: {e}")))?;

    let mut packet = Vec::new();
    packet.push(0x00); // Handshake packet ID
    write_varint(&mut packet, 763); // Protocol version
    write_varint(&mut packet, host.len() as i32);
    packet.extend_from_slice(host.as_bytes());
    packet.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut packet, 1); // Next state: status

    let mut frame = Vec::new();
    write_varint(&mut frame, packet.len() as i32);
    frame.extend_from_slice(&packet);
    // Status request packet [length=1, packet_id=0]
    frame.extend_from_slice(&[0x01, 0x00]);

    stream
        .write_all(&frame)
        .await
        .map_err(|e| HomarrError::Upstream(e.to_string()))?;

    let _packet_len = read_varint(&mut stream).await?;
    let packet_id = read_varint(&mut stream).await?;
    if packet_id != 0 {
        return Err(HomarrError::Upstream(
            "Unexpected Minecraft packet ID".into(),
        ));
    }
    let str_len = read_varint(&mut stream).await? as usize;
    if str_len > 256 * 1024 {
        return Err(HomarrError::Upstream(
            "Minecraft status payload too large".into(),
        ));
    }
    let mut buf = vec![0u8; str_len];
    tokio::time::timeout(Duration::from_secs(4), stream.read_exact(&mut buf))
        .await
        .map_err(|_| HomarrError::Upstream("Minecraft read timeout".into()))?
        .map_err(|e| HomarrError::Upstream(e.to_string()))?;

    let status_json: Value =
        serde_json::from_slice(&buf).map_err(|e| HomarrError::Upstream(e.to_string()))?;
    let version = status_json
        .get("version")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let motd = match status_json.get("description") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(m)) => m.get("text").and_then(|v| v.as_str()).map(str::to_string),
        _ => None,
    };
    let players_online = status_json
        .get("players")
        .and_then(|p| p.get("online"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let players_max = status_json
        .get("players")
        .and_then(|p| p.get("max"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Ok(MinecraftStatus {
        online: true,
        host: host.to_string(),
        port,
        version,
        motd,
        players_online,
        players_max,
    })
}

fn write_varint(buf: &mut Vec<u8>, mut val: i32) {
    let mut u = val as u32;
    loop {
        if (u & !0x7F) == 0 {
            buf.push(u as u8);
            break;
        }
        buf.push(((u & 0x7F) | 0x80) as u8);
        u >>= 7;
    }
    val = val.wrapping_add(0);
    let _ = val;
}

async fn read_varint(stream: &mut TcpStream) -> HomarrResult<i32> {
    let mut num_read = 0u32;
    let mut result = 0i32;
    let mut byte = [0u8; 1];
    loop {
        tokio::time::timeout(Duration::from_secs(4), stream.read_exact(&mut byte))
            .await
            .map_err(|_| HomarrError::Upstream("VarInt timeout".into()))?
            .map_err(|e| HomarrError::Upstream(e.to_string()))?;
        let val = (byte[0] & 0x7F) as i32;
        result |= val << (7 * num_read);
        num_read += 1;
        if num_read > 5 {
            return Err(HomarrError::Upstream("VarInt too big".into()));
        }
        if (byte[0] & 0x80) == 0 {
            break;
        }
    }
    Ok(result)
}

/// Parses both RSS 2.0 (`<item>`) and Atom 1.0 (`<entry>`) XML feeds.
pub fn parse_rss_or_atom(xml: &str) -> Vec<FeedEntry> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut in_item = false;
    let mut feed_title: Option<String> = None;
    let mut current_tag = String::new();
    let mut title = String::new();
    let mut link = String::new();
    let mut published: Option<String> = None;
    let mut description: Option<String> = None;
    let mut categories: Vec<String> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "item" || name == "entry" {
                    in_item = true;
                    title.clear();
                    link.clear();
                    published = None;
                    description = None;
                    categories.clear();
                } else if in_item && name == "link" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"href" {
                            if let Ok(href) = attr.unescape_value() {
                                link = href.into_owned();
                            }
                        }
                    }
                } else if in_item && name == "category" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"term" {
                            if let Ok(term) = attr.unescape_value() {
                                categories.push(term.into_owned());
                            }
                        }
                    }
                }
                current_tag = name;
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if in_item && name == "link" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"href" {
                            if let Ok(href) = attr.unescape_value() {
                                if link.is_empty() {
                                    link = href.into_owned();
                                }
                            }
                        }
                    }
                } else if in_item && name == "category" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"term" {
                            if let Ok(term) = attr.unescape_value() {
                                categories.push(term.into_owned());
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if let Ok(txt) = t.unescape() {
                    let s = txt.trim();
                    if s.is_empty() {
                        buf.clear();
                        continue;
                    }
                    if !in_item && current_tag == "title" && feed_title.is_none() {
                        feed_title = Some(s.to_string());
                    } else if in_item {
                        match current_tag.as_str() {
                            "title" => title.push_str(s),
                            "link" => link.push_str(s),
                            "pubDate" | "published" | "updated" => {
                                published = Some(s.to_string());
                            }
                            "description" | "summary" | "content" => {
                                description = Some(s.to_string());
                            }
                            "category" => categories.push(s.to_string()),
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::CData(c)) => {
                let s = String::from_utf8_lossy(c.as_ref()).trim().to_string();
                if in_item && !s.is_empty() {
                    match current_tag.as_str() {
                        "title" => title.push_str(&s),
                        "description" | "summary" | "content" => description = Some(s),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "item" || name == "entry" {
                    if !title.is_empty() || !link.is_empty() {
                        entries.push(FeedEntry {
                            title: std::mem::take(&mut title),
                            link: std::mem::take(&mut link),
                            published: published.take(),
                            description: description.take(),
                            feed_title: feed_title.clone(),
                            categories: std::mem::take(&mut categories),
                        });
                    }
                    in_item = false;
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    entries
}

pub fn json_map_summary(entries: Vec<FeedEntry>) -> Value {
    json!({
        "entries": entries,
        "count": 0
    })
}
