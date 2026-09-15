export function withReq(url: string, opt: string | null = null): string {
    return opt !== null ? opt : url;
}

export function noArgs(x: string | null = null): string {
    return x !== null ? x : "default";
}
