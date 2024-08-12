import { parseAbi } from "viem";

export { generateNetworkingKeys } from "./helpers";

// move to constants? // also for anvil/optimism
export const KINOMAP: `0x${string}` = "0xcA92476B2483aBD5D82AEBF0b56701Bb2e9be658";
export const MULTICALL: `0x${string}` = "0xcA11bde05977b3631167028862bE2a173976CA11";
export const KINO_ACCOUNT_IMPL: `0x${string}` = "0x38766C70a4FB2f23137D9251a1aA12b1143fC716";
export const DOTOS: `0x${string}` = "0x9BD054E4c7753791FA0C138b9713319F62ed235D";

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

export const dotOsAbi = [
    {
        type: 'function',
        name: 'commit',
        stateMutability: 'nonpayable',
        inputs: [
            { name: '_commit', type: 'bytes32' },
        ],
        outputs: [],
    },
    {
        type: 'function',
        name: 'mint',
        stateMutability: 'nonpayable',
        inputs: [
            { name: 'who', type: 'address' },
            { name: 'name', type: 'bytes' },
            { name: 'initialization', type: 'bytes' },
            { name: 'erc721Data', type: 'bytes' },
            { name: 'implementation', type: 'address' },
            { name: 'secret', type: 'bytes32' },
        ],
        outputs: [{ type: 'address' }],
    },
] as const