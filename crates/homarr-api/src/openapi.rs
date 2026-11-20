use axum::Json;
use serde_json::{json, Map, Value};

pub async fn openapi_spec() -> Json<Value> {
    let mut paths = Map::new();
    let specs: &[(&str, Value)] = &[
        (
            "/api/health/live",
            json!({ "get": { "summary": "Liveness probe" } }),
        ),
        (
            "/api/health/ready",
            json!({ "get": { "summary": "Readiness probe (DB & Redis)" } }),
        ),
        (
            "/api/info",
            json!({ "get": { "summary": "Instance metadata and entity counts" } }),
        ),
        (
            "/api/updates",
            json!({ "get": { "summary": "Check Homarr releases for updates" } }),
        ),
        (
            "/api/analytics",
            json!({ "get": { "summary": "Admin instance analytics snapshot" } }),
        ),
        (
            "/api/openapi",
            json!({ "get": { "summary": "OpenAPI 3.1 specification" } }),
        ),
        (
            "/api/auth/login",
            json!({ "post": { "summary": "Sign in with username/password" } }),
        ),
        (
            "/api/auth/logout",
            json!({ "post": { "summary": "Sign out and revoke session" } }),
        ),
        (
            "/api/auth/session",
            json!({ "get": { "summary": "Current session metadata" } }),
        ),
        (
            "/api/auth/register",
            json!({ "post": { "summary": "Register initial admin or invited user" } }),
        ),
        (
            "/api/api-keys",
            json!({ "get": { "summary": "List user API keys" }, "post": { "summary": "Create API key" } }),
        ),
        (
            "/api/api-keys/{id}",
            json!({ "delete": { "summary": "Delete API key" } }),
        ),
        (
            "/api/boards",
            json!({ "get": { "summary": "List visible boards" }, "post": { "summary": "Create board" } }),
        ),
        (
            "/api/boards/by-name/{name}",
            json!({ "get": { "summary": "Get full board by name" } }),
        ),
        (
            "/api/boards/import",
            json!({ "post": { "summary": "Import board bundle" } }),
        ),
        (
            "/api/boards/{id}",
            json!({ "get": { "summary": "Get full board by ID" }, "patch": { "summary": "Update board" }, "delete": { "summary": "Delete board" } }),
        ),
        (
            "/api/boards/{id}/layout",
            json!({ "put": { "summary": "Save sections & items layout" } }),
        ),
        (
            "/api/boards/{id}/items",
            json!({ "post": { "summary": "Add board item" } }),
        ),
        (
            "/api/boards/{id}/items/{item_id}",
            json!({ "patch": { "summary": "Update board item" }, "delete": { "summary": "Delete board item" } }),
        ),
        (
            "/api/boards/{id}/sections",
            json!({ "post": { "summary": "Add board section" } }),
        ),
        (
            "/api/boards/{id}/sections/{section_id}",
            json!({ "delete": { "summary": "Delete board section" } }),
        ),
        (
            "/api/boards/{id}/sections/{section_id}/collapse",
            json!({ "put": { "summary": "Set your collapsed state of a container section" } }),
        ),
        (
            "/api/boards/{id}/permissions",
            json!({ "get": { "summary": "Get board permissions" }, "put": { "summary": "Set board permissions" } }),
        ),
        (
            "/api/boards/{id}/duplicate",
            json!({ "post": { "summary": "Duplicate board" } }),
        ),
        (
            "/api/boards/{id}/export",
            json!({ "get": { "summary": "Export board JSON" } }),
        ),
        (
            "/api/apps",
            json!({ "get": { "summary": "List applications" }, "post": { "summary": "Create application" } }),
        ),
        (
            "/api/apps/ping",
            json!({ "post": { "summary": "Ping an application URL" } }),
        ),
        (
            "/api/apps/{id}",
            json!({ "get": { "summary": "Get application" }, "patch": { "summary": "Update application" }, "delete": { "summary": "Delete application" } }),
        ),
        (
            "/api/integrations/kinds",
            json!({ "get": { "summary": "List 92 integration kinds" } }),
        ),
        (
            "/api/integrations",
            json!({ "get": { "summary": "List integrations" }, "post": { "summary": "Create integration" } }),
        ),
        (
            "/api/integrations/{id}",
            json!({ "get": { "summary": "Get integration" }, "patch": { "summary": "Update integration" }, "delete": { "summary": "Delete integration" } }),
        ),
        (
            "/api/integrations/{id}/test",
            json!({ "post": { "summary": "Test integration connection" } }),
        ),
        (
            "/api/integrations/{id}/data",
            json!({ "get": { "summary": "Fetch integration telemetry" } }),
        ),
        (
            "/api/integrations/{id}/permissions",
            json!({ "get": { "summary": "Get integration permissions" }, "put": { "summary": "Set integration permissions" } }),
        ),
        (
            "/api/users",
            json!({ "get": { "summary": "List users" }, "post": { "summary": "Create user" } }),
        ),
        (
            "/api/users/me/preferences",
            json!({ "patch": { "summary": "Update current user preferences" } }),
        ),
        (
            "/api/users/{id}",
            json!({ "get": { "summary": "Get user" }, "patch": { "summary": "Update user" }, "delete": { "summary": "Delete user" } }),
        ),
        (
            "/api/groups",
            json!({ "get": { "summary": "List groups" }, "post": { "summary": "Create group" } }),
        ),
        (
            "/api/groups/{id}",
            json!({ "get": { "summary": "Get group" }, "patch": { "summary": "Update group" }, "delete": { "summary": "Delete group" } }),
        ),
        (
            "/api/groups/{id}/permissions",
            json!({ "put": { "summary": "Set group permissions" } }),
        ),
        (
            "/api/groups/{id}/members/{user_id}",
            json!({ "post": { "summary": "Add member to group" }, "delete": { "summary": "Remove member from group" } }),
        ),
        (
            "/api/invites",
            json!({ "get": { "summary": "List registration invites" }, "post": { "summary": "Create invite" } }),
        ),
        (
            "/api/invites/{id}",
            json!({ "delete": { "summary": "Delete invite" } }),
        ),
        (
            "/api/search-engines",
            json!({ "get": { "summary": "List search engines" }, "post": { "summary": "Create search engine" } }),
        ),
        (
            "/api/search-engines/{id}",
            json!({ "patch": { "summary": "Update search engine" }, "delete": { "summary": "Delete search engine" } }),
        ),
        (
            "/api/search/ddg-bangs",
            json!({ "get": { "summary": "Search DuckDuckGo bangs" } }),
        ),
        (
            "/api/icons",
            json!({ "get": { "summary": "Search icon catalog" } }),
        ),
        (
            "/api/icons/sync",
            json!({ "post": { "summary": "Sync icon repositories" } }),
        ),
        (
            "/api/icons/favicon",
            json!({ "get": { "summary": "Fetch website favicon" } }),
        ),
        (
            "/api/medias",
            json!({ "get": { "summary": "List uploaded media" }, "post": { "summary": "Upload media file" } }),
        ),
        (
            "/api/medias/{id}",
            json!({ "get": { "summary": "Serve uploaded media bytes" }, "delete": { "summary": "Delete media" } }),
        ),
        (
            "/api/image-proxy/forward",
            json!({ "post": { "summary": "Create encrypted forwarded image URL" } }),
        ),
        (
            "/api/image-proxy/{id}",
            json!({ "get": { "summary": "Resolve proxied or persistent image" } }),
        ),
        (
            "/api/widgets/catalog",
            json!({ "get": { "summary": "List 67 widget kinds" } }),
        ),
        (
            "/api/widgets/render/{kind}",
            json!({ "get": { "summary": "Render widget HTML fragment" } }),
        ),
        (
            "/api/widgets/resolve",
            json!({ "post": { "summary": "Resolve widget live JSON data" } }),
        ),
        (
            "/api/custom-widgets",
            json!({ "get": { "summary": "List Custom Widget v2 definitions" }, "post": { "summary": "Create Custom Widget v2" } }),
        ),
        (
            "/api/custom-widgets/{id}",
            json!({ "get": { "summary": "Get Custom Widget v2" }, "patch": { "summary": "Update Custom Widget v2" }, "delete": { "summary": "Delete Custom Widget v2" } }),
        ),
        (
            "/api/custom-widgets/{id}/execute",
            json!({ "post": { "summary": "Execute Custom Widget v2 request" } }),
        ),
        (
            "/api/workshop/listings",
            json!({ "get": { "summary": "List Workshop Custom Widget packages" } }),
        ),
        (
            "/api/workshop/install",
            json!({ "post": { "summary": "Install Workshop listing" } }),
        ),
        (
            "/api/docker/containers",
            json!({ "get": { "summary": "List Docker containers" } }),
        ),
        (
            "/api/docker/containers/{id}/{action}",
            json!({ "post": { "summary": "Execute container action" } }),
        ),
        (
            "/api/docker/containers/{id}/logs",
            json!({ "get": { "summary": "Fetch container logs" } }),
        ),
        (
            "/api/docker/discover",
            json!({ "get": { "summary": "Discover Homarr services from Docker" } }),
        ),
        (
            "/api/kubernetes/{resource}",
            json!({ "get": { "summary": "Query Kubernetes cluster resources" } }),
        ),
        (
            "/api/external/weather",
            json!({ "get": { "summary": "Open-Meteo weather" } }),
        ),
        (
            "/api/external/air-quality",
            json!({ "get": { "summary": "Open-Meteo air quality" } }),
        ),
        (
            "/api/external/cities",
            json!({ "get": { "summary": "Geocode city names" } }),
        ),
        (
            "/api/external/rss",
            json!({ "get": { "summary": "Fetch RSS/Atom feeds" } }),
        ),
        (
            "/api/external/stocks",
            json!({ "get": { "summary": "Fetch stock quote" } }),
        ),
        (
            "/api/external/releases",
            json!({ "get": { "summary": "Fetch latest package/repo release" } }),
        ),
        (
            "/api/external/minecraft",
            json!({ "get": { "summary": "Ping Minecraft server status" } }),
        ),
        (
            "/api/settings",
            json!({ "get": { "summary": "Get server settings" }, "patch": { "summary": "Update server settings" } }),
        ),
        (
            "/api/onboarding",
            json!({ "get": { "summary": "Get onboarding wizard state" }, "post": { "summary": "Update onboarding step" } }),
        ),
        (
            "/api/cron-jobs",
            json!({ "get": { "summary": "List background cron jobs" } }),
        ),
        (
            "/api/cron-jobs/{name}/trigger",
            json!({ "post": { "summary": "Manually run cron job" } }),
        ),
        (
            "/api/cron-jobs/{name}",
            json!({ "patch": { "summary": "Update cron schedule/enabled" } }),
        ),
        (
            "/api/certificates",
            json!({ "get": { "summary": "List custom CA certificates" }, "post": { "summary": "Upload PEM certificate" } }),
        ),
        (
            "/api/certificates/{name}",
            json!({ "delete": { "summary": "Delete certificate" } }),
        ),
        (
            "/api/logs",
            json!({ "get": { "summary": "Fetch recent server logs" } }),
        ),
        (
            "/api/backup/export",
            json!({ "get": { "summary": "Export full JSON backup bundle" } }),
        ),
        (
            "/api/backup/import",
            json!({ "post": { "summary": "Import JSON backup bundle" } }),
        ),
        (
            "/api/assistant/config",
            json!({ "get": { "summary": "Get assistant LLM config" }, "put": { "summary": "Save assistant config" } }),
        ),
        (
            "/api/assistant/threads",
            json!({ "get": { "summary": "List assistant threads" }, "post": { "summary": "Create assistant thread" } }),
        ),
        (
            "/api/assistant/threads/{id}",
            json!({ "delete": { "summary": "Delete assistant thread" } }),
        ),
        (
            "/api/assistant/threads/{id}/messages",
            json!({ "get": { "summary": "List thread messages" }, "post": { "summary": "Send message to assistant" } }),
        ),
        (
            "/api/mcp",
            json!({ "post": { "summary": "MCP JSON-RPC 2.0 endpoint" } }),
        ),
        (
            "/websockets",
            json!({ "get": { "summary": "WebSocket event stream (session cookie only)" } }),
        ),
    ];
    for (k, v) in specs {
        paths.insert((*k).to_string(), v.clone());
    }

    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Homarr API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "REST, WebSocket, and MCP HTTP API for Homarr (Rust Edition)"
        },
        "components": {
            "securitySchemes": {
                "apiKey": { "type": "apiKey", "in": "header", "name": "ApiKey" },
                "bearerAuth": { "type": "http", "scheme": "bearer" },
                "cookieAuth": { "type": "apiKey", "in": "cookie", "name": "homarr.session-token" }
            }
        },
        "paths": Value::Object(paths)
    }))
}
