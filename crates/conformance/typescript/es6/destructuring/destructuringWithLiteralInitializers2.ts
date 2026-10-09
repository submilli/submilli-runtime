// @target: es2015
// @strict: true

/*pruned*/;                  
function f01([x, y] = []): void {}
function f02([x, y] = [1]): void {}
function f03([x, y] = [1, 'foo']): void {}

/*pruned*/;                      
function f11([x = 0, y] = []): void {}
function f12([x = 0, y] = [1]): void {}
function f13([x = 0, y] = [1, 'foo']): void {}

/*pruned*/;                              
function f21([x = 0, y = 'bar'] = []): void {}
function f22([x = 0, y = 'bar'] = [1]): void {}
function f23([x = 0, y = 'bar'] = [1, 'foo']): void {}

const nx: number | undefined = null as unknown as (number | undefined);
const sx: string | undefined = null as unknown as (string | undefined);

/*pruned*/;                              
function f31([x = 0, y = 'bar'] = []): void {}
function f32([x = 0, y = 'bar'] = [nx]): void {}
function f33([x = 0, y = 'bar'] = [nx, sx]): void {}

/*pruned*/;                              
function f41([x = 0, y = 'bar'] = []): void {}
function f42([x = 0, y = 'bar'] = [sx]): void {}
function f43([x = 0, y = 'bar'] = [sx, nx]): void {}


function main(): void {}
