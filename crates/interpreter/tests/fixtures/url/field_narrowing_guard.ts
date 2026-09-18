import { parse, URL } from "submilli:url";
import { info } from "submilli:fs";

function catchesTypeError(f: () => void): boolean {
  try {
    f();
    return false;
  } catch (e: TypeError) {
    return true;
  }
}

class Parent {
  value: unknown = null;
  reset(value: unknown): void { this.value = value; }
}

class Child extends Parent {
  value: URL = parse("https://example.com/path");
}

export function main(): void {
  const child = new Child();
  assert(child.value.host === "example.com", "valid external host backing passes guard");
  child.reset(info());
  assert(catchesTypeError(() => { const value = child.value; }), "unrelated host backing is rejected");
  child.reset({
    protocol: "https",
    host: "evil.example",
    port: null,
    path: "/",
    query: new Map<string, string>(),
    fragment: null,
  });
  assert(catchesTypeError(() => { const value = child.value; }), "structural URL impostor is rejected");

}
