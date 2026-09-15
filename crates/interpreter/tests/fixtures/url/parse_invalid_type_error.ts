// Invalid URLs make url.parse / url.build throw the built-in TypeError
// (mirroring `new URL()` per the WHATWG spec).
import { parse, build } from "submilli:url";

function main(): void {
  let caught = "";
  try {
    const u = parse("not a url");
    caught = u.protocol;
  } catch (e: TypeError) {
    caught = e.name;
  }
  assert(caught === "TypeError", "url.parse invalid input is TypeError");

  let built = "";
  try {
    built = build("https", "example.com", 70000, "/", new Map<string, string>(), null);
    assert(false, "out-of-range port should have thrown");
  } catch (e: TypeError) {
    built = e.name + ":" + e.message;
  }
  assert(
    built === "TypeError:url.build (port): 70000 is not a valid port",
    "url.build bad port is TypeError",
  );
}
