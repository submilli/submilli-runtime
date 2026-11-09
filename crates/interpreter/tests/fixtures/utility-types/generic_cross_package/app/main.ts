import { pick, read, call, present, names } from "@test/utilities";
type User = { id: number; name: string; email?: string };
function main(): void {
  const user: User = { id: 1, name: "ada" };
  const picked = pick(user, "name");
  const name: string = read(user, "name");
  const count: number = call(() => 3, 0);
  const flag: boolean = present<boolean | null>(null, true);
  const keys = names(user, "id");
  assert(picked.name === "ada" && name === "ada" && count === 0 && flag && keys[0] === "id");
}
