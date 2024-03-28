export enum ChainId {
  SEPOLIA = 11155111,
  OPTIMISM = 10,
  OPTIMISM_GOERLI = 420,
  LOCAL = 1337,
}

export const SEPOLIA_OPT_HEX = '0xaa36a7';
export const OPTIMISM_OPT_HEX = '0xa';
export const SEPOLIA_OPT_INT = '11155111';

// Sepolia (for now)
export const PACKAGE_STORE_ADDRESSES = {
  [ChainId.SEPOLIA]: '0x18c39eB547A0060C6034f8bEaFB947D1C16eADF1',
  // [ChainId.OPTIMISM]: '0x8f6e1c9C5a0fE0A7f9Cf0e9b3aF1A9c4f5c6A9e0',
};
