-- The dispatch configuration of the one work role that can dispatch: what it recorded to have
-- the queue say which limit it enforces. A restart with new flags replaces the record.
CREATE TABLE work_role (
    active_work_slots INTEGER NOT NULL,
    serialized_harnesses TEXT NOT NULL
) STRICT;
