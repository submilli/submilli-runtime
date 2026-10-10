// expect-error: package `@test/p2` is not a dependency of `@test/p3`
import { Api } from "@test/p1";
import { Token } from "@test/p2";

export function make(): Token {
  return Api.make("m");
}
