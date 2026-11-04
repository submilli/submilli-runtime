import { invalidArgument } from "./errors";

export function requiredText(value: string, name: string): string {
    const result = value.trim();
    if (result.length === 0) throw invalidArgument(name + " must not be empty");
    return result;
}

export function amount(value: number, name: string, allowZero: boolean = false): number {
    if (!Number.isFinite(value) || value < 0 || (!allowZero && value === 0)) {
        throw invalidArgument(name + " must be a positive finite amount");
    }
    return value;
}

export function optionalText(value: string | undefined, name: string): string | undefined {
    if (value === undefined) return undefined;
    return requiredText(value, name);
}

export function date(value: string, name: string): string {
    const result = requiredText(value, name);
    if (result.length !== 10 || result.charAt(4) !== "-" || result.charAt(7) !== "-") {
        throw invalidArgument(name + " must be YYYY-MM-DD");
    }
    return result;
}

export function page(offset: number | undefined, limit: number | undefined): { offset: number, limit: number } {
    const resultOffset = offset ?? 0;
    const resultLimit = limit ?? 100;
    if (!Number.isInteger(resultOffset) || resultOffset < 0) throw invalidArgument("offset must be a non-negative integer");
    if (!Number.isInteger(resultLimit) || resultLimit < 1 || resultLimit > 1000) throw invalidArgument("limit must be an integer from 1 through 1000");
    return { offset: resultOffset, limit: resultLimit };
}
