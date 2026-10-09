// @target: es2015
// @strictNullChecks: true

let cond: boolean = null as unknown as (boolean);

function ff(): void {
    let x: string | undefined = null as unknown as (string | undefined);
    while (true) {
        if (cond) {
            x = "";
        }
        else {
            if (x) {
                x.length;
            }
            if (x) {
                x.length;
            }
        }
    }
}


function main(): void {}
