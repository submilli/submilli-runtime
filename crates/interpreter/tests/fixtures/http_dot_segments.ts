// deny-capability: http.
// A URL parser removes a `.` or `..` path segment, and a `..` takes the segment
// before it along, so a caller's id of `..` would send a package's request to a
// parent endpoint. `submilli:http` refuses such a URL with a `TypeError` before
// any capability check. Every `http.*` capability is denied here, so a refusal
// that reached the policy would be a `PermissionDeniedError` instead, and a URL
// the guard accepts shows up as exactly that. `build` in `submilli:url` refuses
// the same paths, since it would otherwise remove the segment before `get` saw it.
import { get, post, put, patch, delete, head, options, request, download } from "submilli:http";
import { build, encodeComponent } from "submilli:url";

const SPELLINGS: string[] = ["..", ".", "%2E%2E", "%2e%2e", ".%2E", "%2E.", "%2e", "%2E"];

function outcome(send: () => void): string {
  try {
    send();
    return "sent";
  } catch (e) {
    if (e instanceof TypeError) return "refused: " + e.message;
    if (e instanceof PermissionDeniedError) return "policy";
    throw e;
  }
}

function refused(url: string): void {
  const result = outcome(() => { get(url); });
  assert(result.startsWith("refused: "), url + " must be refused, got " + result);
}

function accepted(url: string): void {
  const result = outcome(() => { get(url); });
  assert(result === "policy", url + " must reach the policy, got " + result);
}

class Body {
  serialized: number = 0;
  toJson(): string {
    this.serialized = this.serialized + 1;
    return "{}";
  }
}

function main(): void {
  for (const spelling of SPELLINGS) {
    refused("https://example.com/customers/" + spelling + "/admin");
    refused("https://example.com/customers/" + spelling);
  }

  // Spellings the parser turns into a dot segment before it removes it.
  refused("https://example.com/customers\\..\\admin");
  refused("https://example.com/customers/.\t./admin");
  refused("https://example.com/customers/.. ");
  refused("https:example.com/customers/../admin");
  refused("https://example.com/customers/..?q=1");

  // The message names the segment and the path as written.
  const message = outcome(() => { get("https://example.com/customers/%2E%2E/admin?q=1"); });
  assert(message.includes("http GET"), "names the operation: " + message);
  assert(message.includes("\"%2E%2E\""), "names the segment: " + message);
  assert(message.includes("\"/customers/%2E%2E/admin\""), "names the path: " + message);

  // A dot in a name is not a segment, and the query and fragment are not examined.
  accepted("https://example.com/repo.js");
  accepted("https://example.com/.env");
  accepted("https://example.com/...");
  accepted("https://example.com/a..b");
  accepted("https://example.com/..%20");
  accepted("https://example.com/%252E%252E");
  accepted("https://example.com/x?q=..");
  accepted("https://example.com/x#/../");

  // `encodeComponent("..")` is `..`, so an encoded id is refused too.
  refused("https://example.com/customers/" + encodeComponent("..") + "/admin");

  // Every entry point refuses, before the body is serialized.
  const body = new Body();
  const url = "https://example.com/a/../b";
  assert(outcome(() => { post(url, body); }).startsWith("refused: "), "post");
  assert(outcome(() => { put(url, body); }).startsWith("refused: "), "put");
  assert(outcome(() => { patch(url, body); }).startsWith("refused: "), "patch");
  assert(outcome(() => { delete(url); }).startsWith("refused: "), "delete");
  assert(outcome(() => { head(url); }).startsWith("refused: "), "head");
  assert(outcome(() => { options(url); }).startsWith("refused: "), "options");
  assert(outcome(() => { request("POST", url, body); }).startsWith("refused: "), "request");
  assert(body.serialized === 0, "a refused request does not serialize its body");
  assert(outcome(() => { post("https://example.com/a/b", body); }) === "policy", "accepted post");
  assert(body.serialized === 1, "an accepted request serializes its body before the policy check");

  const downloaded = outcome(() => { download(url, "/out.txt"); });
  assert(downloaded.startsWith("refused: http.download: "), "download: " + downloaded);
  const lowercase = outcome(() => { request("get", url); });
  assert(lowercase.startsWith("refused: http GET: "), "request names the verb in upper case: " + lowercase);

  // `build` would remove the segment itself, leaving `get` nothing to refuse.
  for (const spelling of SPELLINGS) {
    const path = "/customers/" + spelling + "/orders";
    const built = outcome(() => { build("https", "example.com", undefined, path, new Map<string, string>(), undefined); });
    assert(built.startsWith("refused: url.build: "), path + " must be refused by build, got " + built);
  }
  const throughHost = outcome(() => { build("https", "example.com/customers/../admin", undefined, "", new Map<string, string>(), undefined); });
  assert(throughHost.startsWith("refused: url.build: "), "a path inside host is refused too, got " + throughHost);
  const ipv6 = build("https", "[::1]:8080", undefined, "/a.b", new Map<string, string>(), undefined);
  assert(ipv6 === "https://[::1]:8080/a.b", "build accepts a host with a port: " + ipv6);
  const userinfo = build("https", "user:pa..ss@example.com", undefined, "/x", new Map<string, string>(), undefined);
  assert(userinfo === "https://user:pa..ss@example.com/x", "build accepts userinfo: " + userinfo);
  const named = build("https", "example.com", undefined, "/repo.js/...", new Map<string, string>(), undefined);
  assert(named === "https://example.com/repo.js/...", "build keeps dots in names: " + named);
}
