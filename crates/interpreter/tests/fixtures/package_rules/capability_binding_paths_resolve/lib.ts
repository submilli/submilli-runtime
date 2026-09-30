// expect-error-count: 0
import { check } from "submilli:security";
import { Input } from "./types";

export { Input } from "./types";

/** An input spelled as an alias. */
export type Aliased = {
  /** Owning team. */
  teamId: string;
};

/**
 * Binds through an interface.
 * @capability test.com/interface { owner: $input.teamId }
 */
export function throughInterface(input: Input): void {
  const owner = input.teamId;
  check("test.com/interface", { owner });
}

/**
 * Binds through an alias.
 * @capability test.com/alias { owner: $input.teamId }
 */
export function throughAlias(input: Aliased): void {
  const owner = input.teamId;
  check("test.com/alias", { owner });
}

/**
 * Binds through a nullable parameter.
 * @capability test.com/nullable { owner: $input.teamId }
 */
export function throughNullable(input: Input | null): void {
  const owner = input === null ? "" : input.teamId;
  check("test.com/nullable", { owner });
}

/**
 * Binds the components of a URL.
 * @capability test.com/url { host: $url.host, path: $url.path }
 */
export function throughUrl(url: string): void {
  check("test.com/url", { host: url, path: url });
}
