import { profile } from "@test/profiles";

function main(): void {
  const result = { ...profile(), extra: 1 };
  assert(result.name === "package" && result.extra === 1, "package interface spread");
  assert(result.note === undefined, "absent package field");
}
