# Eight-question calibration set

This set is for mechanics and parameter calibration only. It is not part of
the later 30-question blinded pilot.

The eight classes deliberately cover current state, reference lookup, a deep
revision, multiple acceptable evidence sets, explicit stale state,
current/history conflict, handoff, and absent evidence. Questions use natural
paraphrases while gold references remain exact.

Run artifacts belong under the ignored `snapshots/` and `runs/` directories.
The vault experiment log entry `N0139` is excluded from retrieval because it
records this experiment's procedure and results. Each completed gate is then
recorded as a new `N0139` revision after scoring.

Composer ablation levels are cumulative:

- `strong`: revision chunks only;
- `c1`: strong retrieval plus one anchor per entry;
- `c2`: C1 plus current head;
- `c3`: C2 plus successor/base temporal context;
- `c4`: C3 plus authored graph expansion.
