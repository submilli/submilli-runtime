import { read, relay, Relay } from "@test/live";
function main(): void {
  const value = read();
  const direct: unknown = relay(value);
  assert(direct === null);
  const object = new Relay(value);
  const field: unknown = object.value;
  const method: unknown = object.read();
  const argument: unknown = object.relay(value);
  assert(field === null);
  assert(method === null);
  assert(argument === null);
}
