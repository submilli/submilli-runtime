// @target: es2015
// @strict: true
// @noEmit: true

// https://github.com/microsoft/TypeScript/issues/59652

function mutuallyEnabledPair(): {
    discriminator: true,
    value: string,
  } | {
    discriminator: false,
    value: null | null,
  } { return null as unknown as ({
    discriminator: true,
    value: string,
  } | {
    discriminator: false,
    value: null | null,
  }); }

const { discriminator: discriminator1, value: value1 } = mutuallyEnabledPair()

if (discriminator1) {
  value1;
}

export const { discriminator: discriminator2, value: value2 } = mutuallyEnabledPair()

if (discriminator2) {
  value2;
}

function main(): void {}
