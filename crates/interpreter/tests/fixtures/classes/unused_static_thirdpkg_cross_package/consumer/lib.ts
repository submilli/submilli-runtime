import { Api } from "@test/umid";

export function label(): string {
  return new Api("x").label;
}
