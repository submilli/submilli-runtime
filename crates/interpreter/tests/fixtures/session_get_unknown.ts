import session from "submilli:session";

function main(): void {
  session.set("k", 1);
  const v = session.get<unknown>("k");
  assert(v === 1, "unknown accepts the stored value");
}
