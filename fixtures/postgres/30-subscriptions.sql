\set ON_ERROR_STOP on

CREATE SCHEMA billing;

CREATE TABLE billing.subscriptions (
  subscription_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  account_id bigint NOT NULL,
  started_on date NOT NULL,
  plan text NOT NULL CHECK (plan IN ('starter', 'growth', 'enterprise')),
  monthly_amount numeric(18, 2) NOT NULL CHECK (monthly_amount >= 0),
  active boolean NOT NULL
);

COMMENT ON TABLE billing.subscriptions IS 'One row per account subscription.';

INSERT INTO billing.subscriptions (account_id, started_on, plan, monthly_amount, active)
VALUES
  (101, '2026-01-01', 'starter', 29.00, true),
  (102, '2026-01-15', 'growth', 99.00, true),
  (103, '2026-02-01', 'enterprise', 499.00, false);

