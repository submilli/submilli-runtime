// expect-error: cannot assign to readonly property `id`
// expect-error-count: 1
interface User { name: string; readonly id: number }
function main(): void {
  const user: Omit<User, "name"> = { id: 1 };
  user.id = 2;
}
