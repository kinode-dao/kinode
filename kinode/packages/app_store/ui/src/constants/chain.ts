export enum ChainId {
  SEPOLIA = 11155111,
  OPTIMISM = 10,
  OPTIMISM_GOERLI = 420,
  LOCAL = 1337,
}

export const SEPOLIA_OPT_HEX = '0xaa36a7';
export const OPTIMISM_OPT_HEX = '0xa';
export const SEPOLIA_OPT_INT = '11155111';
export const OPTIMISM_OPT_INT = '10';

// Optimism (for now)
export const PACKAGE_STORE_ADDRESSES = {
  [ChainId.OPTIMISM]: '0x52185B6a6017E6f079B994452F234f7C2533787B',
  // [ChainId.SEPOLIA]: '0x18c39eB547A0060C6034f8bEaFB947D1C16eADF1',

};
