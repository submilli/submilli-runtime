import { greet } from "@submilli/greet";

/** Greet `name` using the cross-repo GitHub dependency `@submilli/greet`. */
export function hello(name: string): string {
    return greet(name);
}
