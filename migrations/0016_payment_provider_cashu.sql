ALTER TABLE payment_requests
  DROP CONSTRAINT IF EXISTS payment_requests_provider_check;

ALTER TABLE payment_requests
  ADD CONSTRAINT payment_requests_provider_check
  CHECK (provider IN ('nwc', 'cashu'));
