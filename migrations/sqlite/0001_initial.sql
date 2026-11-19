-- Complete Homarr v2 SQLite schema (41 tables matching Drizzle snapshot 0054)

CREATE TABLE IF NOT EXISTS "user" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text,
    "email" text,
    "email_verified" integer,
    "image" text,
    "password" text,
    "provider" text NOT NULL DEFAULT 'credentials',
    "home_board_id" text REFERENCES "board"("id") ON DELETE SET NULL,
    "mobile_home_board_id" text REFERENCES "board"("id") ON DELETE SET NULL,
    "default_search_engine_id" text REFERENCES "search_engine"("id") ON DELETE SET NULL,
    "open_search_in_new_tab" integer NOT NULL DEFAULT 1,
    "ddg_bangs" integer NOT NULL DEFAULT 1,
    "color_scheme" text NOT NULL DEFAULT 'dark',
    "byte_unit_system" text NOT NULL DEFAULT 'decimal',
    "first_day_of_week" integer NOT NULL DEFAULT 1,
    "ping_icons_enabled" integer NOT NULL DEFAULT 0,
    "enable_right_click_on_widgets" integer NOT NULL DEFAULT 1,
    "header_preferences" text NOT NULL DEFAULT '{"version":4,"visible":true,"searchDisplay":"input","logoDisplay":"logoAndText","tabsDisplay":"compact","zones":{"left":[{"type":"builtin","id":"logo"}],"center":[{"type":"builtin","id":"search"}],"right":[{"type":"builtin","id":"boardTabs"},{"type":"builtin","id":"boardEdit"},{"type":"builtin","id":"boardSettings"},{"type":"builtin","id":"user"}]}}',
    "completed_manage_tour" integer NOT NULL DEFAULT 0,
    "completed_board_tour" integer NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS "account" (
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
    "type" text NOT NULL,
    "provider" text NOT NULL,
    "provider_account_id" text NOT NULL,
    "refresh_token" text,
    "access_token" text,
    "expires_at" integer,
    "token_type" text,
    "scope" text,
    "id_token" text,
    "session_state" text,
    PRIMARY KEY ("provider", "provider_account_id")
);
CREATE INDEX IF NOT EXISTS "userId_idx" ON "account" ("user_id");

CREATE TABLE IF NOT EXISTS "session" (
    "session_token" text PRIMARY KEY NOT NULL,
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
    "expires" integer NOT NULL
);
CREATE INDEX IF NOT EXISTS "user_id_idx" ON "session" ("user_id");

CREATE TABLE IF NOT EXISTS "verificationToken" (
    "identifier" text NOT NULL,
    "token" text NOT NULL,
    "expires" integer NOT NULL,
    PRIMARY KEY ("identifier", "token")
);

CREATE TABLE IF NOT EXISTS "apiKey" (
    "id" text PRIMARY KEY NOT NULL,
    "api_key" text NOT NULL,
    "read_only" integer NOT NULL DEFAULT 0,
    "oauth_client_id" text,
    "oauth_client_name" text,
    "oauth_redirect_uri" text,
    "oauth_issued_at" integer,
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "group" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "owner_id" text REFERENCES "user"("id") ON DELETE SET NULL,
    "home_board_id" text REFERENCES "board"("id") ON DELETE SET NULL,
    "mobile_home_board_id" text REFERENCES "board"("id") ON DELETE SET NULL,
    "position" integer NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS "group_name_unique" ON "group" ("name");

CREATE TABLE IF NOT EXISTS "groupMember" (
    "group_id" text NOT NULL REFERENCES "group"("id") ON DELETE CASCADE,
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
    PRIMARY KEY ("group_id", "user_id")
);

CREATE TABLE IF NOT EXISTS "groupPermission" (
    "group_id" text NOT NULL REFERENCES "group"("id") ON DELETE CASCADE,
    "permission" text NOT NULL
);

CREATE TABLE IF NOT EXISTS "invite" (
    "id" text PRIMARY KEY NOT NULL,
    "token" text NOT NULL,
    "expiration_date" integer NOT NULL,
    "creator_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE
);
CREATE UNIQUE INDEX IF NOT EXISTS "invite_token_unique" ON "invite" ("token");

CREATE TABLE IF NOT EXISTS "media" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "content" blob NOT NULL,
    "content_type" text NOT NULL,
    "size" integer NOT NULL,
    "created_at" integer NOT NULL DEFAULT (unixepoch()),
    "creator_id" text REFERENCES "user"("id") ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS "app" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "description" text,
    "icon_url" text NOT NULL,
    "href" text,
    "ping_url" text
);

CREATE TABLE IF NOT EXISTS "integration" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "url" text NOT NULL,
    "external_url" text,
    "kind" text NOT NULL,
    "app_id" text REFERENCES "app"("id") ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS "integration__kind_idx" ON "integration" ("kind");

CREATE TABLE IF NOT EXISTS "integrationSecret" (
    "kind" text NOT NULL,
    "value" text NOT NULL,
    "updated_at" integer NOT NULL,
    "integration_id" text NOT NULL REFERENCES "integration"("id") ON DELETE CASCADE,
    PRIMARY KEY ("integration_id", "kind")
);
CREATE INDEX IF NOT EXISTS "integration_secret__kind_idx" ON "integrationSecret" ("kind");
CREATE INDEX IF NOT EXISTS "integration_secret__updated_at_idx" ON "integrationSecret" ("updated_at");

CREATE TABLE IF NOT EXISTS "integrationUserPermission" (
    "integration_id" text NOT NULL REFERENCES "integration"("id") ON DELETE CASCADE,
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
    "permission" text NOT NULL,
    PRIMARY KEY ("integration_id", "user_id", "permission")
);

CREATE TABLE IF NOT EXISTS "integrationGroupPermissions" (
    "integration_id" text NOT NULL REFERENCES "integration"("id") ON DELETE CASCADE,
    "group_id" text NOT NULL REFERENCES "group"("id") ON DELETE CASCADE,
    "permission" text NOT NULL,
    PRIMARY KEY ("integration_id", "group_id", "permission")
);

CREATE TABLE IF NOT EXISTS "board" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "is_public" integer NOT NULL DEFAULT 0,
    "creator_id" text REFERENCES "user"("id") ON DELETE SET NULL,
    "page_title" text,
    "meta_title" text,
    "logo_image_url" text,
    "favicon_image_url" text,
    "background_image_url" text,
    "background_image_attachment" text NOT NULL DEFAULT 'fixed',
    "background_image_repeat" text NOT NULL DEFAULT 'no-repeat',
    "background_image_size" text NOT NULL DEFAULT 'cover',
    "primary_color" text,
    "secondary_color" text,
    "opacity" integer NOT NULL DEFAULT 100,
    "custom_css" text,
    "icon_color" text,
    "item_radius" text,
    "disable_status" integer NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS "board_name_unique" ON "board" ("name");

CREATE TABLE IF NOT EXISTS "boardUserPermission" (
    "board_id" text NOT NULL REFERENCES "board"("id") ON DELETE CASCADE,
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
    "permission" text NOT NULL,
    PRIMARY KEY ("board_id", "user_id", "permission")
);

CREATE TABLE IF NOT EXISTS "boardGroupPermission" (
    "board_id" text NOT NULL REFERENCES "board"("id") ON DELETE CASCADE,
    "group_id" text NOT NULL REFERENCES "group"("id") ON DELETE CASCADE,
    "permission" text NOT NULL,
    PRIMARY KEY ("board_id", "group_id", "permission")
);

CREATE TABLE IF NOT EXISTS "layout" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "board_id" text NOT NULL REFERENCES "board"("id") ON DELETE CASCADE,
    "column_count" integer NOT NULL,
    "left_gutter_column_count" integer NOT NULL DEFAULT 0,
    "right_gutter_column_count" integer NOT NULL DEFAULT 0,
    "breakpoint" integer NOT NULL DEFAULT 0,
    "role" text NOT NULL DEFAULT 'custom',
    "resize_source" text
);

CREATE TABLE IF NOT EXISTS "section" (
    "id" text PRIMARY KEY NOT NULL,
    "board_id" text NOT NULL REFERENCES "board"("id") ON DELETE CASCADE,
    "kind" text NOT NULL,
    "x_offset" integer,
    "y_offset" integer,
    "name" text,
    "options" text DEFAULT '{"json":{}}'
);

CREATE TABLE IF NOT EXISTS "section_layout" (
    "section_id" text NOT NULL REFERENCES "section"("id") ON DELETE CASCADE,
    "layout_id" text NOT NULL REFERENCES "layout"("id") ON DELETE CASCADE,
    "parent_section_id" text REFERENCES "section"("id") ON DELETE CASCADE,
    "x_offset" integer NOT NULL,
    "y_offset" integer NOT NULL,
    "width" integer NOT NULL,
    "height" integer NOT NULL,
    PRIMARY KEY ("section_id", "layout_id")
);

CREATE TABLE IF NOT EXISTS "section_collapse_state" (
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
    "section_id" text NOT NULL REFERENCES "section"("id") ON DELETE CASCADE,
    "collapsed" integer NOT NULL DEFAULT 0,
    PRIMARY KEY ("user_id", "section_id")
);

CREATE TABLE IF NOT EXISTS "item" (
    "id" text PRIMARY KEY NOT NULL,
    "board_id" text NOT NULL REFERENCES "board"("id") ON DELETE CASCADE,
    "kind" text NOT NULL,
    "options" text NOT NULL DEFAULT '{"json":{}}',
    "advanced_options" text NOT NULL DEFAULT '{"json":{}}'
);

CREATE TABLE IF NOT EXISTS "item_layout" (
    "item_id" text NOT NULL REFERENCES "item"("id") ON DELETE CASCADE,
    "section_id" text NOT NULL REFERENCES "section"("id") ON DELETE CASCADE,
    "layout_id" text NOT NULL REFERENCES "layout"("id") ON DELETE CASCADE,
    "x_offset" integer NOT NULL,
    "y_offset" integer NOT NULL,
    "width" integer NOT NULL,
    "height" integer NOT NULL,
    PRIMARY KEY ("item_id", "section_id", "layout_id")
);

CREATE TABLE IF NOT EXISTS "user_item_state" (
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
    "item_id" text NOT NULL REFERENCES "item"("id") ON DELETE CASCADE,
    "key" text NOT NULL,
    "value" text NOT NULL,
    PRIMARY KEY ("user_id", "item_id", "key")
);

CREATE TABLE IF NOT EXISTS "integration_item" (
    "item_id" text NOT NULL REFERENCES "item"("id") ON DELETE CASCADE,
    "integration_id" text NOT NULL REFERENCES "integration"("id") ON DELETE CASCADE,
    PRIMARY KEY ("item_id", "integration_id")
);

CREATE TABLE IF NOT EXISTS "widget_secret" (
    "item_id" text NOT NULL REFERENCES "item"("id") ON DELETE CASCADE,
    "kind" text NOT NULL,
    "value" text NOT NULL,
    "updated_at" integer NOT NULL,
    PRIMARY KEY ("item_id", "kind")
);

CREATE TABLE IF NOT EXISTS "iconRepository" (
    "id" text PRIMARY KEY NOT NULL,
    "slug" text NOT NULL
);

CREATE TABLE IF NOT EXISTS "icon" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "url" text NOT NULL,
    "checksum" text NOT NULL,
    "icon_repository_id" text NOT NULL REFERENCES "iconRepository"("id") ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "serverSetting" (
    "setting_key" text PRIMARY KEY NOT NULL,
    "value" text NOT NULL DEFAULT '{"json":{}}'
);
CREATE UNIQUE INDEX IF NOT EXISTS "serverSetting_settingKey_unique" ON "serverSetting" ("setting_key");

CREATE TABLE IF NOT EXISTS "assistant_configuration" (
    "id" text PRIMARY KEY NOT NULL DEFAULT 'default',
    "enabled" integer NOT NULL DEFAULT 0,
    "web_search_enabled" integer NOT NULL DEFAULT 0,
    "provider" text NOT NULL DEFAULT 'openrouter',
    "base_url" text NOT NULL DEFAULT 'https://openrouter.ai/api/v1',
    "model_discovery_path" text DEFAULT '/models',
    "encrypted_api_key" text,
    "encrypted_headers" text,
    "model_id" text,
    "updated_at" integer NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS "assistant_thread" (
    "id" text PRIMARY KEY NOT NULL,
    "user_id" text NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
    "title" text,
    "model_id" text,
    "created_at" integer NOT NULL DEFAULT (unixepoch()),
    "updated_at" integer NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS "assistant_thread__user_id_updated_at_idx" ON "assistant_thread" ("user_id", "updated_at");

CREATE TABLE IF NOT EXISTS "assistant_message" (
    "id" text PRIMARY KEY NOT NULL,
    "thread_id" text NOT NULL REFERENCES "assistant_thread"("id") ON DELETE CASCADE,
    "parent_id" text,
    "format" text NOT NULL DEFAULT 'ai-sdk/v6',
    "content" text NOT NULL,
    "created_at" integer NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS "assistant_message__thread_id_created_at_idx" ON "assistant_message" ("thread_id", "created_at");

CREATE TABLE IF NOT EXISTS "search_engine" (
    "id" text PRIMARY KEY NOT NULL,
    "icon_url" text NOT NULL,
    "name" text NOT NULL,
    "short" text NOT NULL,
    "description" text,
    "url_template" text,
    "type" text NOT NULL DEFAULT 'generic',
    "integration_id" text REFERENCES "integration"("id") ON DELETE CASCADE
);
CREATE UNIQUE INDEX IF NOT EXISTS "search_engine_short_unique" ON "search_engine" ("short");

CREATE TABLE IF NOT EXISTS "onboarding" (
    "id" text PRIMARY KEY NOT NULL,
    "step" text NOT NULL,
    "previous_step" text
);

CREATE TABLE IF NOT EXISTS "custom_widget_definition" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "description" text,
    "icon_url" text,
    "url" text NOT NULL,
    "auth_type" text NOT NULL DEFAULT 'none',
    "header_name" text,
    "method" text NOT NULL DEFAULT 'GET',
    "request_body" text,
    "display_type" text NOT NULL DEFAULT 'singleValue',
    "display_config" text NOT NULL DEFAULT '{"json":{}}',
    "enabled" integer NOT NULL DEFAULT 1,
    "created_at" integer NOT NULL DEFAULT (unixepoch()),
    "updated_at" integer NOT NULL DEFAULT (unixepoch()),
    "creator_id" text REFERENCES "user"("id") ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS "custom_widget_secret" (
    "kind" text NOT NULL,
    "value" text NOT NULL,
    "updated_at" integer NOT NULL,
    "definition_id" text NOT NULL REFERENCES "custom_widget_definition"("id") ON DELETE CASCADE,
    PRIMARY KEY ("definition_id", "kind")
);

CREATE TABLE IF NOT EXISTS "custom_widget_v2_definition" (
    "id" text PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "description" text,
    "icon_url" text,
    "sources" text NOT NULL,
    "requests" text NOT NULL,
    "options" text NOT NULL,
    "template" text NOT NULL,
    "workshop" text,
    "enabled" integer NOT NULL DEFAULT 1,
    "created_at" integer NOT NULL DEFAULT (unixepoch()),
    "updated_at" integer NOT NULL DEFAULT (unixepoch()),
    "creator_id" text REFERENCES "user"("id") ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS "custom_widget_v2_secret" (
    "source_id" text NOT NULL,
    "kind" text NOT NULL,
    "encrypted_value" text NOT NULL,
    "updated_at" integer NOT NULL,
    "definition_id" text NOT NULL REFERENCES "custom_widget_v2_definition"("id") ON DELETE CASCADE,
    PRIMARY KEY ("definition_id", "source_id", "kind")
);

CREATE TABLE IF NOT EXISTS "cron_job_configuration" (
    "name" text PRIMARY KEY NOT NULL,
    "cron_expression" text NOT NULL,
    "is_enabled" integer NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS "trusted_certificate_hostname" (
    "hostname" text NOT NULL,
    "thumbprint" text NOT NULL,
    "certificate" text NOT NULL,
    PRIMARY KEY ("hostname", "thumbprint")
);
