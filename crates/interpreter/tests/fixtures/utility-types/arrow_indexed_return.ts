type User = { id: number; name: string };
function main(): void {
  const id = (user: User): User["id"] => user.id;
  const name = (user: User): User["name"] | undefined => user.name;
  assert(id({ id: 3, name: "n" }) === 3 && name({ id: 3, name: "n" }) === "n", "arrow functions return indexed access types");
}
