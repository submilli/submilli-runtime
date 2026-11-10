// @target: es2015
// Each of these types has an error in it. 
// String named and numeric named properties conflict if they would be equivalent after ToNumber on the property name.
class C {
    "1": number;
    "1.0": number; // not a duplicate
    1.0: number;
}

interface I {
    "1": number;
    "1.": number; // not a duplicate
    1: number;
}

let a: {
    "1": number;
    1.0: string;
} = null as unknown as ({
    "1": number;
    1.0: string;
});

let b = {
    "0": '',
    0: ''
}

function main(): void {}
