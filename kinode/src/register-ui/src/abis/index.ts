import { parseAbi } from "viem";

export { generateNetworkingKeys } from "./helpers";

// move to constants? // also for anvil/optimism
export const KINOMAP: `0x${string}` = "0x7290Aa297818d0b9660B2871Bb87f85a3f9B4559";
export const MULTICALL: `0x${string}` = "0xcA11bde05977b3631167028862bE2a173976CA11";
export const KINO_ACCOUNT_IMPL: `0x${string}` = "0x58790D9957ECE58607A4b58308BBD5FE1a2e4789";
export const DOTOS: `0x${string}` = "0xcf0af6048D42B2a8e18b1B062a6c4a027D8C5Ea6";
export const DOTDEV: `0x${string}` = "0x3BA6AE3eca7ca88af8BbCc8A9d8EA5e665b69Eb3";

export const multicallAbi = parseAbi([
    `function aggregate(Call[] calls) external payable returns (uint256 blockNumber, bytes[] returnData)`,
    `struct Call { address target; bytes callData; }`,
]);

export const kinomapAbi = parseAbi([
    "function mint(address, bytes calldata, bytes calldata, bytes calldata, address) external returns (address tba)",
    "function note(bytes calldata,bytes calldata) external returns (bytes32)",
    "function get(bytes32 node) external view returns (address tokenBoundAccount, address tokenOwner, bytes memory note)",
]);

export const mechAbi = parseAbi([
    "function execute(address to, uint256 value, bytes calldata data, uint8 operation) returns (bytes memory returnData)",
    "function token() external view returns (uint256,address,uint256)"
])
