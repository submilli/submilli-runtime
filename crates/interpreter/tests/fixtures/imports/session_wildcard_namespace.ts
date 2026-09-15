import * as session from "submilli:session";

function main(): void {
  session.set("k", true);
  assert(session.get("k") as boolean, "* as ns synonym binds the package as a namespace");
}
