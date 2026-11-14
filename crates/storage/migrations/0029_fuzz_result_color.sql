-- A highlight colour per Fuzzer result ('' = none).
ALTER TABLE fuzz_results ADD COLUMN color TEXT NOT NULL DEFAULT '';
