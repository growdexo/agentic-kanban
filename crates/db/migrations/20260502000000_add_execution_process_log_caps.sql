ALTER TABLE execution_processes
ADD COLUMN log_bytes_written INTEGER NOT NULL DEFAULT 0;

ALTER TABLE execution_processes
ADD COLUMN log_truncated BOOLEAN NOT NULL DEFAULT FALSE;
