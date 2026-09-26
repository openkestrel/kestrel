ALTER TABLE session ADD COLUMN context_used INTEGER;
ALTER TABLE session ADD COLUMN context_size INTEGER;
ALTER TABLE session ADD COLUMN cost_amount REAL;
ALTER TABLE session ADD COLUMN cost_currency TEXT;
