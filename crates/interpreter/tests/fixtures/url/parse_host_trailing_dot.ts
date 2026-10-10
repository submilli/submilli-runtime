// `evil.test.` names the same host as `evil.test`, so `parse` reports every
// trailing-dot spelling as the one host a capability rule is written against.
import { parse, build } from "submilli:url";

function main(): void {
  assert(parse("https://evil.test./").host === "evil.test", "one trailing dot");
  assert(parse("https://evil.test../").host === "evil.test", "repeated trailing dots");
  assert(parse("https://EVIL.test./").host === "evil.test", "upper-case with trailing dot");
  assert(parse("https://evil.test%2e/").host === "evil.test", "percent-encoded trailing dot");
  assert(parse("https://evil.test。/").host === "evil.test", "ideographic full stop");
  assert(parse("https://evil.test.:8443/a").host === "evil.test", "trailing dot before a port");

  assert(parse("https://a.b.evil.test/").host === "a.b.evil.test", "inner dots are kept");
  assert(parse("https://127.0.0.1./").host === "127.0.0.1", "IPv4 literal");
  assert(parse("https://[::1]/").host === "[::1]", "IPv6 literal is unchanged");
  // Without its dots this host would be empty or invalid, so it keeps them.
  assert(parse("https://./").host === ".", "a host of only dots keeps them");
  assert(parse("https://.1../").host === ".1..", "a host that would be invalid keeps them");

  const parsed = parse("https://evil.test.:8443/a?x=1");
  const rebuilt = build(parsed.protocol, parsed.host, parsed.port, parsed.path, parsed.query, parsed.fragment);
  assert(rebuilt === "https://evil.test:8443/a?x=1", "rebuilding uses the normalized host");

  for (const url of ["http://./", "http://.1../x", "https://..:8443/a"]) {
    const p = parse(url);
    const again = build(p.protocol, p.host, p.port, p.path, p.query, p.fragment);
    assert(parse(again).host === p.host && parse(again).path === p.path, url + " rebuilds as " + again);
  }
}
