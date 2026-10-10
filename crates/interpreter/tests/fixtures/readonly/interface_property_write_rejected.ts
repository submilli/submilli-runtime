// expect-error: cannot assign to readonly property `id`
interface Issue {
  readonly id: string;
  title: string;
}

export function main(): string {
  const issue: Issue = { id: "a", title: "t" };
  issue.id = "b";
  return issue.title;
}
