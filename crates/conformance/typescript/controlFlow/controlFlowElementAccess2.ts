// @target: es2015
// @strict: true
const config: {
    [key: string]: boolean | { prop: string };
} = null as unknown as ({
    [key: string]: boolean | { prop: string };
});

if (typeof config['works'] !== 'boolean') {
    config.works.prop = 'test'; // ok
    config['works'].prop = 'test'; // error, config['works']: boolean | { 'prop': string }
}
if (typeof config.works !== 'boolean') {
    config['works'].prop = 'test'; // error, config['works']: boolean | { 'prop': string }
    config.works.prop = 'test'; // ok
}


function main(): void {}
