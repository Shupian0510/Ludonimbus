# A cloud as Ludonimbus 0.6.2 wrote it

`cloud/` is the content of a cloud folder (layout 1) made by the released
0.6.2 code, not by hand. It is the input for everything that has to keep
working after an upgrade: reading old snapshots, the migration to layout 2,
restoring every old version.

**Never edit the files in `cloud/`.** They are what a real user's cloud looks
like; a test that needs something else builds it in the test.

## What is in it

Two devices, `PC-A` and `PC-B`, and four games:

| Folder | What it covers |
|---|---|
| `test-game` | 11 snapshots from both devices: plain uploads, a conflict (the losing side kept as `conflict-keep`), a restore of the first version (`pre-restore` copy + `restore`), a manual backup with a note, a locked snapshot with a note |
| `abs-game` | A save folder no root covers: paths recorded as `drive-c/…` |
| `fe-fortunes-weave`, `fe-fortunes-weave-2` | One game added on two devices under two keys (the same built-in rule) |

`expected.json` is what 0.6.2 itself listed when the sample was made: every
snapshot of every game with its trigger, device, note, lock, content hash
and the text of each file. A version restored by a later Ludonimbus must
give exactly these files.

## How it was made

`make_sample.rs` is the generator. It is not compiled here (it uses the
0.6.2 API); to run it again:

```
git worktree add ../ludonimbus-0.6.2 v0.6.2
cp crates/core/tests/fixtures/cloud-0.6/make_sample.rs ../ludonimbus-0.6.2/crates/core/tests/
cd ../ludonimbus-0.6.2
LN_SAMPLE_ROOT='C:\lnfix' cargo test -p ludonimbus-core --test make_sample -- --ignored
```

`LN_SAMPLE_ROOT` must be a short neutral folder: its path ends up inside
the snapshots (`drive-c/lnfix/…`), and nothing personal may be committed.
The result is `cloud/` and `expected.json` in that folder. Ids and times
differ on every run, so a new run replaces the whole sample.
