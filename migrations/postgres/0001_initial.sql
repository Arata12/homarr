-- Complete Homarr v2 PostgreSQL schema (41 tables matching Drizzle snapshot 0054)

CREATE TABLE IF NOT EXISTS "user" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" text,
    "email" text,
    "email_verified" timestamp,
    "image" text,
    "password" text,
    "provider" varchar(64) NOT NULL DEFAULT 'credentials',
    "home_board_id" varchar(64),
    "mobile_home_board_id" varchar(64),
    "default_search_engine_id" varchar(64),
    "open_search_in_new_tab" boolean NOT NULL DEFAULT true,
    "ddg_bangs" boolean NOT NULL DEFAULT true,
    "color_scheme" varchar(5) NOT NULL DEFAULT 'dark',
    "byte_unit_system" varchar(7) NOT NULL DEFAULT 'decimal',
    "first_day_of_week" smallint NOT NULL DEFAULT 1,
    "ping_icons_enabled" boolean NOT NULL DEFAULT false,
    "enable_right_click_on_widgets" boolean NOT NULL DEFAULT true,
    "header_preferences" text NOT NULL DEFAULT '{"version":4,"visible":true,"searchDisplay":"input","logoDisplay":"logoAndText","tabsDisplay":"compact","zones":{"left":[{"type":"builtin","id":"logo"}],"center":[{"type":"builtin","id":"search"}],"right":[{"type":"builtin","id":"boardTabs"},{"type":"builtin","id":"boardEdit"},{"type":"builtin","id":"boardSettings"},{"type":"builtin","id":"user"}]}}',
    "completed_manage_tour" boolean NOT NULL DEFAULT false,
    "completed_board_tour" boolean NOT NULL DEFAULT false
);

CREATE TABLE IF NOT EXISTS "app" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "description" text,
    "icon_url" text NOT NULL,
    "href" text,
    "ping_url" text
);

CREATE TABLE IF NOT EXISTS "integration" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" text NOT NULL,
    "url" text NOT NULL,
    "external_url" text,
    "kind" varchar(128) NOT NULL,
    "app_id" varchar(128)
);
CREATE INDEX IF NOT EXISTS "integration__kind_idx" ON "integration" USING btree ("kind");

CREATE TABLE IF NOT EXISTS "board" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" varchar(256) NOT NULL,
    "is_public" boolean NOT NULL DEFAULT false,
    "creator_id" varchar(64),
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
    "disable_status" boolean NOT NULL DEFAULT false,
    CONSTRAINT "board_name_unique" UNIQUE("name")
);

CREATE TABLE IF NOT EXISTS "search_engine" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "icon_url" text NOT NULL,
    "name" varchar(64) NOT NULL,
    "short" varchar(8) NOT NULL,
    "description" text,
    "url_template" text,
    "type" varchar(64) NOT NULL DEFAULT 'generic',
    "integration_id" varchar(64),
    CONSTRAINT "search_engine_short_unique" UNIQUE("short")
);

CREATE TABLE IF NOT EXISTS "account" (
    "user_id" varchar(64) NOT NULL,
    "type" text NOT NULL,
    "provider" varchar(64) NOT NULL,
    "provider_account_id" varchar(64) NOT NULL,
    "refresh_token" text,
    "access_token" text,
    "expires_at" integer,
    "token_type" text,
    "scope" text,
    "id_token" text,
    "session_state" text,
    CONSTRAINT "account_provider_provider_account_id_pk" PRIMARY KEY("provider","provider_account_id")
);
CREATE INDEX IF NOT EXISTS "userId_idx" ON "account" USING btree ("user_id");

CREATE TABLE IF NOT EXISTS "session" (
    "session_token" varchar(512) PRIMARY KEY NOT NULL,
    "user_id" varchar(64) NOT NULL,
    "expires" timestamp NOT NULL
);
CREATE INDEX IF NOT EXISTS "user_id_idx" ON "session" USING btree ("user_id");

CREATE TABLE IF NOT EXISTS "verificationToken" (
    "identifier" varchar(64) NOT NULL,
    "token" varchar(512) NOT NULL,
    "expires" timestamp NOT NULL,
    CONSTRAINT "verificationToken_identifier_token_pk" PRIMARY KEY("identifier","token")
);

CREATE TABLE IF NOT EXISTS "apiKey" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "api_key" text NOT NULL,
    "read_only" boolean NOT NULL DEFAULT false,
    "oauth_client_id" text,
    "oauth_client_name" text,
    "oauth_redirect_uri" text,
    "oauth_issued_at" timestamp,
    "user_id" varchar(64) NOT NULL
);

CREATE TABLE IF NOT EXISTS "group" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" varchar(64) NOT NULL,
    "owner_id" varchar(64),
    "home_board_id" varchar(64),
    "mobile_home_board_id" varchar(64),
    "position" smallint NOT NULL,
    CONSTRAINT "group_name_unique" UNIQUE("name")
);

CREATE TABLE IF NOT EXISTS "groupMember" (
    "group_id" varchar(64) NOT NULL,
    "user_id" varchar(64) NOT NULL,
    CONSTRAINT "groupMember_group_id_user_id_pk" PRIMARY KEY("group_id","user_id")
);

CREATE TABLE IF NOT EXISTS "groupPermission" (
    "group_id" varchar(64) NOT NULL,
    "permission" text NOT NULL
);

CREATE TABLE IF NOT EXISTS "invite" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "token" varchar(512) NOT NULL,
    "expiration_date" timestamp NOT NULL,
    "creator_id" varchar(64) NOT NULL,
    CONSTRAINT "invite_token_unique" UNIQUE("token")
);

CREATE TABLE IF NOT EXISTS "media" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" varchar(512) NOT NULL,
    "content" bytea NOT NULL,
    "content_type" text NOT NULL,
    "size" integer NOT NULL,
    "created_at" timestamp NOT NULL DEFAULT now(),
    "creator_id" varchar(64)
);

CREATE TABLE IF NOT EXISTS "integrationSecret" (
    "kind" varchar(64) NOT NULL,
    "value" text NOT NULL,
    "updated_at" timestamp NOT NULL,
    "integration_id" varchar(64) NOT NULL,
    CONSTRAINT "integrationSecret_integration_id_kind_pk" PRIMARY KEY("integration_id","kind")
);
CREATE INDEX IF NOT EXISTS "integration_secret__kind_idx" ON "integrationSecret" USING btree ("kind");
CREATE INDEX IF NOT EXISTS "integration_secret__updated_at_idx" ON "integrationSecret" USING btree ("updated_at");

CREATE TABLE IF NOT EXISTS "integrationUserPermission" (
    "integration_id" varchar(64) NOT NULL,
    "user_id" varchar(64) NOT NULL,
    "permission" varchar(128) NOT NULL,
    CONSTRAINT "integrationUserPermission_integration_id_user_id_permission_pk" PRIMARY KEY("integration_id","user_id","permission")
);

CREATE TABLE IF NOT EXISTS "integrationGroupPermissions" (
    "integration_id" varchar(64) NOT NULL,
    "group_id" varchar(64) NOT NULL,
    "permission" varchar(128) NOT NULL,
    CONSTRAINT "integration_group_permission__pk" PRIMARY KEY("integration_id","group_id","permission")
);

CREATE TABLE IF NOT EXISTS "boardUserPermission" (
    "board_id" varchar(64) NOT NULL,
    "user_id" varchar(64) NOT NULL,
    "permission" varchar(128) NOT NULL,
    CONSTRAINT "boardUserPermission_board_id_user_id_permission_pk" PRIMARY KEY("board_id","user_id","permission")
);

CREATE TABLE IF NOT EXISTS "boardGroupPermission" (
    "board_id" varchar(64) NOT NULL,
    "group_id" varchar(64) NOT NULL,
    "permission" varchar(128) NOT NULL,
    CONSTRAINT "boardGroupPermission_board_id_group_id_permission_pk" PRIMARY KEY("board_id","group_id","permission")
);

CREATE TABLE IF NOT EXISTS "layout" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" varchar(32) NOT NULL,
    "board_id" varchar(64) NOT NULL,
    "column_count" smallint NOT NULL,
    "left_gutter_column_count" smallint NOT NULL DEFAULT 0,
    "right_gutter_column_count" smallint NOT NULL DEFAULT 0,
    "breakpoint" smallint NOT NULL DEFAULT 0,
    "role" varchar(16) NOT NULL DEFAULT 'custom',
    "resize_source" text
);

CREATE TABLE IF NOT EXISTS "section" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "board_id" varchar(64) NOT NULL,
    "kind" text NOT NULL,
    "x_offset" integer,
    "y_offset" integer,
    "name" text,
    "options" text DEFAULT '{"json":{}}'
);

CREATE TABLE IF NOT EXISTS "section_layout" (
    "section_id" varchar(64) NOT NULL,
    "layout_id" varchar(64) NOT NULL,
    "parent_section_id" varchar(64),
    "x_offset" integer NOT NULL,
    "y_offset" integer NOT NULL,
    "width" integer NOT NULL,
    "height" integer NOT NULL,
    CONSTRAINT "section_layout_section_id_layout_id_pk" PRIMARY KEY("section_id","layout_id")
);

CREATE TABLE IF NOT EXISTS "section_collapse_state" (
    "user_id" varchar(64) NOT NULL,
    "section_id" varchar(64) NOT NULL,
    "collapsed" boolean NOT NULL DEFAULT false,
    CONSTRAINT "section_collapse_state_user_id_section_id_pk" PRIMARY KEY("user_id","section_id")
);

CREATE TABLE IF NOT EXISTS "item" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "board_id" varchar(64) NOT NULL,
    "kind" text NOT NULL,
    "options" text NOT NULL DEFAULT '{"json":{}}',
    "advanced_options" text NOT NULL DEFAULT '{"json":{}}'
);

CREATE TABLE IF NOT EXISTS "item_layout" (
    "item_id" varchar(64) NOT NULL,
    "section_id" varchar(64) NOT NULL,
    "layout_id" varchar(64) NOT NULL,
    "x_offset" integer NOT NULL,
    "y_offset" integer NOT NULL,
    "width" integer NOT NULL,
    "height" integer NOT NULL,
    CONSTRAINT "item_layout_item_id_section_id_layout_id_pk" PRIMARY KEY("item_id","section_id","layout_id")
);

CREATE TABLE IF NOT EXISTS "user_item_state" (
    "user_id" varchar(64) NOT NULL,
    "item_id" varchar(64) NOT NULL,
    "key" text NOT NULL,
    "value" text NOT NULL,
    CONSTRAINT "user_item_state_user_id_item_id_key_pk" PRIMARY KEY("user_id","item_id","key")
);

CREATE TABLE IF NOT EXISTS "integration_item" (
    "item_id" varchar(64) NOT NULL,
    "integration_id" varchar(64) NOT NULL,
    CONSTRAINT "integration_item_item_id_integration_id_pk" PRIMARY KEY("item_id","integration_id")
);

CREATE TABLE IF NOT EXISTS "widget_secret" (
    "item_id" varchar(64) NOT NULL,
    "kind" varchar(64) NOT NULL,
    "value" text NOT NULL,
    "updated_at" timestamp NOT NULL,
    CONSTRAINT "widget_secret_item_id_kind_pk" PRIMARY KEY("item_id","kind")
);

CREATE TABLE IF NOT EXISTS "iconRepository" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "slug" varchar(150) NOT NULL
);

CREATE TABLE IF NOT EXISTS "icon" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" varchar(250) NOT NULL,
    "url" text NOT NULL,
    "checksum" text NOT NULL,
    "icon_repository_id" varchar(64) NOT NULL
);

CREATE TABLE IF NOT EXISTS "serverSetting" (
    "setting_key" varchar(64) PRIMARY KEY NOT NULL,
    "value" text NOT NULL DEFAULT '{"json":{}}',
    CONSTRAINT "serverSetting_settingKey_unique" UNIQUE("setting_key")
);

CREATE TABLE IF NOT EXISTS "assistant_configuration" (
    "id" varchar(64) PRIMARY KEY DEFAULT 'default' NOT NULL,
    "enabled" boolean NOT NULL DEFAULT false,
    "web_search_enabled" boolean NOT NULL DEFAULT false,
    "provider" varchar(32) NOT NULL DEFAULT 'openrouter',
    "base_url" varchar(2048) NOT NULL DEFAULT 'https://openrouter.ai/api/v1',
    "model_discovery_path" varchar(512) DEFAULT '/models',
    "encrypted_api_key" text,
    "encrypted_headers" text,
    "model_id" varchar(256),
    "updated_at" timestamp NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS "assistant_thread" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "user_id" varchar(64) NOT NULL,
    "title" varchar(256),
    "model_id" varchar(256),
    "created_at" timestamp NOT NULL DEFAULT now(),
    "updated_at" timestamp NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS "assistant_thread__user_id_updated_at_idx" ON "assistant_thread" USING btree ("user_id","updated_at");

CREATE TABLE IF NOT EXISTS "assistant_message" (
    "id" varchar(128) PRIMARY KEY NOT NULL,
    "thread_id" varchar(64) NOT NULL,
    "parent_id" varchar(128),
    "format" varchar(64) NOT NULL DEFAULT 'ai-sdk/v6',
    "content" text NOT NULL,
    "created_at" timestamp NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS "assistant_message__thread_id_created_at_idx" ON "assistant_message" USING btree ("thread_id","created_at");

CREATE TABLE IF NOT EXISTS "onboarding" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "step" varchar(64) NOT NULL,
    "previous_step" varchar(64)
);

CREATE TABLE IF NOT EXISTS "custom_widget_definition" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" varchar(256) NOT NULL,
    "description" text,
    "icon_url" text,
    "url" text NOT NULL,
    "auth_type" varchar(32) NOT NULL DEFAULT 'none',
    "header_name" varchar(256),
    "method" varchar(16) NOT NULL DEFAULT 'GET',
    "request_body" text,
    "display_type" varchar(32) NOT NULL DEFAULT 'singleValue',
    "display_config" text NOT NULL DEFAULT '{"json":{}}',
    "enabled" boolean NOT NULL DEFAULT true,
    "created_at" timestamp NOT NULL DEFAULT now(),
    "updated_at" timestamp NOT NULL DEFAULT now(),
    "creator_id" varchar(64)
);

CREATE TABLE IF NOT EXISTS "custom_widget_secret" (
    "kind" varchar(64) NOT NULL,
    "value" text NOT NULL,
    "updated_at" timestamp NOT NULL,
    "definition_id" varchar(64) NOT NULL,
    CONSTRAINT "custom_widget_secret_definition_id_kind_pk" PRIMARY KEY("definition_id","kind")
);

CREATE TABLE IF NOT EXISTS "custom_widget_v2_definition" (
    "id" varchar(64) PRIMARY KEY NOT NULL,
    "name" varchar(256) NOT NULL,
    "description" text,
    "icon_url" text,
    "sources" text NOT NULL,
    "requests" text NOT NULL,
    "options" text NOT NULL,
    "template" text NOT NULL,
    "workshop" text,
    "enabled" boolean NOT NULL DEFAULT true,
    "created_at" timestamp NOT NULL DEFAULT now(),
    "updated_at" timestamp NOT NULL DEFAULT now(),
    "creator_id" varchar(64)
);

CREATE TABLE IF NOT EXISTS "custom_widget_v2_secret" (
    "source_id" varchar(64) NOT NULL,
    "kind" varchar(64) NOT NULL,
    "encrypted_value" text NOT NULL,
    "updated_at" timestamp NOT NULL,
    "definition_id" varchar(64) NOT NULL,
    CONSTRAINT "custom_widget_v2_secret_definition_id_source_id_kind_pk" PRIMARY KEY("definition_id","source_id","kind")
);

CREATE TABLE IF NOT EXISTS "cron_job_configuration" (
    "name" varchar(256) PRIMARY KEY NOT NULL,
    "cron_expression" varchar(32) NOT NULL,
    "is_enabled" boolean NOT NULL DEFAULT true
);

CREATE TABLE IF NOT EXISTS "trusted_certificate_hostname" (
    "hostname" varchar(256) NOT NULL,
    "thumbprint" varchar(128) NOT NULL,
    "certificate" text NOT NULL,
    CONSTRAINT "trusted_certificate_hostname_hostname_thumbprint_pk" PRIMARY KEY("hostname","thumbprint")
);

DO $$
BEGIN
    ALTER TABLE "account" ADD CONSTRAINT "account_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "apiKey" ADD CONSTRAINT "apiKey_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "session" ADD CONSTRAINT "session_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "board" ADD CONSTRAINT "board_creator_id_user_id_fk" FOREIGN KEY ("creator_id") REFERENCES "user"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "user" ADD CONSTRAINT "user_home_board_id_board_id_fk" FOREIGN KEY ("home_board_id") REFERENCES "board"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "user" ADD CONSTRAINT "user_mobile_home_board_id_board_id_fk" FOREIGN KEY ("mobile_home_board_id") REFERENCES "board"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "user" ADD CONSTRAINT "user_default_search_engine_id_search_engine_id_fk" FOREIGN KEY ("default_search_engine_id") REFERENCES "search_engine"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "integration" ADD CONSTRAINT "integration_app_id_app_id_fk" FOREIGN KEY ("app_id") REFERENCES "app"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "integrationSecret" ADD CONSTRAINT "integrationSecret_integration_id_integration_id_fk" FOREIGN KEY ("integration_id") REFERENCES "integration"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "integrationUserPermission" ADD CONSTRAINT "integrationUserPermission_integration_id_integration_id_fk" FOREIGN KEY ("integration_id") REFERENCES "integration"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "integrationUserPermission" ADD CONSTRAINT "integrationUserPermission_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "group" ADD CONSTRAINT "group_owner_id_user_id_fk" FOREIGN KEY ("owner_id") REFERENCES "user"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "group" ADD CONSTRAINT "group_home_board_id_board_id_fk" FOREIGN KEY ("home_board_id") REFERENCES "board"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "group" ADD CONSTRAINT "group_mobile_home_board_id_board_id_fk" FOREIGN KEY ("mobile_home_board_id") REFERENCES "board"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "groupMember" ADD CONSTRAINT "groupMember_group_id_group_id_fk" FOREIGN KEY ("group_id") REFERENCES "group"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "groupMember" ADD CONSTRAINT "groupMember_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "groupPermission" ADD CONSTRAINT "groupPermission_group_id_group_id_fk" FOREIGN KEY ("group_id") REFERENCES "group"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "integrationGroupPermissions" ADD CONSTRAINT "integrationGroupPermissions_integration_id_integration_id_fk" FOREIGN KEY ("integration_id") REFERENCES "integration"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "integrationGroupPermissions" ADD CONSTRAINT "integrationGroupPermissions_group_id_group_id_fk" FOREIGN KEY ("group_id") REFERENCES "group"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "boardUserPermission" ADD CONSTRAINT "boardUserPermission_board_id_board_id_fk" FOREIGN KEY ("board_id") REFERENCES "board"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "boardUserPermission" ADD CONSTRAINT "boardUserPermission_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "boardGroupPermission" ADD CONSTRAINT "boardGroupPermission_board_id_board_id_fk" FOREIGN KEY ("board_id") REFERENCES "board"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "boardGroupPermission" ADD CONSTRAINT "boardGroupPermission_group_id_group_id_fk" FOREIGN KEY ("group_id") REFERENCES "group"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "invite" ADD CONSTRAINT "invite_creator_id_user_id_fk" FOREIGN KEY ("creator_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "media" ADD CONSTRAINT "media_creator_id_user_id_fk" FOREIGN KEY ("creator_id") REFERENCES "user"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "layout" ADD CONSTRAINT "layout_board_id_board_id_fk" FOREIGN KEY ("board_id") REFERENCES "board"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "section" ADD CONSTRAINT "section_board_id_board_id_fk" FOREIGN KEY ("board_id") REFERENCES "board"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "section_layout" ADD CONSTRAINT "section_layout_section_id_section_id_fk" FOREIGN KEY ("section_id") REFERENCES "section"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "section_layout" ADD CONSTRAINT "section_layout_layout_id_layout_id_fk" FOREIGN KEY ("layout_id") REFERENCES "layout"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "section_layout" ADD CONSTRAINT "section_layout_parent_section_id_section_id_fk" FOREIGN KEY ("parent_section_id") REFERENCES "section"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "section_collapse_state" ADD CONSTRAINT "section_collapse_state_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "section_collapse_state" ADD CONSTRAINT "section_collapse_state_section_id_section_id_fk" FOREIGN KEY ("section_id") REFERENCES "section"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "item" ADD CONSTRAINT "item_board_id_board_id_fk" FOREIGN KEY ("board_id") REFERENCES "board"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "item_layout" ADD CONSTRAINT "item_layout_item_id_item_id_fk" FOREIGN KEY ("item_id") REFERENCES "item"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "item_layout" ADD CONSTRAINT "item_layout_section_id_section_id_fk" FOREIGN KEY ("section_id") REFERENCES "section"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "item_layout" ADD CONSTRAINT "item_layout_layout_id_layout_id_fk" FOREIGN KEY ("layout_id") REFERENCES "layout"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "user_item_state" ADD CONSTRAINT "user_item_state_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "user_item_state" ADD CONSTRAINT "user_item_state_item_id_item_id_fk" FOREIGN KEY ("item_id") REFERENCES "item"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "integration_item" ADD CONSTRAINT "integration_item_item_id_item_id_fk" FOREIGN KEY ("item_id") REFERENCES "item"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "integration_item" ADD CONSTRAINT "integration_item_integration_id_integration_id_fk" FOREIGN KEY ("integration_id") REFERENCES "integration"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "widget_secret" ADD CONSTRAINT "widget_secret_item_id_item_id_fk" FOREIGN KEY ("item_id") REFERENCES "item"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "icon" ADD CONSTRAINT "icon_icon_repository_id_iconRepository_id_fk" FOREIGN KEY ("icon_repository_id") REFERENCES "iconRepository"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "assistant_thread" ADD CONSTRAINT "assistant_thread_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "user"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "assistant_message" ADD CONSTRAINT "assistant_message_thread_id_assistant_thread_id_fk" FOREIGN KEY ("thread_id") REFERENCES "assistant_thread"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "search_engine" ADD CONSTRAINT "search_engine_integration_id_integration_id_fk" FOREIGN KEY ("integration_id") REFERENCES "integration"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "custom_widget_definition" ADD CONSTRAINT "custom_widget_definition_creator_id_user_id_fk" FOREIGN KEY ("creator_id") REFERENCES "user"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "custom_widget_secret" ADD CONSTRAINT "custom_widget_secret_definition_id_custom_widget_definition_id_fk" FOREIGN KEY ("definition_id") REFERENCES "custom_widget_definition"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "custom_widget_v2_definition" ADD CONSTRAINT "custom_widget_v2_definition_creator_id_user_id_fk" FOREIGN KEY ("creator_id") REFERENCES "user"("id") ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE "custom_widget_v2_secret" ADD CONSTRAINT "custom_widget_v2_secret_definition_id_fk" FOREIGN KEY ("definition_id") REFERENCES "custom_widget_v2_definition"("id") ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
