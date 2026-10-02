import { greet } from "@submilli/greet";

/**
 * Greet `name` using the cross-repo GitHub dependency `@submilli/greet`.
 * @param name Who to greet.
 * @returns The greeting.
 */
export function hello(name: string): string {
    return greet(name);
}
