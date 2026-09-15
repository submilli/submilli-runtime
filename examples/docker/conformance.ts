// Conformance program for the container image, executed through /v1/execute
// against a running container by scripts/docker-conformance.sh.
//
// It deliberately does *not* re-prove language semantics. The fixture suite in
// crates/interpreter/tests/fixtures/ already does that, and those results cannot
// vary by environment: same WasmGC module, same Rust host functions, same answer
// on a laptop and in distroless. What can vary is the boundary where the runtime
// reaches out to the operating system — TLS trust, DNS, the timezone database,
// the sandbox filesystem. Those are what this touches, in one round-trip, using
// the fixture suite's assert-and-trap convention.
//
// The encrypted secret store is deliberately absent from this program, and the
// omission is not a coverage gap. `secrets.get` is refused to main-authored code
// whatever the policy says, so a program cannot read a secret value — it is the
// carve-out working, not an obstacle to route around. The store is proven in
// docker-conformance.sh instead, where registering a blueprint that declares a
// `store:`-sourced secret forces a real decrypt through the mounted key file.

import fs from "submilli:fs";
import http from "submilli:http";

export function main(): string {
  checkOutboundTls();
  checkTimeZoneDatabase();
  checkSandboxFilesystem();
  return "conformance ok: tls, tzdata, vfs";
}

// Exercises reqwest 0.12's bundled webpki-roots and musl's static DNS resolver.
// A certless or resolverless image traps here rather than returning a status.
function checkOutboundTls(): void {
  const response = http.get("https://example.com");
  assert(response.status === 200, `HTTPS GET returned ${response.status}`);
  assert(response.body.length > 0, "HTTPS GET returned an empty body");
}

// Reads /usr/share/zoneinfo. Both offsets are asserted because a stub database
// that resolved every zone to UTC would satisfy a single-offset check.
function checkTimeZoneDatabase(): void {
  const winter = Temporal.ZonedDateTime.from("2026-01-15T12:00:00[America/New_York]");
  assert(winter.offset === "-05:00", `January offset was ${winter.offset}`);
  const summer = Temporal.ZonedDateTime.from("2026-07-15T12:00:00[America/New_York]");
  assert(summer.offset === "-04:00", `July offset was ${summer.offset}`);
}

// The per_session VFS on the mounted volume, written as uid 65532. The 64 KiB
// write is there so the check fails on a volume that is present but not
// writable past the first block, rather than passing on a trivial one.
function checkSandboxFilesystem(): void {
  const info = fs.info();
  assert(info.mode === "per_session", `VFS mode was ${info.mode}`);

  fs.mkdir("/conformance", false);
  fs.writeText("/conformance/note.txt", "durable");
  assert(fs.readText("/conformance/note.txt") === "durable", "readText did not round-trip");

  const stat = fs.stat("/conformance/note.txt");
  assert(stat !== null, "stat returned null for a file just written");
  assert(stat!.kind === "file", `stat reported kind ${stat!.kind}`);
  assert(stat!.size === 7, `stat reported size ${stat!.size}`);

  let bulk = "";
  for (let i = 0; i < 64; i++) {
    bulk += "x".repeat(1024);
  }
  fs.writeText("/conformance/bulk.txt", bulk);
  assert(fs.stat("/conformance/bulk.txt")!.size === 65536, "64 KiB write did not land intact");

  let listed = false;
  for (const entry of fs.list("/conformance", false)) {
    if (entry.name === "note.txt") {
      listed = true;
    }
  }
  assert(listed, "list did not surface a file that exists");
}
