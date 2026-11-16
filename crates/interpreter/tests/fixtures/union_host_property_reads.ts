import { parse, URL } from "submilli:url";
function host(value: URL | { host: string }): string {
  return value.host;
}
function optionalHost(value: URL | { host: string } | undefined): string | undefined {
  return value?.host;
}
function port(value: URL | { port: number | undefined }): number | undefined {
  return value.port;
}
function main(): void {
  const url = parse("https://example.com:8443/path");
  assert(host(url) === "example.com");
  assert(host({ host: "local" }) === "local");
  assert(optionalHost(url) === "example.com");
  assert(optionalHost({ host: "local" }) === "local");
  assert(optionalHost(undefined) === undefined);
  assert(port(url) === 8443);
  assert(port(parse("https://example.com/path")) === undefined);
  assert(port({ port: 42 }) === 42);
}
