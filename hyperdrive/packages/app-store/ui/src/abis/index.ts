import { parseAbi } from "viem";

export { encodeMulticalls, encodeIntoMintCall } from "./helpers";

export const HYPERMAP: `0x${string}` = "0x000000000033e5CCbC52Ec7BDa87dB768f9aA93F";
export const MULTICALL: `0x${string}` = "0xcA11bde05977b3631167028862bE2a173976CA11";
export const KINO_ACCOUNT_IMPL: `0x${string}` = "0x000000000012d439e33aAD99149d52A5c6f980Dc";
export const KINO_ACCOUNT_UPGRADABLE_IMPL: `0x${string}` = "0x83119a31628f2c19f578b0cac9a43eaba8d8512b";


export const multicallAbi = parseAbi([
    `function aggregate(Call[] calls) external payable returns (uint256 blockNumber, bytes[] returnData)`,
    `struct Call { address target; bytes callData; }`,
]);

export const hypermapAbi = parseAbi([
    "function mint(address, bytes calldata, bytes calldata, address) external returns (address tba)",
    "function note(bytes calldata,bytes calldata) external returns (bytes32)",
    "function get(bytes32 namehash) external view returns (address tokenBoundAccount, address tokenOwner, bytes memory note)",
]);

export const mechAbi = parseAbi([
    "function execute(address to, uint256 value, bytes calldata data, uint8 operation) returns (bytes memory returnData)",
    "function token() external view returns (uint256,address,uint256)"
])
