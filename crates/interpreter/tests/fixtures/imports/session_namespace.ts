import session from "submilli:session";

function main(): void {
  session.set("k", 1);
  assert(session.get("k") as number === 1, "namespace import reads and writes");
}
