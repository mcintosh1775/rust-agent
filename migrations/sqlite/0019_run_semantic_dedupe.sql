-- Compatibility migration retained for SQLite upgrade history.
-- Historical sqlite installs reached this migration through legacy paths before we
-- consolidated this schema into the baseline initializer.
-- Keep this file as a no-op so upgraded DBs that already have these columns do not
-- fail migration resolution while fresh installs remain in the fast-path baseline.
SELECT 1;

