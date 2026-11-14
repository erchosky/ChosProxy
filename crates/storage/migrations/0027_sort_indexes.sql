-- Quality pass after M31b: sorting the history by response length read
-- and sorted every row (about 90 ms per page at 100 000 exchanges). With
-- an index a page is read in order.
CREATE INDEX idx_exchanges_response_len ON exchanges(response_len);
