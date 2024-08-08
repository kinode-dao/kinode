import { parseAbi } from "viem";

export { generateNetworkingKeys } from "./helpers";

// move to constants? // also for anvil/optimism
export const KINOMAP: `0x${string}` = "0xAfA2e57D3cBA08169b416457C14eBA2D6021c4b5";
export const MULTICALL: `0x${string}` = "0xcA11bde05977b3631167028862bE2a173976CA11";
export const KINO_ACCOUNT_IMPL: `0x${string}` = "0xd30217e86A4910f4D7cB3E73fC3CfD28a2C33e4e";
export const DOTOS: `0x${string}` = "0x4f0d377e66E4A2750A928495cE261A345e2f0557";
// export const DOTDEV: `0x${string}` = "0x3BA6AE3eca7ca88af8BbCc8A9d8EA5e665b69Eb3";

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

export const dotOsAbi = parseAbi([
    "function commit(bytes32)",
    "function getCommit(bytes memory, bytes32)",
    "function mint(address,bytes,bytes,bytes,address,bytes32)",
])
