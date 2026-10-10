// `build` takes each part of a URL as its own argument. A `host` that also
// carries a path, query or fragment, or a `protocol` that is not a bare scheme,
// is refused: the extra text would be parsed into the URL and then silently
// replaced or dropped, so the URL built would not be the one the caller wrote.
import { build } from "submilli:url";

function outcome(protocol: string, host: string, path: string): string {
  try {
    return "built " + build(protocol, host, undefined, path, new Map<string, string>(), undefined);
  } catch (e) {
    if (e instanceof TypeError) return e.message;
    throw e;
  }
}

function refused(protocol: string, host: string, path: string, expected: string): void {
  const message = outcome(protocol, host, path);
  assert(message.startsWith("url.build: ") && message.includes(expected), protocol + " " + host + ": " + message);
}

function main(): void {
  refused("https", "example.com/x", "/orders", "has '/'");
  refused("https", "example.com/x", "", "has '/'");
  refused("https", "example.com\\x", "/orders", "has '\\\\'");
  refused("https", "example.com?q=1", "/orders", "has '?'");
  refused("https", "example.com#top", "/orders", "has '#'");
  refused("https:", "example.com", "/orders", "is not a scheme");
  refused("https://example.com/x?", "y", "", "is not a scheme");
  refused("", "example.com", "/", "is not a scheme");

  const ordinary = new Map<string, string>();
  assert(build("https", "example.com", undefined, "/orders", ordinary, undefined) === "https://example.com/orders", "host name");
  assert(build("https", "example.com:8443", undefined, "/a", ordinary, undefined) === "https://example.com:8443/a", "host with a port");
  assert(build("https", "[::1]", 8080, "/a", ordinary, undefined) === "https://[::1]:8080/a", "IPv6 host");
  assert(build("https", "user:pw@example.com", undefined, "/a", ordinary, undefined) === "https://user:pw@example.com/a", "userinfo");
  assert(build("HTTPS", "example.com", undefined, "/a", ordinary, undefined) === "https://example.com/a", "scheme in upper case");
}
