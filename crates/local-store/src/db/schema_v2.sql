ALTER TABLE workspace ADD COLUMN holiday_setup TEXT NOT NULL DEFAULT 'complete'
    CHECK (
        holiday_setup IN ('pending', 'complete')
        AND (workspace_kind = 'local' OR holiday_setup = 'complete')
    );
