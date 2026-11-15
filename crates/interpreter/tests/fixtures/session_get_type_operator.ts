import session from "submilli:session";
import { get } from "submilli:session";

function direct<T>(): keyof T | undefined {
  return session.get<keyof T>("key");
}
function indirect<T>(): { field: keyof T } | undefined {
  const read = get;
  return read<{ field: keyof T }>("object");
}
function indexed<T>(): T[keyof T] | undefined {
  return get<T[keyof T]>("key");
}
interface Entry<T> { field: keyof T; }
function interfaced<T>(): Entry<T> | undefined {
  return get<Entry<T>>("object");
}
function rejects(read: () => unknown): boolean {
  try {
    read();
  } catch (e) {
    return e instanceof TypeError && e.message.includes("unresolved type operator");
  }
  return false;
}
function main(): void {
  session.set("key", 42);
  session.set("object", { field: 42 });
  assert(rejects(() => direct<{ right: number }>()), "direct operator");
  assert(rejects(() => indirect<{ right: number }>()), "nested operator via alias");
  assert(rejects(() => indexed<{ right: string }>()), "indexed operator");
  assert(rejects(() => interfaced<{ right: string }>()), "interface member operator");
  assert(get<keyof { right: number }>("missing") === undefined, "concrete keyof");
}
