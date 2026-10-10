// @target: es2015
// @strictNullChecks: true
function test(strOrNull: string | null, strOrUndefined: string | undefined): void {
    let str: string = "original";
    let nil: null = null as unknown as (null);
    if (null === strOrNull) {
        nil = strOrNull;
    }
    else {
        str = strOrNull;
    }
    if (undefined !== strOrUndefined) {
        str = strOrUndefined;
    }
}


function main(): void {}
