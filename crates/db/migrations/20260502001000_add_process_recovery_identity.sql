ALTER TABLE execution_processes
  ADD COLUMN os_pid INTEGER;

ALTER TABLE execution_processes
  ADD COLUMN process_group_id INTEGER;

ALTER TABLE execution_processes
  ADD COLUMN command_snapshot TEXT;

ALTER TABLE execution_processes
  ADD COLUMN argv_snapshot TEXT;

ALTER TABLE execution_processes
  ADD COLUMN recovery_reason TEXT
    CHECK (recovery_reason IS NULL OR recovery_reason IN (
      'recovered_running',
      'recovery_orphaned',
      'recovery_exit_unknown'
    ));

CREATE INDEX idx_execution_processes_os_pid
  ON execution_processes(os_pid);

CREATE TABLE startup_recovery_summaries (
    id                              BLOB PRIMARY KEY,
    recovered_at                    TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    running_found                   INTEGER NOT NULL,
    reattached_count                INTEGER NOT NULL,
    orphaned_count                  INTEGER NOT NULL,
    reattached_execution_process_ids TEXT NOT NULL DEFAULT '[]',
    orphaned_execution_process_ids   TEXT NOT NULL DEFAULT '[]',
    created_at                      TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at                      TEXT NOT NULL DEFAULT (datetime('now', 'subsec'))
);
