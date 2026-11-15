// Stored JSON cannot manufacture a host-backed value; the descriptor rejects it.
import * as http from "submilli:http";
import * as session from "submilli:session";

function main(): void {
  session.set("response", { status: 200, body: "private" });
  let rejected = false;
  try {
    session.get<http.Response>("response");
  } catch (e) {
    rejected = e instanceof TypeError;
    assert(!e.message.includes("private"), "stored values stay private");
  }
  assert(rejected, "JSON cannot produce a host-backed Response");
  assert(session.get<http.Response>("missing") === undefined, "missing stays undefined");
}
