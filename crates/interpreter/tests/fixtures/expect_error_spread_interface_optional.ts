// expect-error: expected `string`, got `string | undefined`
interface Profile {
  name?: string;
}

function copy(profile: Profile): string {
  const result = { ...profile };
  return result.name;
}

function main(): void {}
