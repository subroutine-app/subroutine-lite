
CREATE TABLE users (
    id          UUID        PRIMARY KEY,
    auth_issuer  TEXT COLLATE "C" NOT NULL,
    auth_subject TEXT COLLATE "C" NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    change_seq  BIGINT      NOT NULL DEFAULT 0,
    dataset_id  UUID        NOT NULL DEFAULT gen_random_uuid(),
    CONSTRAINT users_id_not_nil CHECK (id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT users_auth_identity_nonempty CHECK (
        auth_issuer <> '' AND auth_subject <> ''
    ),
    CONSTRAINT users_auth_identity_key UNIQUE (auth_issuer, auth_subject),
    CONSTRAINT users_change_seq_nonnegative CHECK (change_seq >= 0),
    CONSTRAINT users_dataset_id_not_nil CHECK (
        dataset_id <> '00000000-0000-0000-0000-000000000000'
    ),
    CONSTRAINT users_dataset_id_key UNIQUE (dataset_id),
    CONSTRAINT users_id_dataset_id_key UNIQUE (id, dataset_id)
);


CREATE TABLE actions (
    id                 UUID        PRIMARY KEY,

    recurrence_id      UUID        NOT NULL,
    routine_id         UUID,
    template_id        UUID,
    title              TEXT        NOT NULL,
    content            TEXT,
    queued             BOOLEAN     NOT NULL DEFAULT FALSE,

    pinned             BOOLEAN     NOT NULL DEFAULT FALSE,
    start_at           TIMESTAMPTZ,
    start_date         DATE,
    duration           TEXT,
    completion         TIMESTAMPTZ,
    recurrence         JSONB,
    source_provider    TEXT,
    source_external_id TEXT,
    deleted            BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0,

    CONSTRAINT actions_start_single_shape CHECK (
        num_nonnulls(start_at, start_date) <= 1
    ),
    CONSTRAINT actions_duration_iso8601 CHECK (duration IS NULL OR duration LIKE 'P%')
);

CREATE INDEX actions_recurrence_id_idx ON actions (recurrence_id);
CREATE INDEX actions_queued_idx ON actions (queued) WHERE deleted = FALSE;

CREATE TABLE action_templates (
    id         UUID        PRIMARY KEY,
    title      TEXT        NOT NULL,
    content    TEXT,

    naive_time TIME,
    duration   TEXT,
    recurrence JSONB,
    deleted    BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    sort_order BIGINT NOT NULL DEFAULT 9223372036854775807,
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0,
    CONSTRAINT action_templates_duration_iso8601 CHECK (
        duration IS NULL OR duration LIKE 'P%'
    )
);

CREATE TABLE events (
    id                 UUID        PRIMARY KEY,
    lineage_id         UUID        NOT NULL,
    template_id        UUID,
    title              TEXT        NOT NULL,
    content            TEXT,
    start_at           TIMESTAMPTZ NOT NULL,
    duration           TEXT        NOT NULL,
    recurrence         JSONB,
    source_provider    TEXT,
    source_external_id TEXT,

    source_busy        BOOLEAN,
    busy_override      BOOLEAN,
    deleted            BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0,
    CONSTRAINT events_duration_iso8601 CHECK (duration LIKE 'P%')
);

CREATE INDEX events_start_at_idx ON events (start_at) WHERE deleted = FALSE;
CREATE INDEX events_lineage_id_idx ON events (lineage_id);

CREATE TABLE event_templates (
    id                 UUID        PRIMARY KEY,
    lineage_id         UUID        NOT NULL,
    title              TEXT        NOT NULL,
    content            TEXT,
    duration           TEXT        NOT NULL,
    recurrence         JSONB,
    source_provider    TEXT,
    source_external_id TEXT,
    source_busy        BOOLEAN,
    busy_override      BOOLEAN,
    deleted            BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    sort_order BIGINT NOT NULL DEFAULT 9223372036854775807,
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0,
    CONSTRAINT event_templates_duration_iso8601 CHECK (duration LIKE 'P%')
);

CREATE TABLE signals (
    id          UUID        PRIMARY KEY,
    lineage_id  UUID        NOT NULL,
    template_id UUID,
    title       TEXT        NOT NULL,
    content     TEXT,
    datetime    TIMESTAMPTZ NOT NULL,
    recurrence  JSONB,
    deleted     BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0
);

CREATE INDEX signals_datetime_idx ON signals (datetime) WHERE deleted = FALSE;
CREATE INDEX signals_lineage_id_idx ON signals (lineage_id);

CREATE TABLE signal_templates (
    id         UUID        PRIMARY KEY,
    lineage_id UUID        NOT NULL,
    title      TEXT        NOT NULL,
    content    TEXT,
    recurrence JSONB,
    deleted    BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE markers (
    id                 UUID        PRIMARY KEY,
    lineage_id         UUID        NOT NULL,
    template_id        UUID,
    title              TEXT        NOT NULL,
    content            TEXT,
    date               DATE        NOT NULL,

    end_date           DATE,
    recurrence         JSONB,
    source_provider    TEXT,
    source_external_id TEXT,
    deleted            BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0,
    CONSTRAINT markers_end_date_not_before_date CHECK (
        end_date IS NULL OR end_date >= date
    )
);

CREATE INDEX markers_date_idx ON markers (date) WHERE deleted = FALSE;
CREATE INDEX markers_lineage_id_idx ON markers (lineage_id);

CREATE TABLE marker_templates (
    id                 UUID        PRIMARY KEY,
    lineage_id         UUID        NOT NULL,
    title              TEXT        NOT NULL,
    content            TEXT,
    span_days          INTEGER     NOT NULL DEFAULT 1,
    recurrence         JSONB,
    source_provider    TEXT,
    source_external_id TEXT,
    deleted            BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0,
    CONSTRAINT marker_templates_span_days_positive CHECK (span_days >= 1)
);

CREATE TABLE routines (
    id          UUID        PRIMARY KEY,
    title       TEXT        NOT NULL,
    content     TEXT,
    target_at   TIMESTAMPTZ,
    target_date DATE,
    recurrence  JSONB,
    deleted     BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    recurrence_id UUID NOT NULL,
    position BIGINT,
    user_id UUID NOT NULL REFERENCES users (id),
    change_seq BIGINT NOT NULL DEFAULT 0,
    CONSTRAINT routines_user_id_id_key UNIQUE (user_id, id),
    CONSTRAINT routines_target_single_shape CHECK (
        num_nonnulls(target_at, target_date) <= 1
    )
);


CREATE TABLE routine_steps (
    id         UUID    PRIMARY KEY,
    routine_id UUID    NOT NULL,
    title      TEXT    NOT NULL,
    duration   TEXT,
    position   INTEGER NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id),
    CONSTRAINT routine_steps_user_id_routine_id_fkey
        FOREIGN KEY (user_id, routine_id) REFERENCES routines (user_id, id) ON DELETE CASCADE,
    CONSTRAINT routine_steps_duration_iso8601 CHECK (
        duration IS NULL OR duration LIKE 'P%'
    )
);

CREATE INDEX routine_steps_routine_id_idx ON routine_steps (routine_id);

CREATE TABLE integration_items (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    provider_id      TEXT        NOT NULL,
    external_id      TEXT        NOT NULL,
    external_version TEXT,
    item_type        TEXT        NOT NULL
                                 CHECK (item_type IN ('action', 'event', 'marker', 'signal')),
    internal_uuid    UUID        NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ignored BOOLEAN NOT NULL DEFAULT FALSE,
    user_id UUID NOT NULL REFERENCES users (id),
    CONSTRAINT integration_items_user_provider_external_key
        UNIQUE (user_id, provider_id, external_id)
);

CREATE INDEX integration_items_internal_idx
    ON integration_items (internal_uuid);

CREATE INDEX routines_position_idx
    ON routines (position, created_at, id)
    WHERE deleted = FALSE;

CREATE INDEX actions_user_active_created_at_idx
    ON actions (user_id, created_at)
    WHERE deleted = FALSE;
CREATE INDEX actions_user_active_queued_idx
    ON actions (user_id, queued, created_at)
    WHERE deleted = FALSE;
CREATE INDEX actions_user_recurrence_id_idx
    ON actions (user_id, recurrence_id);

CREATE INDEX action_templates_user_active_order_idx
    ON action_templates (user_id, sort_order, created_at)
    WHERE deleted = FALSE;

CREATE INDEX events_user_active_start_at_idx
    ON events (user_id, start_at)
    WHERE deleted = FALSE;
CREATE INDEX events_user_lineage_id_idx
    ON events (user_id, lineage_id);

CREATE INDEX event_templates_user_active_order_idx
    ON event_templates (user_id, sort_order, created_at)
    WHERE deleted = FALSE;
CREATE INDEX event_templates_user_lineage_id_idx
    ON event_templates (user_id, lineage_id);

CREATE INDEX markers_user_active_date_idx
    ON markers (user_id, date)
    WHERE deleted = FALSE;
CREATE INDEX markers_user_lineage_id_idx
    ON markers (user_id, lineage_id);

CREATE INDEX marker_templates_user_active_created_at_idx
    ON marker_templates (user_id, created_at)
    WHERE deleted = FALSE;
CREATE INDEX marker_templates_user_lineage_id_idx
    ON marker_templates (user_id, lineage_id);

CREATE INDEX signals_user_active_datetime_idx
    ON signals (user_id, datetime)
    WHERE deleted = FALSE;
CREATE INDEX signals_user_lineage_id_idx
    ON signals (user_id, lineage_id);

CREATE INDEX signal_templates_user_active_created_at_idx
    ON signal_templates (user_id, created_at)
    WHERE deleted = FALSE;
CREATE INDEX signal_templates_user_lineage_id_idx
    ON signal_templates (user_id, lineage_id);

CREATE INDEX routines_user_active_position_idx
    ON routines (user_id, position, created_at, id)
    WHERE deleted = FALSE;
CREATE INDEX routines_user_recurrence_id_idx
    ON routines (user_id, recurrence_id);

CREATE INDEX routine_steps_user_routine_position_idx
    ON routine_steps (user_id, routine_id, position);

CREATE INDEX integration_items_user_internal_idx
    ON integration_items (user_id, internal_uuid);

CREATE INDEX actions_user_change_seq_idx ON actions (user_id, change_seq);
CREATE INDEX action_templates_user_change_seq_idx ON action_templates (user_id, change_seq);
CREATE INDEX events_user_change_seq_idx ON events (user_id, change_seq);
CREATE INDEX event_templates_user_change_seq_idx ON event_templates (user_id, change_seq);
CREATE INDEX markers_user_change_seq_idx ON markers (user_id, change_seq);
CREATE INDEX marker_templates_user_change_seq_idx ON marker_templates (user_id, change_seq);
CREATE INDEX signals_user_change_seq_idx ON signals (user_id, change_seq);
CREATE INDEX signal_templates_user_change_seq_idx ON signal_templates (user_id, change_seq);
CREATE INDEX routines_user_change_seq_idx ON routines (user_id, change_seq);

CREATE TABLE administrative_audit_events (
    id             UUID        PRIMARY KEY,
    event_type     TEXT        NOT NULL,
    actor          TEXT        NOT NULL,
    reason         TEXT        NOT NULL,
    target_user_id UUID        NOT NULL REFERENCES users (id),
    details        JSONB       NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT administrative_audit_events_id_not_nil CHECK (
        id <> '00000000-0000-0000-0000-000000000000'
    ),
    CONSTRAINT administrative_audit_events_event_type_not_blank CHECK (
        BTRIM(event_type) <> ''
    ),
    CONSTRAINT administrative_audit_events_actor_not_blank CHECK (
        BTRIM(actor) <> ''
    ),
    CONSTRAINT administrative_audit_events_reason_not_blank CHECK (
        BTRIM(reason) <> ''
    ),
    CONSTRAINT administrative_audit_events_details_object CHECK (
        JSONB_TYPEOF(details) = 'object'
    )
);

CREATE INDEX administrative_audit_events_target_created_at_idx
    ON administrative_audit_events (target_user_id, created_at DESC);


CREATE TABLE mutation_receipts (
    user_id          UUID        NOT NULL,
    dataset_id       UUID        NOT NULL,
    mutation_id      UUID        NOT NULL,
    client_id        UUID        NOT NULL,
    protocol_version SMALLINT    NOT NULL,
    request_hash     BYTEA       NOT NULL,
    base_seq         BIGINT      NOT NULL,
    commit_seq       BIGINT      NOT NULL,
    effect           TEXT        NOT NULL,
    result           JSONB       NOT NULL,
    committed_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (user_id, dataset_id, mutation_id),
    CONSTRAINT mutation_receipts_owner_fk FOREIGN KEY (user_id, dataset_id)
        REFERENCES users (id, dataset_id) ON DELETE CASCADE,
    CONSTRAINT mutation_receipts_dataset_id_not_nil CHECK (
        dataset_id <> '00000000-0000-0000-0000-000000000000'
    ),
    CONSTRAINT mutation_receipts_mutation_id_not_nil CHECK (
        mutation_id <> '00000000-0000-0000-0000-000000000000'
    ),
    CONSTRAINT mutation_receipts_client_id_not_nil CHECK (
        client_id <> '00000000-0000-0000-0000-000000000000'
    ),
    CONSTRAINT mutation_receipts_protocol_version_positive CHECK (protocol_version > 0),
    CONSTRAINT mutation_receipts_request_hash_sha256 CHECK (octet_length(request_hash) = 32),
    CONSTRAINT mutation_receipts_base_seq_nonnegative CHECK (base_seq >= 0),
    CONSTRAINT mutation_receipts_commit_seq_valid CHECK (commit_seq >= base_seq),
    CONSTRAINT mutation_receipts_effect_valid CHECK (effect IN ('applied', 'no_op')),
    CONSTRAINT mutation_receipts_result_object CHECK (jsonb_typeof(result) = 'object')
);

CREATE INDEX mutation_receipts_committed_at_idx ON mutation_receipts (committed_at);

CREATE TABLE account_profiles (
    user_id            UUID        PRIMARY KEY
        REFERENCES users (id) ON DELETE CASCADE,
    display_name       TEXT,

    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT account_profiles_display_name_length CHECK (
        display_name IS NULL
        OR char_length(display_name) BETWEEN 1 AND 80
    )
);

