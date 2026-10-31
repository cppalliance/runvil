# The Gateway discovery contract

The Gateway and Workshop meet through a file plus three HTTP calls. Nothing else is shared. This document is the whole of that meeting point: the file format, the two lock files, the stale rule, the shutdown route, the three calls, and the fixtures that pin them.

Both sides are written against this text. Workshop's reader is [`workshop-gateway-discovery`](src/lib.rs), and the Gateway's writer lives in the `runvil-gateway` repository, which spells the same protocol from the other side. When the two disagree, this document is right and one of them has a bug.

- Names in the file: `gateway.json`, `gateway.json.lock`, `gateway.instance.lock`, `gateway.log`
- Endpoints: `GET /health`, `GET /v1/models`, `POST /shutdown`
- Address: `127.0.0.1:{port}`, never `localhost`
- Contract fixtures: [`fixtures/`](fixtures/)

## The discovery file

The Gateway writes `gateway.json` into the run directory once it has bound its loopback port. A reader that finds a live file attaches to that Gateway instead of starting a second one. This is the Jupyter `connection file` pattern.

### The six fields

```json
{
  "port": 8081,
  "api_key": "0e1f8f0d8f8c4e0f9a1c2b3d4e5f6a7b",
  "pid": 4242,
  "epoch": 1757000000,
  "version": "0.2.0",
  "started_at": "2026-09-03T12:00:00Z"
}
```

- `port` (integer, `u16`): the port the Gateway bound. A reader connects to `127.0.0.1:{port}`. Must not be `0`.
- `api_key` (string): the bearer key every authenticated request carries. Must not be empty and must not contain a carriage return or a line feed, because it is written into an `Authorization` header line.
- `pid` (integer, `u32`): the Gateway process id, used by stale detection. Must not be `0`.
- `epoch` (integer, `u64`): the boot instant in unix seconds. `0` means "not recorded", and a file with `0` carries no boot identity, so it reads as stale.
- `version` (string): the Gateway crate version, for diagnostics only.
- `started_at` (string): the same boot instant as an RFC 3339 timestamp, for diagnostics and for boot identity comparison. A whitespace-only value means "not recorded".

### Unknown fields

A reader must tolerate fields it does not know. serde's derived `Deserialize` ignores them, which is what both sides rely on, so a newer Gateway can add a field without breaking an older Workshop.

### Validation

A file is valid when `port` is nonzero, `api_key` is nonempty and holds no line break, and `pid` is nonzero. A file that fails validation is not "absent": it is stale with reason `Invalid`, and the reader deletes it.

A file carries a **boot identity** when `epoch` is nonzero and `started_at` is not blank. A file with valid fields but no boot identity is stale with reason `BootIdentityInvalid`.

### JSON Schema

The schema below is the machine-readable form of the field table. It is written by hand and kept beside the fixtures rather than generated, because the contract is the input to both sides, not an output of either.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://runvil.example/schemas/gateway.json",
  "title": "Gateway discovery file",
  "type": "object",
  "required": ["port", "api_key", "pid", "epoch", "version", "started_at"],
  "additionalProperties": true,
  "properties": {
    "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
    "api_key": { "type": "string", "minLength": 1, "pattern": "^[^\\r\\n]*$" },
    "pid": { "type": "integer", "minimum": 1, "maximum": 4294967295 },
    "epoch": { "type": "integer", "minimum": 0 },
    "version": { "type": "string" },
    "started_at": { "type": "string" }
  }
}
```

`additionalProperties` is `true` on purpose: unknown fields are tolerated. The `api_key` pattern is the "no line break" rule. `epoch` allows `0` even though a zero epoch makes the file stale, because the field is present-and-informational while the boot identity rule decides staleness.

## The run directory and the two locks

### Where things live

- The run directory is `<home>/.runvil/run`, where `<home>` is the user profile directory the OS reports.
- The state directory is `<home>/.runvil`, the run directory's parent.
- `gateway.json`, `gateway.json.lock`, and `gateway.instance.lock` all sit in the run directory, beside each other.
- `gateway.log` sits in `<home>/.runvil/logs/gateway.log`, beside the run directory rather than in it, because the Gateway starts logging before it loads its configuration, so a Gateway that exits on its configuration still records why.

### The atomic owner-only write

The Gateway must never leave a truncated `gateway.json`. A reader racing a writer must see either the whole previous file or the whole new one. So every write goes:

1. to a uniquely named sibling temp file, `gateway.json.<pid>-<counter>.rv-tmp`, so two concurrent writers never fill the same temp;
2. created with mode `0600` on Unix, so the bearer key never exists in a looser file and the rename carries that mode to the target;
3. synced to disk;
4. renamed over `gateway.json`, which is atomic within a directory.

A failed write removes the temp and leaves the target's previous contents. On Windows there are no mode bits, and the file relies on the user profile's ACL, which already restricts it to the owner.

Clean shutdown removes the file with `remove_if_mine`: it removes `gateway.json` only when the file's `pid` is the remover's own, so a shutting-down Gateway cannot delete a replacement's file. A missing file is not an error. An unreadable or foreign file is left for the next reader's stale cleanup.

### `gateway.json.lock` - the launch election

The problem it solves: two readers find no live Gateway at the same moment and both launch one. The Jupyter phantom-server bug class. The lock settles the race so exactly one launches.

- It is an OS advisory lock on an ordinary file in the run directory. Whoever holds it is the launcher.
- The holder **re-validates** `gateway.json` after taking the lock before deciding to launch, because a previous winner may have written the file and released the lock in between.
- While another process holds it, a loser polls every 25 ms. As soon as the winner's file goes live the loser attaches to it.
- A loser never deletes anything. Cleanup of a stale file is the lock holder's privilege, because only the holder knows no launch is in flight.
- A loser that waits out its budget without an attachable Gateway reports a launch timeout.
- Dropping the file handle releases the lock; there is no pid file and no explicit unlock.

### `gateway.instance.lock` - process-lifetime ownership

A different question from the launch election: is a Gateway process already alive on this machine?

- Every Gateway process acquires this lock before it performs any startup side effect, and holds it until the process exits.
- It is separate from `gateway.json.lock` because a Workshop parent may hold the launch election while the spawned Gateway acquires this lease. One lock cannot answer both questions.
- Acquisition is non-blocking. "Another process owns it" is a normal answer, not an error.
- The OS releases it when the owning process dies, so a killed Gateway frees it with no cleanup path.

## The stale rule

A reader attaches only to a file that proves a live, authorized Gateway **right now**. Everything else is stale: the file is deleted and the reader relaunches or falls back to explicit configuration.

The rule, in order. Each step's failure is the reason recorded.

1. **Read.** No file: nothing to attach to and nothing to clean. The file exists but cannot be read: an I/O error, not staleness. The file is not valid JSON, or fails field validation: stale, `Invalid`. The file is deleted.
2. **One process boot, first observation.** Look up the pid's image and start marker. No such process: stale, `ProcessDead`. The image's file name is not `runvil-gateway` (`runvil-gateway.exe` on Windows, case-insensitively): stale, `ImageMismatch`. This is the reused-pid case.
3. **Boot identity.** The file must carry one; see above. Missing: stale, `BootIdentityInvalid`.
4. **One connection, two proofs.** Open one TCP connection to `127.0.0.1:{port}` and, on that same connection, send `GET /health` and then the bearer proof on `GET /v1/models`. Same connection matters: authority cannot come from a listener that replaced the endpoint after the health answer. Failures:
   - health did not answer `200`: stale, `HealthFailed`;
   - the socket was lost after health succeeded: stale, `HealthFailed`, and the reader does not reconnect, because a new connection could reach a different endpoint;
   - the bearer answer was not 2xx: stale, `KeyRejected`.
5. **One process boot, second observation.** Re-read the pid's image and start marker and require them identical to step 2's. The pid died or was reused: stale, `ProcessChanged`.

The two observations bracket the network proof on purpose. That closes the health-to-bearer replacement gap and rejects pid reuse during the interval. The result is a point-in-time proof; it makes no claim that the process is still live after validation returns.

A reader that must not delete (a launch-race loser) runs the same rule but treats the answer as a yes/no and leaves the file alone.

### The stale reasons

Every reader-visible reason, with what it means for an operator.

- `Invalid` - the file was not valid JSON or failed field validation. Something wrote it badly, or it was truncated by a writer that is not following the atomic write.
- `ProcessDead` - the recorded pid names no process. The Gateway exited without withdrawing its file, typically a crash.
- `ImageMismatch` - the pid is alive but runs a different program, so the pid was reused. The recorded Gateway is gone.
- `BootIdentityInvalid` - the file carries no boot identity, so the reader cannot tell one boot from another on the same pid and port.
- `ProcessChanged` - the process identity changed while validation ran. The recorded Gateway died mid-proof.
- `HealthFailed` - the process is the right image but does not answer `GET /health` with `200`. It is wedged or still starting.
- `KeyRejected` - the process is the right image and is healthy, but refused the file's bearer key. The file is from a different boot than the listener.

`HealthFailed` and `KeyRejected` are different operator problems: a wedged Gateway versus a stale key. Keep them distinct.

## `POST /shutdown` and the log

### The shutdown route

A reader holding a validated connection asks the Gateway to exit by posting its bearer key to `POST /shutdown`.

- The request is `POST /shutdown HTTP/1.0` with `Host: 127.0.0.1:{port}` and `Authorization: Bearer {api_key}`.
- A 2xx answer means the Gateway accepted and is draining. The caller returns without waiting for the process to die; the launch election's instance lock is what actually reports the process going away.
- Any other answer is a rejection, reported with its status line.
- A delivery failure is an I/O error, including connect refused and read timeout.
- The request carries an absolute deadline where the caller has one, so cleanup of a child that never answers cannot outlive its budget.

The bearer is never in an error message or in `Debug` output. A rejected shutdown whose status line echoes the bearer must not surface the key.

### The log path

`<home>/.runvil/logs/gateway.log`. A reader that sees a launched Gateway exit without writing a file names this path in the error, because that log is the only place the Gateway's own reason appears.

## The three HTTP calls

All three are raw HTTP over `std::net::TcpStream`: no HTTP client crate, no TLS, no chunked encoding, no keep-alive beyond the one validation connection.

### `GET /health`

- Request: `GET /health HTTP/1.0`, `Host: 127.0.0.1:{port}`, blank line.
- Success: the status code is `200`. The body is drained and ignored.
- Used two ways: as a readiness poll while a launched Gateway comes up, and as the first half of the validation proof.
- The readiness poll retries every 25 ms up to its budget, with a per-attempt connect and read timeout so one hung attempt cannot eat the whole budget. A failed attempt does not end the poll; the budget does, reporting the last attempt's error.

### `GET /v1/models`

- Request: `GET /v1/models HTTP/1.0`, `Host: 127.0.0.1:{port}`, `Authorization: Bearer {api_key}`, blank line.
- Success: a 2xx status. The body is drained and ignored.
- This is the authority proof: a 2xx says the bearer in the file is the bearer the listener accepts. It is sent on the same connection as the health request, in that order.

### `POST /shutdown`

Described above.

### The `Host` header rule

Every one of the three sends `Host: 127.0.0.1:{port}`, the address the caller actually connected to. The Gateway's loopback `Host` allowlist refuses anything else, so sending `localhost` (or a bare host with no port) fails the allowlist and reads as a rejection that has nothing to do with the key. Never send `localhost`.

## Golden fixtures

The `fixtures/` directory holds committed `gateway.json` bytes. They are the drift alarm: a change to the writer's serialization that a reader would misread shows up here.

- [`fixtures/gateway.json`](fixtures/gateway.json) - a canonical file, the shape the Gateway writes.
- [`fixtures/gateway-unknown-field.json`](fixtures/gateway-unknown-field.json) - the same file with one field a reader does not know, which must still parse.
- [`fixtures/gateway-no-boot-identity.json`](fixtures/gateway-no-boot-identity.json) - a valid-fields file with a zero epoch and blank start time, which parses but carries no boot identity.
- [`fixtures/gateway-invalid-port.json`](fixtures/gateway-invalid-port.json) - a zero port, which parses and fails validation.

The tests that read these fixtures live beside the reader. The Gateway's own writer is pinned by the same fixtures from its side.
