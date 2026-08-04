# Blind paired pilot

`questions.jsonl` is the frozen 30-question Strong-versus-C3 pilot set for the
real vault snapshot. `PRE_REGISTRATION.md` fixes the controls, human rubric,
and decision gates before the first pilot run. `ADJUDICATION.jsonl` preserves
the frozen blind A/B verdicts, and `REPORT.md` records the revealed aggregate
result and gate decision.

The former five-question wiring draft was retired during duplicate review. Its
evidence and claims were near-duplicates of calibration questions, so none of
those five items can count as pilot evidence.

Raw runs, blind keys, and intermediate review packages belong under `runs/`
and stay ignored. Commit the pre-registration, question set, adjudication, and
final aggregate report, but do not put question text or answer analysis into
the vault. Vault progress records may contain only hashes and aggregate
results.
