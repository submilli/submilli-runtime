import { Api, Base } from "@test/p1";

// `Base`'s constructor and `label` mention `Token`, which @test/p3 never
// declares as a dependency.
export class Sub extends Base {}

export function viaMid(): string {
  return `${Api.make("m").describe()}|${Api.DEFAULT.describe()}`;
}

export function viaSub(): string {
  return new Sub(Api.make("s")).label();
}
