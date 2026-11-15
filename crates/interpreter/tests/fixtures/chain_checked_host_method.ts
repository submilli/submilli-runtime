import { Response } from "submilli:http";

function read(response: Response | null | undefined): { value: number } | undefined {
    return response?.json<{ value: number }>();
}

function main(): void {
    assert(read(null) === undefined, "null receiver skips JSON parsing");
    assert(read(undefined) === undefined, "undefined receiver skips JSON parsing");
}
