import { parseAbi } from "viem";

export { generateNetworkingKeys } from "./helpers";

// move to constants? // also for anvil/base
export const HYPERMAP: `0x${string}` = "0x000000000033e5CCbC52Ec7BDa87dB768f9aA93F";
export const MULTICALL: `0x${string}` = "0xcA11bde05977b3631167028862bE2a173976CA11";
export const KINO_ACCOUNT_IMPL: `0x${string}` = "0x000000000012d439e33aAD99149d52A5c6f980Dc";
export const DOTOS: `0x${string}` = "0x091684B4C62DE83b864Db9591600f3751C2025c4";

export const multicallAbi = parseAbi([
    `function aggregate(Call[] calls) external payable returns (uint256 blockNumber, bytes[] returnData)`,
    `struct Call { address target; bytes callData; }`,
]);

export const hypermapAbi = parseAbi([
    "function mint(address, bytes calldata, bytes calldata, bytes calldata, address) external returns (address tba)",
    "function note(bytes calldata,bytes calldata) external returns (bytes32)",
    "function get(bytes32 node) external view returns (address tokenBoundAccount, address tokenOwner, bytes memory note)",
]);

export const mechAbi = parseAbi([
    "function execute(address to, uint256 value, bytes calldata data, uint8 operation) returns (bytes memory returnData)",
    "function token() external view returns (uint256,address,uint256)"
])

export const dotOsAbi = parseAbi([
    "function commit(bytes32 _commit) external",
]);

export const tbaMintAbi = parseAbi([
    "function mint(address who, bytes calldata name, bytes calldata initialization, address implementation) external returns (address)"
]);